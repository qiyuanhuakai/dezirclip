use crate::{Host, HostLifecycle};
use dzc_core::{capture::*, ports::SourceResolver};
use dzc_platform::{capture::{self as native, ClipboardSubscription, NativeCaptureReader}, ipc::CaptureLease, runtime::NativeSourceResolver};
use dzc_protocol::wire::{EventData, EventKind, WireErrorCode};
use std::{sync::{Arc, Weak, Mutex, mpsc, atomic::{AtomicBool, AtomicUsize, Ordering}}, thread::JoinHandle, time::Duration};

/// Embedded presentation hooks receive the actual Core result. They neither
/// select a new privacy ticket nor reimplement capture classification/storage.
pub trait CaptureHooks: Send + Sync {
    fn source(&self) -> Option<CaptureSource> { None }
    fn paused(&self) -> bool { false }
    fn sequential_echo(&self) -> Option<String> { None }
    fn own_copy(&self, _timestamp: i64) -> OwnClipboardCopy { OwnClipboardCopy::default() }
    fn clear_own_copy(&self) {}
    fn event(&self, _event: CaptureEvent) {}
    fn stored(&self, _capture: StoredCapture, _work: crate::CaptureWork) {}
}
struct HeadlessHooks(Weak<Host>);
impl CaptureHooks for HeadlessHooks {
    fn stored(&self,capture:StoredCapture,work:crate::CaptureWork) {
        let Some(host)=self.0.upgrade() else {return;};
        let observer=self.0.clone();
        let changed=Arc::new(move|id:i64,status:&'static str| {if let Some(host)=observer.upgrade() {let _=host.publish(EventKind::EntryUpdated,EventData{entry_id:Some(id),status:Some(status.into())});}});
        if crate::schedule_capture_ocr(host.history.clone(),&capture,work,changed).is_err() {diagnostic(&host,"CaptureFollowUpUnavailable");}
    }
}

const QUEUED_BYTES: usize = 32 * 1024 * 1024;
struct Budget { bytes: usize, pending: Arc<AtomicUsize> }
impl Drop for Budget { fn drop(&mut self) { self.pending.fetch_sub(self.bytes, Ordering::AcqRel); } }
struct Task { frame: CaptureFrame, source: CaptureSource, timestamp: i64, _budget: Arc<Budget> }
trait NativeWorker: Send {
    fn stop(&mut self) -> Result<(), String>;
    fn is_finished(&self) -> bool;
}
impl NativeWorker for ClipboardSubscription {
    fn stop(&mut self) -> Result<(), String> { ClipboardSubscription::stop(self) }
    fn is_finished(&self) -> bool { ClipboardSubscription::is_finished(self) }
}

struct Running {
    native: Box<dyn NativeWorker>,
    sender: Option<mpsc::SyncSender<Task>>,
    processor: Option<JoinHandle<()>>,
    done: mpsc::Receiver<()>,
    stop: Arc<AtomicBool>,
    _lease: Arc<CaptureLease>,
    work: Arc<crate::work::WorkTracker>,
    retention: Arc<crate::work::LeaseRetention>,
    pending: Arc<AtomicUsize>,
}

pub struct CaptureManager { host: Weak<Host>, running: Mutex<Option<Running>>, hooks: Arc<dyn CaptureHooks> }
impl CaptureManager {
    /// A failed readiness result still returns the owned manager. The Host keeps
    /// its lifecycle until stop confirms both native and processor completion.
    pub fn start(host: &Arc<Host>) -> Result<(Arc<Self>, Result<(), WireErrorCode>), WireErrorCode> {
        Self::start_with_hooks(host, Arc::new(HeadlessHooks(Arc::downgrade(host))))
    }
    pub fn start_with_hooks(host: &Arc<Host>, hooks: Arc<dyn CaptureHooks>) -> Result<(Arc<Self>, Result<(), WireErrorCode>), WireErrorCode> {
        // Reserve the lifecycle before creating either worker. Concurrent start
        // or shutdown cannot strand a manager that failed to attach afterwards.
        let mut lifecycle = host.lifecycle.lock().map_err(|_|WireErrorCode::Unavailable)?;
        if lifecycle.is_some() || host.is_stopping() { return Err(WireErrorCode::Busy); }
        let lease = CaptureLease::acquire().map_err(|_| WireErrorCode::Busy)?;
        let retention = crate::work::LeaseRetention::new(lease.clone(), host.actions.clone());
        let stop = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::sync_channel::<Task>(16);
        let (done_tx, done) = mpsc::sync_channel(1);
        let processor_host = Arc::downgrade(host);
        let processor_stop = stop.clone();
        let processor_lease = lease.clone();
        let work = Arc::new(crate::work::WorkTracker::default());
        let processor_work = work.clone();
        let processor_hooks = hooks.clone();
        let processor_retention = retention.clone();
        let processor = std::thread::Builder::new().name("dzc-host-capture".into()).spawn(move || {
            let _lease = processor_lease;
            while let Ok(task) = receiver.recv() {
                if processor_stop.load(Ordering::Acquire) { break; }
                let Some(host) = processor_host.upgrade() else { break; };
                let Ok(work) = processor_work.begin(_lease.clone(), (task._budget.clone(), processor_retention.clone())) else { diagnostic(&host, "CaptureQueueFull"); continue; };
                let _ = process(&host, task, processor_hooks.as_ref(), work);
            }
            let _ = done_tx.send(());
        }).map_err(|_| WireErrorCode::Unavailable)?;
        let callback_host = Arc::downgrade(host);
        let callback_stop = stop.clone();
        let callback_sender = sender.clone();
        let callback_lease = lease.clone();
        let callback_retention = retention.clone();
        let pending = Arc::new(AtomicUsize::new(0));
        let running_pending = pending.clone();
        let manager_hooks = hooks.clone();
        let notifications = Mutex::new(CaptureNotificationState::at_sequence(native::sequence_number()));
        let subscription = native::subscribe_clipboard(Arc::new(move |change| {
            let _lease = &callback_lease;
            let _retention = &callback_retention;
            if callback_stop.load(Ordering::Acquire) { return; }
            let Some(host) = callback_host.upgrade() else { return; };
            let Ok(mut notifications) = notifications.lock() else { return; };
            if !notifications.accept_sequence(change.sequence) { return; }
            let Ok(timestamp) = crate::router::now_ms() else { return; };
            let Ok(settings) = host.history.capture_settings_snapshot() else { diagnostic(&host, "CaptureSettingsUnavailable"); return; };
            let source = hooks.source().unwrap_or_else(|| {
                let source = NativeSourceResolver.resolve_source().unwrap_or_default();
                CaptureSource { app_name: source.app_name.unwrap_or_default(), process_path: source.process_path }
            });
            let context = CaptureContext { source: source.clone(), timestamp, settings, session: &host.session, paused: hooks.paused(), sequential_echo: hooks.sequential_echo() };
            notifications.set_persistence_mode(context.settings.persistent);
            let store = host.history.capture_store(&host.directory);
            let mut events = HostEvents(&host, hooks.as_ref());
            let Ok(frame) = CaptureEngine::read_authorized(&context, &mut NativeCaptureReader::for_notification(change.sequence), &store, &mut events) else { return; };
            let repeated = if notifications.repeats_file(frame.metadata) {
                match CaptureEngine::file_history_exists(&context, &frame, &store) { Ok(exists) => exists, Err(_) => { diagnostic(&host, "CaptureReadFailed"); return; } }
            } else { false };
            let owner_copy=host.own_copy(timestamp);
            let own_copy=if owner_copy.primary!=0 {owner_copy}else{hooks.own_copy(timestamp)};
            if let Err(reason) = notifications.inspect(frame.metadata, timestamp.max(0) as u64, own_copy, repeated) {
                if reason == CaptureSkip::OwnClipboard { host.clear_copy_echo();hooks.clear_own_copy(); }
                events.emit(CaptureEvent::Skipped(reason));
                return;
            }
            host.clear_copy_echo();hooks.clear_own_copy();
            let bytes = weight(&frame);
            if pending.fetch_update(Ordering::AcqRel, Ordering::Acquire, |previous| previous.checked_add(bytes).filter(|next| *next <= QUEUED_BYTES)).is_err() {
                diagnostic(&host, "CaptureQueueFull"); return;
            }
            let task = Task { frame, source, timestamp, _budget: Arc::new(Budget { bytes, pending: pending.clone() }) };
            if callback_sender.try_send(task).is_err() { diagnostic(&host, "CaptureQueueFull"); }
        }));
        let mut subscription = match subscription {
            Ok(subscription) => subscription,
            Err(_) => { drop(sender); let _ = processor.join(); return Err(WireErrorCode::Unavailable); },
        };
        let ready = subscription.wait_ready(Duration::from_secs(5)).map_err(|_| WireErrorCode::Unavailable);
        let manager = Arc::new(Self { host: Arc::downgrade(host), hooks:manager_hooks, running: Mutex::new(Some(Running { native: Box::new(subscription), sender: Some(sender), processor: Some(processor), done, stop, _lease: lease, work, retention, pending:running_pending })) });
        *lifecycle = Some(manager.clone());
        host.capture_owner.store(true, Ordering::Release);
        Ok((manager, ready))
    }

