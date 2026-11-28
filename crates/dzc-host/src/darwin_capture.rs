//! Sample only changed pasteboard selections and commit through Core's capture policy.
use crate::{Host, HostLifecycle, darwin_clipboard::DarwinClipboardPump, router::now_ms, wire::*};
use dzc_core::capture::{CaptureContext, CaptureEngine, CaptureEvent, CaptureSource, EventSink};
use dzc_platform::ipc::CaptureLease;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tokio::task::JoinHandle;

struct CaptureOwner {
    active: AtomicBool,
    lease: Mutex<Option<Arc<CaptureLease>>>,
}

impl HostLifecycle for CaptureOwner {
    fn capture_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    fn stop_capture(&self) -> Result<(), WireErrorCode> {
        self.active.store(false, Ordering::Release);
        Ok(())
    }

    fn release_ownership(&self) -> Result<(), WireErrorCode> {
        self.lease
            .lock()
            .map_err(|_| WireErrorCode::Unavailable)?
            .take();
        Ok(())
    }
}

pub struct DarwinCapture {
    owner: Arc<CaptureOwner>,
    epoch: u64,
    selection: isize,
    pending: Option<JoinHandle<()>>,
}

impl DarwinCapture {
    pub fn start(host: &Arc<Host>, clipboard: &DarwinClipboardPump) -> Result<Self, WireErrorCode> {
        let epoch = host.snapshot()?.privacy_epoch;
        let lease = CaptureLease::acquire().map_err(|_| WireErrorCode::Busy)?;
        let owner = Arc::new(CaptureOwner {
            active: AtomicBool::new(true),
            lease: Mutex::new(Some(lease)),
        });
        host.attach_lifecycle(owner.clone())?;
        host.capture_owner.store(true, Ordering::Release);
        clipboard.observe_privacy_epoch(epoch);
        Ok(Self {
            owner,
            epoch,
            selection: clipboard.native_clipboard().change_count(),
            pending: None,
        })
    }

    pub(crate) fn sample(&mut self, host: &Arc<Host>, pump: &DarwinClipboardPump) {
        if !self.owner.capture_active() {
            return;
        }
        let clipboard = pump.native_clipboard();
        let selection = clipboard.change_count();
        let epoch = pump.privacy_epoch();
        if !self.selection_is_ready(selection, epoch, pump.owns_selection(selection)) {
            return;
        }
        // A copy request may hold this guard while it awaits the main thread.
        // Retry the still-current selection on a later tick in that case.
        let Ok(authorization) = host.requests.try_lock() else {
            return;
        };
        self.selection = selection;

        let Ok(timestamp) = now_ms() else { return };
        let Ok(state) = host.history.runtime_snapshot(timestamp) else {
            return;
        };
        if state.privacy_epoch != epoch {
            pump.observe_privacy_epoch(state.privacy_epoch);
            self.epoch = state.privacy_epoch;
            return;
        }
        if state.session.active {
            return;
        }
        let Ok(settings) = host.history.capture_settings_snapshot() else {
            return;
        };
        // The general pasteboard provides no verified source application.
        // An exclusion cannot be safely evaluated against a foreground guess.
        if !settings.excluded_source_apps.is_empty() {
            return;
        }
        let source = CaptureSource::default();
        let context = CaptureContext {
            source: source.clone(),
            timestamp,
            settings,
            session: &host.session,
            paused: false,
            sequential_echo: None,
        };
        let store = host.history.capture_store(&host.directory);
        let Ok(frame) = CaptureEngine::read_authorized(
            &context,
            &mut clipboard.capture_reader(selection),
            &store,
            &mut DarwinEvents(host),
        ) else {
            return;
        };
        if frame
            .metadata
            .privacy_ticket
            .is_none_or(|ticket| ticket.epoch() != epoch)
        {
            return;
        }
        let Some(lease) = self.owner.lease.lock().ok().and_then(|lease| lease.clone()) else {
            return;
        };
        drop(authorization);
        let host = host.clone();
        let owner = self.owner.clone();
        self.pending = Some(tokio::task::spawn_blocking(move || {
            let _lease = lease;
            let Ok(_authorization) = host.requests.lock() else {
                return;
            };
            let Ok(settings) = host.history.capture_settings_snapshot() else {
                return;
            };
            if !owner.capture_active() || !settings.excluded_source_apps.is_empty() {
                return;
            }
            let context = CaptureContext {
                source,
                timestamp,
                settings,
                session: &host.session,
                paused: false,
                sequential_echo: None,
            };
            let store = host.history.capture_store(&host.directory);
            CaptureEngine::capture_frame(&context, frame, &store, &mut DarwinEvents(&host));
        }));
    }

    fn selection_is_ready(&mut self, selection: isize, epoch: u64, owned: bool) -> bool {
        if self.epoch != epoch || owned {
            self.epoch = epoch;
            self.selection = selection;
            return false;
        }
        // Leave changed selections pending until the current write is complete.
        // The next tick reads the actual pasteboard again before authorizing it.
        self.selection != selection && self.pending.as_ref().is_none_or(JoinHandle::is_finished)
    }

    pub(crate) async fn finish(&mut self) -> std::io::Result<()> {
        let _ = self.owner.stop_capture();
        if let Some(worker) = self.pending.take() {
            worker.await.map_err(std::io::Error::other)?;
        }
        self.owner
            .release_ownership()
            .map_err(crate::darwin_desktop::wire_error)
    }
}

struct DarwinEvents<'a>(&'a Host);
impl EventSink for DarwinEvents<'_> {
    fn emit(&mut self, event: CaptureEvent) {
        let (kind, id, status) = match event {
            CaptureEvent::Stored { id, .. } => (EventKind::EntryAdded, Some(id), None),
            CaptureEvent::Removed { id } => (EventKind::EntryDeleted, Some(id), None),
            CaptureEvent::Failed(_) | CaptureEvent::Rejected(_) => (
                EventKind::ServiceStateChanged,
                None,
                Some("CaptureRejected".into()),
            ),
            _ => return,
        };
        let _ = self.0.publish(
            kind,
            EventData {
                entry_id: id,
                status,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn changed_selection_remains_pending_until_previous_write_finishes() {
        let (complete, writing) = tokio::sync::oneshot::channel();
        let mut capture = DarwinCapture {
            owner: Arc::new(CaptureOwner {
                active: AtomicBool::new(true),
                lease: Mutex::new(None),
            }),
            epoch: 4,
            selection: 10,
            pending: Some(tokio::spawn(async move {
                writing.await.unwrap();
            })),
        };

        assert!(!capture.selection_is_ready(11, 4, false));
        assert!(!capture.selection_is_ready(12, 4, false));
        assert_eq!(capture.selection, 10);
        complete.send(()).unwrap();
        capture.pending.as_mut().unwrap().await.unwrap();
        assert!(capture.selection_is_ready(12, 4, false));

        assert!(!capture.selection_is_ready(13, 4, true));
        assert!(!capture.selection_is_ready(13, 4, false));
        assert!(!capture.selection_is_ready(14, 5, false));
        assert!(!capture.selection_is_ready(14, 5, false));
        assert!(capture.selection_is_ready(15, 5, false));
    }
}
