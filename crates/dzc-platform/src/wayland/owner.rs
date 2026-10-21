use super::{MimeData, WaylandClipboardBackend, WaylandError};
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::sync::{mpsc, Mutex, OnceLock};
use std::time::Duration;

/// One explicitly requested clipboard owner. It starts no service or process.
pub struct WaylandOwner {
    cancel: UnixStream,
    ready: mpsc::Receiver<Result<(), WaylandError>>,
    readiness: Mutex<Option<Result<(), WaylandError>>>,
    done: mpsc::Receiver<()>,
    thread: Option<std::thread::JoinHandle<Result<(), WaylandError>>>,
    private: bool,
}
impl WaylandOwner {
    pub fn start(payloads: Vec<MimeData>) -> Result<Self, WaylandError> {
        Self::start_authorized(payloads, Vec::new(), None)
    }
    fn start_authorized(
        payloads: Vec<MimeData>,
        leases: Vec<dzc_core::api::NativeReadLease>,
        exports: Option<crate::private_files::BoundExport>,
    ) -> Result<Self, WaylandError> {
        super::validate_payloads(&payloads)?;
        let private = !leases.is_empty();
        let (cancel, trigger) = WaylandClipboardBackend::cancellation_pair()?;
        let (ready_tx, ready) = mpsc::sync_channel(1);
        let (done_tx, done) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("wayland-clipboard-owner".into())
            .spawn(move || {
                let notify = ready_tx.clone();
                let result = WaylandClipboardBackend::serve_authorized(
                    payloads,
                    leases,
                    exports,
                    || {
                        let _ = notify.send(Ok(()));
                    },
                    cancel,
                );
                if let Err(error) = result {
                    let _ = ready_tx.try_send(Err(error));
                }
                let _ = done_tx.send(());
                result
            })
            .map_err(|_| WaylandError::ConnectionUnavailable)?;
        Ok(Self {
            cancel: trigger,
            ready,
            readiness: Mutex::new(None),
            done,
            thread: Some(thread),
            private,
        })
    }
    pub fn wait_ready(&self) -> Result<(), WaylandError> {
        let mut state = self
            .readiness
            .lock()
            .map_err(|_| WaylandError::Disconnected)?;
        if let Some(result) = *state {
            return result;
        }
        let result = self
            .ready
            .recv_timeout(Duration::from_secs(15))
            .map_err(|_| WaylandError::TimedOut)?;
        *state = Some(result);
        result
    }
    pub fn stop(&mut self) -> Result<(), WaylandError> {
        if self.thread.is_none() {
            return Ok(());
        }
        let _ = (&self.cancel).write_all(&[1]);
        if self.done.recv_timeout(Duration::from_secs(5)).is_err()
            && !self
                .thread
                .as_ref()
                .is_some_and(|worker| worker.is_finished())
        {
            return Err(WaylandError::TimedOut);
        }
        self.thread
            .take()
            .expect("owner exists until joined")
            .join()
            .map_err(|_| WaylandError::Disconnected)?
    }
    pub fn is_stopped(&self) -> bool {
        self.thread.is_none()
    }
    pub(crate) fn is_private(&self) -> bool {
        self.private
    }
}
impl Drop for WaylandOwner {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// GUI writes keep one owner alive in this process until the next copy or exit.
/// A failed new negotiation retains the previous owner.
static OWNER: OnceLock<Mutex<Option<WaylandOwner>>> = OnceLock::new();
static PENDING: OnceLock<Mutex<Vec<WaylandOwner>>> = OnceLock::new();
pub fn write_in_process(payload: crate::native_clipboard::Payload) -> Result<(), WaylandError> {
    write_in_process_with_intent(payload, false)
}
pub fn write_in_process_with_intent(
    payload: crate::native_clipboard::Payload,
    suppress_recapture: bool,
) -> Result<(), WaylandError> {
    let mut slot = OWNER
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| WaylandError::Disconnected)?;
    replace_owner(&mut slot, payload, suppress_recapture)
}
pub(crate) fn replace_owner(
    slot: &mut Option<WaylandOwner>,
    payload: crate::native_clipboard::Payload,
    suppress_recapture: bool,
) -> Result<(), WaylandError> {
    let prepared = crate::native_clipboard::prepare_owner_payload(payload)
        .map_err(|_| WaylandError::PermissionDenied)?;
    let mut fresh = WaylandOwner::start_authorized(
        super::representations_with_intent(prepared.payload, suppress_recapture)?,
        prepared.leases,
        prepared.exports,
    )?;
    if let Err(error) = fresh.wait_ready() {
        stop_or_retain(fresh);
        return Err(error);
    }
    if let Some(mut old) = slot.take() {
        let _ = old.stop();
        if !old.is_stopped() {
            *slot = Some(old);
            stop_or_retain(fresh);
            return Err(WaylandError::TimedOut);
        }
    }
    *slot = Some(fresh);
    Ok(())
}

fn stop_or_retain(mut owner: WaylandOwner) {
    let _ = owner.stop();
    if !owner.is_stopped() {
        PENDING
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(owner);
    }
}
pub(crate) fn revoke_private_owner() -> Result<(), WaylandError> {
    let mut failed = false;
    if let Some(owner) = OWNER.get() {
        let mut slot = owner.lock().map_err(|_| WaylandError::Disconnected)?;
        if let Some(owner) = slot.as_mut().filter(|owner| owner.is_private()) {
            let _ = owner.stop();
            if owner.is_stopped() {
                *slot = None;
            } else {
                failed = true;
            }
        }
    }
    if let Some(pending) = PENDING.get() {
        let mut pending = pending.lock().map_err(|_| WaylandError::Disconnected)?;
        pending.retain_mut(|owner| {
            let _ = owner.stop();
            !owner.is_stopped()
        });
        failed |= !pending.is_empty();
    }
    if failed {
        Err(WaylandError::TimedOut)
    } else {
        Ok(())
    }
}
