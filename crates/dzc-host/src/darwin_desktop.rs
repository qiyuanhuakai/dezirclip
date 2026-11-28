//! Window-independent lifecycle shared by the Darwin helper and the signed probe.
use crate::{
    Grant, Host, darwin_clipboard::DarwinClipboardPump, serve_listener, wire::WireErrorCode,
};
use dzc_platform::ipc::Listener;
use std::{future::Future, io, sync::Arc};
use tokio::sync::watch;

/// Continue clipboard dispatch until the listener has drained admitted requests.
pub async fn serve_darwin_desktop(
    host: Arc<Host>,
    listener: Listener,
    mut clipboard: DarwinClipboardPump,
    grant: Grant,
    parent: impl Future<Output = io::Result<()>>,
) -> io::Result<()> {
    let (shutdown, receiver) = watch::channel(false);
    let server = serve_listener(host.clone(), listener, receiver);
    tokio::pin!(server, parent);
    let mut drain = None;
    let mut parent_result = Ok(());
    let server_result = loop {
        tokio::select! {
            result = &mut server => break result,
            true = clipboard.process_next_write() => {},
            result = &mut parent, if drain.is_none() => {
                parent_result = result;
                host.grants().revoke(&grant);
                let _ = shutdown.send(true);
                let stopping = host.clone();
                drain = Some(tokio::task::spawn_blocking(move || stopping.shutdown()));
            },
        }
    };
    host.grants().revoke(&grant);
    if let Some(worker) = drain {
        let stopped = worker.await.map_err(io::Error::other)?;
        // An admitted request can still hold an operation guard at first.
        // The server has drained by now, so finish the in-process shutdown.
        if stopped == Err(WireErrorCode::Busy) {
            host.shutdown().map_err(wire_error)?;
        } else {
            stopped.map_err(wire_error)?;
        }
    }
    server_result.and(parent_result)
}

pub fn wire_error(code: WireErrorCode) -> io::Error {
    io::Error::other(format!("{code:?}"))
}