    pub fn stop(&self) -> Result<(), WireErrorCode> {
        self.stop_with_timeout(Duration::from_secs(5))?;
        if let Some(host) = self.host.upgrade() { host.actions.revoke_private_exports()?; host.actions.stop_helpers()?; }
        self.release_ownership()
    }
    fn stop_with_timeout(&self, timeout: Duration) -> Result<(), WireErrorCode> {
        let mut slot = self.running.lock().map_err(|_| WireErrorCode::Unavailable)?;
        let Some(running) = slot.as_mut() else { return Ok(()); };
        running.stop.store(true, Ordering::Release);
        running.native.stop().map_err(|_| WireErrorCode::Busy)?;
        running.sender.take();
        if running.processor.is_some() && running.done.recv_timeout(timeout).is_err() && !running.processor.as_ref().is_some_and(|worker| worker.is_finished()) { return Err(WireErrorCode::Busy); }
        let joined = running.processor.take().map(|worker| worker.join());
        running.work.drain(timeout)?;
        if let Some(host) = self.host.upgrade() { host.capture_owner.store(false, Ordering::Release); }
        if joined.is_some_and(|result| result.is_err()) { Err(WireErrorCode::Unavailable) } else { Ok(()) }
    }
}
impl HostLifecycle for CaptureManager {
    fn begin_work(&self)->Result<crate::CaptureWork,WireErrorCode> {
        let slot=self.running.lock().map_err(|_|WireErrorCode::Unavailable)?;
        let running=slot.as_ref().filter(|running|!running.stop.load(Ordering::Acquire)).ok_or(WireErrorCode::Busy)?;
        running.work.begin(running._lease.clone(),running.retention.clone())
    }
    fn capture_generated(&self, frame:CaptureFrame, source:CaptureSource, timestamp:i64) -> Result<bool,WireErrorCode> {
        let host=self.host.upgrade().ok_or(WireErrorCode::Unavailable)?;
        let slot=self.running.lock().map_err(|_|WireErrorCode::Unavailable)?;
        let running=slot.as_ref().filter(|running|!running.stop.load(Ordering::Acquire)).ok_or(WireErrorCode::Busy)?;
        let bytes=weight(&frame);
        running.pending.fetch_update(Ordering::AcqRel,Ordering::Acquire,|previous|previous.checked_add(bytes).filter(|next|*next<=QUEUED_BYTES)).map_err(|_|WireErrorCode::Busy)?;
        let budget=Arc::new(Budget{bytes,pending:running.pending.clone()});
        let work=running.work.begin(running._lease.clone(),(budget.clone(),running.retention.clone()))?;
        drop(slot);
        process(&host,Task{frame,source,timestamp,_budget:budget},self.hooks.as_ref(),work)
    }
    fn capture_lease(&self) -> Option<Arc<CaptureLease>> {
        self.running.lock().ok()?.as_ref().filter(|running| !running.stop.load(Ordering::Acquire)).map(|running| running._lease.clone())
    }
    fn stop_capture(&self) -> Result<(), WireErrorCode> { self.stop_with_timeout(Duration::from_secs(5)) }
    fn release_ownership(&self) -> Result<(), WireErrorCode> {
        let mut slot = self.running.lock().map_err(|_| WireErrorCode::Unavailable)?;
        if slot.as_ref().is_some_and(|running| running.processor.is_some() || !running.native.is_finished()) { return Err(WireErrorCode::Busy); }
        if let Some(running) = slot.as_ref() { running.work.drain(Duration::ZERO)?; running.retention.confirm(); }
        *slot = None;
        Ok(())
    }
    fn capture_active(&self) -> bool {
        self.running.lock().ok().is_some_and(|slot|slot.as_ref().is_some_and(|running|
            !running.stop.load(Ordering::Acquire) && !running.native.is_finished()
            && running.processor.as_ref().is_some_and(|worker|!worker.is_finished())))
    }
}

fn weight(frame: &CaptureFrame) -> usize {
    match &frame.data {
        CaptureData::Text(text) => text.len(), CaptureData::RichText { text, html } => text.len().saturating_add(html.len()),
        CaptureData::Image { data_url } => data_url.len(), CaptureData::EncodedImage { bytes, .. } | CaptureData::RgbaImage { bytes, .. } => bytes.len(),
        CaptureData::Files(files) => files.iter().fold(0usize, |total, file| total.saturating_add(file.len())),
    }
}
fn process(host: &Host, task: Task, hooks: &dyn CaptureHooks, work: crate::CaptureWork) -> Result<bool,WireErrorCode> {
    let Ok(settings) = host.history.capture_settings_snapshot() else { diagnostic(host, "CaptureSettingsUnavailable"); return Err(WireErrorCode::Unavailable); };
    let context = CaptureContext { source: task.source, timestamp: task.timestamp, settings, session: &host.session, paused: hooks.paused(), sequential_echo: hooks.sequential_echo() };
    let store = host.history.capture_store(&host.directory);
    match CaptureEngine::capture_frame(&context, task.frame, &store, &mut HostEvents(host, hooks)) {
        CaptureOutcome::Stored(capture) => {hooks.stored(capture,work);Ok(true)},
        CaptureOutcome::Skipped(CaptureSkip::PrivacyEpochChanged)=>Err(WireErrorCode::PrivacyBlocked),
        CaptureOutcome::Skipped(_)=>Ok(false),
        CaptureOutcome::Rejected(_)=>Err(WireErrorCode::PrivacyBlocked),
        CaptureOutcome::Failed(_)=>Err(WireErrorCode::Unavailable),
    }
}
fn diagnostic(host: &Host, status: &str) { let _ = host.publish(EventKind::ServiceStateChanged, EventData { entry_id: None, status: Some(status.into()) }); }
struct HostEvents<'a>(&'a Host, &'a dyn CaptureHooks);
impl EventSink for HostEvents<'_> {
    fn emit(&mut self, event: CaptureEvent) {
        match &event {
            CaptureEvent::Stored { id, .. } => { let _ = self.0.publish(EventKind::EntryAdded, EventData { entry_id: Some(*id), status: None }); },
            CaptureEvent::Removed { id } => { let _ = self.0.publish(EventKind::EntryDeleted, EventData { entry_id: Some(*id), status: None }); },
            CaptureEvent::Failed(_) | CaptureEvent::Rejected(_) => diagnostic(self.0, "CaptureRejected"),
            _ => (),
        }
        self.1.event(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn isolated_native_listener_captures_into_the_real_host_and_releases_its_lease() {
        if std::env::var("DEZIRCLIP_HOST_NATIVE_TEST").as_deref() != Ok("1") { return; }
        let _serial = crate::work::native_test_guard();
        let runtime = std::path::PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").expect("private fixture runtime"));
        assert!(runtime.join("dezirclip-isolated-host-fixture").is_file());
        use dzc_core::ports::ClipboardWriter;
        let host = crate::router::tests::synthetic_host();
        let (capture, ready) = CaptureManager::start(&host).unwrap();
        ready.unwrap();
        assert!(CaptureLease::acquire().is_err());
        let writer = dzc_platform::runtime::EmbeddedClipboardWriter::default();
        use dzc_protocol::wire::{HelloParams,ClientRole,Method,Request,EventsResult};
        let (session,_) = host.hello(HelloParams {protocol_version:dzc_protocol::wire::VERSION,client_version:"native-fixture".into(),client_role:ClientRole::Surface,capabilities:vec![],required_capabilities:vec![],grant:None}).unwrap();
        let text = format!("phase-three native synthetic {}",uuid::Uuid::new_v4());
        writer.write(dzc_core::ports::ClipboardPayload::Text(text.clone())).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let stored=host.session.lock().unwrap().iter().find(|entry|entry.id<0 && entry.content==text).map(|entry|entry.id);
            if let Some(id)=stored {
                let response=host.dispatch(&session,Request {request_id:"native-event".into(),method:Method::EventsSubscribe,params:serde_json::json!({"after_sequence":0,"wait_ms":0})});
                let events:EventsResult=serde_json::from_value(response.result.unwrap()).unwrap();
                if events.events.iter().any(|event|event.event==EventKind::EntryAdded && event.data.entry_id==Some(id)) {
                    assert!(!serde_json::to_string(&events).unwrap().contains(&text));
                    break;
                }
            }
            assert!(std::time::Instant::now() < deadline,"real native notification did not commit a session entry");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(host.snapshot().unwrap().event_sequence > 0);
        writer.stop().unwrap();
        host.shutdown().unwrap();
        assert!(host.capture_stopped());
        assert!(capture.running.lock().unwrap().is_none());
        drop(CaptureLease::acquire().unwrap());
    }
    #[test]
    fn dropping_host_after_unconfirmed_stop_cannot_release_live_workers_lease() {
        let _serial = crate::work::native_test_guard();
        struct Unresponsive { worker: JoinHandle<()> }
        impl NativeWorker for Unresponsive {
            fn is_finished(&self) -> bool { self.worker.is_finished() }
            fn stop(&mut self) -> Result<(), String> {
                if self.worker.is_finished() { Ok(()) } else { Err("UnconfirmedSyntheticWorker".into()) }
            }
        }
        let host = crate::router::tests::synthetic_host();
        let lease = CaptureLease::acquire().unwrap();
        let (sender, receiver) = mpsc::sync_channel::<Task>(1);
        let (release, blocked) = mpsc::sync_channel::<()>(1);
        let native_lease = lease.clone();
        let native_sender = sender.clone();
        let native_worker = std::thread::spawn(move || {
            let _lease = native_lease;
            let _sender = native_sender;
            let _ = blocked.recv();
        });
        let (done_tx, done) = mpsc::sync_channel(1);
        let (observed_tx, observed) = mpsc::sync_channel(1);
        let processor_lease = lease.clone();
        let processor = std::thread::spawn(move || {
            let _lease = processor_lease;
            while receiver.recv().is_ok() {}
            let _ = done_tx.send(());
            let _ = observed_tx.send(());
        });
        let manager = Arc::new(CaptureManager { host: Arc::downgrade(&host), hooks:Arc::new(HeadlessHooks(Arc::downgrade(&host))), running: Mutex::new(Some(Running {
            native: Box::new(Unresponsive { worker: native_worker }), sender: Some(sender), processor: Some(processor),
            done, stop: Arc::new(AtomicBool::new(false)), _lease: lease.clone(), work: Arc::new(crate::work::WorkTracker::default()),
            retention: crate::work::LeaseRetention::new(lease, host.actions.clone()),
            pending:Arc::new(AtomicUsize::new(0)),
        })) });
        host.attach_lifecycle(manager.clone()).unwrap();
        assert_eq!(manager.stop_with_timeout(Duration::from_millis(5)), Err(WireErrorCode::Busy));
        let weak_manager = Arc::downgrade(&manager);
        drop(manager);
        drop(host);
        assert!(weak_manager.upgrade().is_none());
        assert!(CaptureLease::acquire().is_err(), "live native and processor workers still own the native lease");
        release.send(()).unwrap();
        observed.recv_timeout(Duration::from_secs(2)).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            if let Ok(recovered) = CaptureLease::acquire() { drop(recovered); break; }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}
