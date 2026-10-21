//! A narrow URI owner preserves kernel file references without canonicalizing memfd links.
use dzc_core::api::NativeReadLease;
use std::{
    io::Write,
    os::{fd::AsRawFd, unix::net::UnixStream},
    sync::mpsc,
    time::{Duration, Instant},
};
use x11rb::{
    connection::Connection,
    protocol::{
        xproto::{
            AtomEnum, ConnectionExt, CreateWindowAux, EventMask, SelectionNotifyEvent, WindowClass,
        },
        Event,
    },
    wrapper::ConnectionExt as _,
    CURRENT_TIME,
};

pub(super) struct FileOwner {
    cancel: UnixStream,
    done: mpsc::Receiver<()>,
    worker: Option<std::thread::JoinHandle<Result<(), String>>>,
    exports: Option<crate::private_files::BoundExport>,
}
impl FileOwner {
    pub(super) fn start(
        paths: Vec<String>,
        leases: Vec<NativeReadLease>,
        exports: Option<crate::private_files::BoundExport>,
    ) -> Result<Self, String> {
        if paths.is_empty() || paths.len() > 64 || leases.is_empty() {
            return Err("PrivateFileOwnerRejected".into());
        }
        let (cancel, trigger) = UnixStream::pair().map_err(|_| "PrivateFileOwnerUnavailable")?;
        let (ready_tx, ready) = mpsc::sync_channel(1);
        let (done_tx, done) = mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("private-x11-file-owner".into())
            .spawn(move || {
                let result = run(paths, leases, cancel, || {
                    let _ = ready_tx.send(Ok(()));
                });
                if let Err(error) = &result {
                    let _ = ready_tx.try_send(Err(error.clone()));
                }
                let _ = done_tx.send(());
                result
            })
            .map_err(|_| "PrivateFileOwnerUnavailable")?;
        let mut owner = Self {
            cancel: trigger,
            done,
            worker: Some(worker),
            exports,
        };
        match ready.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(())) => Ok(owner),
            _ => {
                let _ = owner.stop();
                if !owner.is_stopped() {
                    retain(owner);
                }
                Err("PrivateFileOwnerUnavailable".into())
            }
        }
    }
    pub(super) fn stop(&mut self) -> Result<(), String> {
        if self.worker.is_none() {
            return self.close_export();
        }
        let _ = (&self.cancel).write_all(&[1]);
        if self.done.recv_timeout(Duration::from_secs(5)).is_err()
            && !self
                .worker
                .as_ref()
                .is_some_and(|worker| worker.is_finished())
        {
            return Err("PrivateFileStopNotConfirmed".into());
        }
        self.wait()
    }
    pub(super) fn is_stopped(&self) -> bool {
        self.worker.is_none() && self.exports.is_none()
    }
    pub(super) fn wait(&mut self) -> Result<(), String> {
        let result = match self.worker.take() {
            Some(worker) => worker.join().map_err(|_| "PrivateFileOwnerPanicked")?,
            None => Ok(()),
        };
        self.close_export()?;
        result
    }
    fn close_export(&mut self) -> Result<(), String> {
        if let Some(export) = self.exports.as_mut() {
            export.close()?;
        }
        self.exports = None;
        Ok(())
    }
}
impl Drop for FileOwner {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
static PENDING: std::sync::OnceLock<std::sync::Mutex<Vec<FileOwner>>> = std::sync::OnceLock::new();
pub(super) fn retain(owner: FileOwner) {
    PENDING
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(owner);
}
pub(super) fn drain() -> Result<(), String> {
    if let Some(owners) = PENDING.get() {
        let mut owners = owners.lock().map_err(|_| "PrivateFileStopNotConfirmed")?;
        owners.retain_mut(|owner| {
            let _ = owner.stop();
            !owner.is_stopped()
        });
        if !owners.is_empty() {
            return Err("PrivateFileStopNotConfirmed".into());
        }
    }
    Ok(())
}
fn run(
    paths: Vec<String>,
    leases: Vec<NativeReadLease>,
    cancel: UnixStream,
    ready: impl FnOnce(),
) -> Result<(), String> {
    crate::runtime_display::validate_x11_display().map_err(|_| "PrivateFileOwnerUnavailable")?;
    let (conn, screen) = x11rb::connect(None).map_err(|_| "PrivateFileOwnerUnavailable")?;
    let window = conn
        .generate_id()
        .map_err(|_| "PrivateFileOwnerUnavailable")?;
    conn.create_window(
        x11rb::COPY_DEPTH_FROM_PARENT,
        window,
        conn.setup().roots[screen].root,
        0,
        0,
        1,
        1,
        0,
        WindowClass::INPUT_ONLY,
        0,
        &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
    )
    .map_err(|_| "PrivateFileOwnerUnavailable")?
    .check()
    .map_err(|_| "PrivateFileOwnerUnavailable")?;
    let atom = |name: &[u8]| -> Result<u32, String> {
        conn.intern_atom(false, name)
            .map_err(|_| "PrivateFileOwnerUnavailable")?
            .reply()
            .map(|reply| reply.atom)
            .map_err(|_| "PrivateFileOwnerUnavailable".into())
    };
    let clipboard = atom(b"CLIPBOARD")?;
    let targets = atom(b"TARGETS")?;
    let uri = atom(b"text/uri-list")?;
    let secret = atom(b"x-kde-passwordManagerHint")?;
    let uris = paths
        .iter()
        .map(|path| super::file_uri(path) + "\r\n")
        .collect::<String>();
    if uris.len() > 16 * 1024 {
        return Err("PrivateFileOwnerRejected".into());
    }
    if leases.iter().any(|lease| !lease.is_current()) {
        return Err("PrivacyEpochChanged".into());
    }
    conn.set_selection_owner(window, clipboard, CURRENT_TIME)
        .map_err(|_| "PrivateFileOwnerUnavailable")?
        .check()
        .map_err(|_| "PrivateFileOwnerUnavailable")?;
    if conn
        .get_selection_owner(clipboard)
        .map_err(|_| "PrivateFileOwnerUnavailable")?
        .reply()
        .map_err(|_| "PrivateFileOwnerUnavailable")?
        .owner
        != window
    {
        return Err("PrivateFileOwnerUnavailable".into());
    }
    ready();
    let expires = Instant::now() + Duration::from_secs(300);
    loop {
        if leases.iter().any(|lease| !lease.is_current()) || Instant::now() >= expires {
            return Ok(());
        }
        crate::wayland::io::check_cancel(Some(&cancel)).map_err(|_| "PrivateFileOwnerCancelled")?;
        while let Some(event) = conn
            .poll_for_event()
            .map_err(|_| "PrivateFileOwnerDisconnected")?
        {
            match event {
                Event::SelectionClear(_) => return Ok(()),
                Event::SelectionRequest(request) => {
                    let property = if request.property == 0 {
                        request.target
                    } else {
                        request.property
                    };
                    let allowed = request.selection == clipboard
                        && leases.iter().all(NativeReadLease::is_current);
                    let success = if !allowed {
                        false
                    } else if request.target == targets {
                        conn.change_property32(
                            x11rb::protocol::xproto::PropMode::REPLACE,
                            request.requestor,
                            property,
                            AtomEnum::ATOM,
                            &[targets, uri, secret],
                        )
                        .map_err(|_| "PrivateFileOwnerDisconnected")?
                        .check()
                        .is_ok()
                    } else if request.target == uri || request.target == secret {
                        let bytes = if request.target == uri {
                            uris.as_bytes()
                        } else {
                            b"secret"
                        };
                        conn.change_property8(
                            x11rb::protocol::xproto::PropMode::REPLACE,
                            request.requestor,
                            property,
                            request.target,
                            bytes,
                        )
                        .map_err(|_| "PrivateFileOwnerDisconnected")?
                        .check()
                        .is_ok()
                    } else {
                        false
                    };
                    let notification = SelectionNotifyEvent {
                        response_type: x11rb::protocol::xproto::SELECTION_NOTIFY_EVENT,
                        sequence: 0,
                        time: request.time,
                        requestor: request.requestor,
                        selection: request.selection,
                        target: request.target,
                        property: if success { property } else { 0 },
                    };
                    let _ = conn
                        .send_event(false, request.requestor, EventMask::NO_EVENT, notification)
                        .map_err(|_| "PrivateFileOwnerDisconnected")?
                        .check();
                    conn.flush().map_err(|_| "PrivateFileOwnerDisconnected")?;
                }
                _ => (),
            }
        }
        match crate::wayland::io::wait(
            conn.stream().as_raw_fd(),
            libc::POLLIN,
            Some(&cancel),
            Instant::now() + Duration::from_millis(100),
        ) {
            Ok(()) | Err(crate::wayland::WaylandError::TimedOut) => (),
            Err(crate::wayland::WaylandError::Cancelled) => return Ok(()),
            Err(_) => return Err("PrivateFileOwnerDisconnected".into()),
        }
    }
}
