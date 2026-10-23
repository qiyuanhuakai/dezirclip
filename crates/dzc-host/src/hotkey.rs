use crate::Host;
use dzc_core::ports::{Shortcut, ShortcutProvider, ShortcutRegistration};
use dzc_platform::{ipc::CaptureLease, shortcut::{NativeShortcutProvider, normalize_shortcut}};
use dzc_protocol::wire::WireErrorCode;
use std::{sync::{Arc, Weak, mpsc, atomic::{AtomicBool, Ordering}}, thread::JoinHandle, time::{Duration, Instant}};

struct Registration {
    native: Box<dyn ShortcutRegistration>,
    cancel: Arc<AtomicBool>,
    wake: mpsc::SyncSender<()>,
    worker: Option<JoinHandle<()>>,
    done: mpsc::Receiver<()>,
    chord: String,
}
impl Registration {
    fn stop(&mut self) -> Result<(), WireErrorCode> {
        self.cancel.store(true, Ordering::Release);
        let _ = self.wake.try_send(());
        if let Some(worker) = &self.worker { worker.thread().unpark(); }
        self.native.unregister().map_err(|_| WireErrorCode::Busy)?;
        if self.worker.is_some() && self.done.recv_timeout(Duration::from_secs(5)).is_err()
            && !self.worker.as_ref().is_some_and(|worker| worker.is_finished()) { return Err(WireErrorCode::Busy); }
        if let Some(worker) = self.worker.take() { worker.join().map_err(|_| WireErrorCode::Unavailable)?; }
        Ok(())
    }
}
impl Drop for Registration { fn drop(&mut self) { let _ = self.stop(); } }

/// No thread or native registration is created until a nonempty chord is requested.
pub(crate) struct Hotkey {
    host: Weak<Host>, provider: NativeShortcutProvider,
    current: Option<Registration>, retained: Vec<Registration>,
}
impl Hotkey {
    pub(crate) fn new(host: Weak<Host>) -> Self { Self {host, provider:Default::default(), current:None, retained:Vec::new()} }
    fn drain_retained(&mut self) -> Result<(), WireErrorCode> {
        self.provider.drain_failed_registrations().map_err(|_| WireErrorCode::Busy)?;
        let index = 0;
        while index < self.retained.len() {
            self.retained[index].stop()?;
            self.retained.remove(index);
        }
        if self.current.as_ref().is_some_and(|current| current.cancel.load(Ordering::Acquire)) {
            self.current.as_mut().unwrap().stop()?;
            self.current.take();
        }
        Ok(())
    }
    fn retain_until_stopped(&mut self, mut registration: Registration) -> Result<(), WireErrorCode> {
        match registration.stop() { Ok(()) => Ok(()), Err(error) => { self.retained.push(registration); Err(error) } }
    }
    fn register(&self, chord: &str, lease: Arc<CaptureLease>) -> Result<Registration, WireErrorCode> {
        let (wake, receiver) = mpsc::sync_channel(1);
        let (done_tx, done) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let worker_host = self.host.clone();
        let worker_lease = lease.clone();
        let worker = std::thread::Builder::new().name("dzc-privacy-shortcut".into()).spawn(move || {
            let _lease = worker_lease;
            while receiver.recv().is_ok() {
                if worker_cancel.load(Ordering::Acquire) { break; }
                let deadline = Instant::now() + Duration::from_secs(5);
                loop {
                    if worker_cancel.load(Ordering::Acquire) { break; }
                    let Some(host) = worker_host.upgrade() else { break; };
                    match host.try_toggle_privacy() { Ok(false) if Instant::now() < deadline => (), _ => break }
                    // Retry only an outstanding key press, never an idle poll.
                    std::thread::park_timeout(Duration::from_millis(10));
                }
            }
            let _ = done_tx.send(());
        }).map_err(|_| WireErrorCode::Unavailable)?;
        let callback_wake = wake.clone();
        let native = self.provider.register(&Shortcut {id:"privacy-session".into(),accelerator:chord.into()}, Box::new(move || {
            let _lease = &lease;
            let _ = callback_wake.try_send(());
        }));
        match native {
            Ok(native) => Ok(Registration {native,cancel,wake,worker:Some(worker),done,chord:chord.into()}),
            Err(_) => {
                cancel.store(true, Ordering::Release);
                let _ = wake.try_send(());
                worker.thread().unpark();
                // This worker never waits for a request lock. A pending native
                // worker still holds its own callback lease in the provider.
                if done.recv_timeout(Duration::from_secs(5)).is_ok() || worker.is_finished() { let _ = worker.join(); }
                self.provider.drain_failed_registrations().map_err(|_| WireErrorCode::Busy)?;
                Err(WireErrorCode::UnsupportedCapability)
            }
        }
    }
    pub(crate) fn set(&mut self, value: &str) -> Result<String, WireErrorCode> {
        self.drain_retained()?;
        let chord = if value.is_empty() { String::new() } else { normalize_shortcut(value).map_err(|_| WireErrorCode::InvalidRequest)? };
        let host = self.host.upgrade().ok_or(WireErrorCode::Unavailable)?;
        if host.is_stopping() { return Err(WireErrorCode::Busy); }
        let reserved = host.history.reserved_shortcuts().map_err(|_| WireErrorCode::Unavailable)?;
        if !chord.is_empty() && reserved.iter().any(|value| normalize_shortcut(value).is_ok_and(|reserved| reserved == chord)) { return Err(WireErrorCode::Busy); }
        if self.current.as_ref().is_some_and(|current| current.chord == chord) && self.provider.capability().active { return Ok(chord); }
        let mut next = if chord.is_empty() { None } else {
            let lease = host.lifecycle.lock().map_err(|_| WireErrorCode::Unavailable)?.as_ref()
                .and_then(|lifecycle| lifecycle.capture_lease()).ok_or(WireErrorCode::Unavailable)?;
            Some(self.register(&chord, lease)?)
        };
        if let Some(current) = self.current.as_mut() {
            if let Err(error) = current.stop() {
                if let Some(next) = next.take() { let _ = self.retain_until_stopped(next); }
                return Err(error);
            }
        }
        self.current.take();
        if host.history.privacy().set_registered_hotkey(&chord).is_err() {
            if let Some(next) = next.take() { self.retain_until_stopped(next)?; }
            return Err(WireErrorCode::Unavailable);
        }
        self.current = next;
        host.publish(dzc_protocol::wire::EventKind::ServiceStateChanged, dzc_protocol::wire::EventData {entry_id:None,status:Some("PrivacyHotkeyChanged".into())})?;
        Ok(chord)
    }
    pub(crate) fn stop(&mut self) -> Result<(), WireErrorCode> {
        if let Some(current) = self.current.as_mut() { current.stop()?; }
        self.current.take();
        self.drain_retained()
    }
}
