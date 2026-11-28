//! Window-independent lifecycle shared by the Darwin helper and the signed probe.
use crate::{
    Grant, Host, darwin_capture::DarwinCapture, darwin_clipboard::DarwinClipboardPump,
    serve_listener, wire::WireErrorCode,
};
use dzc_platform::ipc::Listener;
use std::{future::Future, io, sync::Arc, time::Duration};
use tokio::sync::watch;

/// Continue clipboard dispatch until the listener has drained admitted requests.
pub async fn serve_darwin_desktop(
    host: Arc<Host>,
    listener: Listener,
    mut clipboard: DarwinClipboardPump,
    mut capture: Option<DarwinCapture>,
    grant: Grant,
    parent: impl Future<Output = io::Result<()>>,
) -> io::Result<()> {
    let (shutdown, receiver) = watch::channel(false);
    let server = serve_listener(host.clone(), listener, receiver);
    tokio::pin!(server, parent);
    let mut interval = tokio::time::interval(Duration::from_millis(150));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut drain = None;
    let mut parent_result = Ok(());
    let server_result = loop {
        tokio::select! {
            result = &mut server => break result,
            true = clipboard.process_next_write() => {},
            _ = interval.tick(), if capture.is_some() && drain.is_none() => {
                if let Some(capture) = &mut capture {
                    capture.sample(&host, &clipboard);
                }
            },
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
    if let Some(capture) = &mut capture {
        capture.finish().await?;
    }
    server_result.and(parent_result)
}

pub fn wire_error(code: WireErrorCode) -> io::Error {
    io::Error::other(format!("{code:?}"))
}
