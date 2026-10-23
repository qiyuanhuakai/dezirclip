use dzc_platform::ipc::CaptureLease;
use dzc_protocol::wire::WireErrorCode;
use std::{sync::{Arc, Condvar, Mutex, atomic::{AtomicBool,Ordering}}, time::Duration};

pub(crate) struct LeaseRetention { lease: Arc<CaptureLease>, actions: Arc<dyn crate::HostActions>, confirmed: AtomicBool }
impl LeaseRetention {
    pub(crate) fn new(lease: Arc<CaptureLease>, actions: Arc<dyn crate::HostActions>) -> Arc<Self> { Arc::new(Self {lease,actions,confirmed:AtomicBool::new(false)}) }
    pub(crate) fn confirm(&self) { self.confirmed.store(true,Ordering::Release); }
}
impl Drop for LeaseRetention {
    fn drop(&mut self) {
        if self.confirmed.load(Ordering::Acquire) { return; }
        if self.actions.revoke_private_exports().and_then(|_|self.actions.stop_helpers()).is_ok() { return; }
        // Only a failed shutdown creates this cleanup worker. Linking a Host or
        // running an ordinary CLI creates no timer. A detached native helper
        // cannot make a dropped Host release capture ownership prematurely.
        let retained = Arc::new((self.lease.clone(), self.actions.clone()));
        let worker = retained.clone();
        if std::thread::Builder::new().name("dzc-owner-drain".into()).spawn(move || {
            while worker.1.revoke_private_exports().and_then(|_|worker.1.stop_helpers()).is_err() {
                std::thread::sleep(Duration::from_secs(1));
            }
        }).is_err() {
            // Retaining ownership is preferable to allowing a second capturer
            // when the process cannot create a worker to confirm helper exit.
            std::mem::forget(retained);
        }
    }
}

#[derive(Default)]
pub(crate) struct WorkTracker { active: Mutex<usize>, changed: Condvar }
impl WorkTracker {
    pub(crate) fn begin(self: &Arc<Self>, lease: Arc<CaptureLease>, budget: impl Send + Sync + 'static) -> Result<CaptureWork, WireErrorCode> {
        let mut active = self.active.lock().map_err(|_| WireErrorCode::Unavailable)?;
        *active = active.checked_add(1).filter(|next| *next <= 16).ok_or(WireErrorCode::Busy)?;
        Ok(CaptureWork { _work: Arc::new(Work { tracker: self.clone(), _lease: lease, _budget: Box::new(budget) }) })
    }
    pub(crate) fn drain(&self, timeout: Duration) -> Result<(), WireErrorCode> {
        let active = self.active.lock().map_err(|_| WireErrorCode::Unavailable)?;
        let (active, _) = self.changed.wait_timeout_while(active, timeout, |active| *active != 0).map_err(|_| WireErrorCode::Unavailable)?;
        if *active == 0 { Ok(()) } else { Err(WireErrorCode::Busy) }
    }
}

struct Work { tracker: Arc<WorkTracker>, _lease: Arc<CaptureLease>, _budget: Box<dyn Send + Sync> }
impl Drop for Work {
    fn drop(&mut self) {
        if let Ok(mut active) = self.tracker.active.lock() { *active -= 1; self.tracker.changed.notify_all(); }
    }
}

/// Move this token into follow-up work. Its last clone keeps the real OS lease
/// until that work finishes; joining only the listener cannot complete handoff.
#[derive(Clone)]
pub struct CaptureWork { _work: Arc<Work> }

#[cfg(test)]
pub(crate) fn native_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static SERIAL: Mutex<()> = Mutex::new(());
    SERIAL.lock().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn queued_follow_up_work_keeps_the_real_lease_and_drain_requires_its_last_clone() {
        let _serial = native_test_guard();
        let tracker = Arc::new(WorkTracker::default());
        let lease = CaptureLease::acquire().unwrap();
        let work = tracker.begin(lease.clone(), ()).unwrap();
        let asynchronous = work.clone();
        drop(work);
        drop(lease);
        assert_eq!(tracker.drain(Duration::from_millis(1)), Err(WireErrorCode::Busy));
        assert!(CaptureLease::acquire().is_err());
        drop(asynchronous);
        tracker.drain(Duration::ZERO).unwrap();
        drop(CaptureLease::acquire().unwrap());
    }
    #[test]
    fn a_dropped_owner_retains_its_lease_until_its_unconfirmed_helper_actually_finishes() {
        let _serial=native_test_guard();
        struct Actions {first:AtomicBool,finish:Mutex<std::sync::mpsc::Receiver<()>>,entered:std::sync::mpsc::SyncSender<()>}
        impl crate::HostActions for Actions {
            fn platform_capabilities(&self)->serde_json::Value {serde_json::json!({})}
            fn can_copy(&self)->bool {false}
            fn copy(&self,_:dzc_core::ports::ClipboardPayload)->Result<(),WireErrorCode>{Err(WireErrorCode::Unavailable)}
            fn can_paste(&self)->bool {false}
            fn paste(&self)->Result<(),WireErrorCode>{Err(WireErrorCode::Unavailable)}
            fn revoke_private_exports(&self)->Result<(),WireErrorCode>{Ok(())}
            fn stop_helpers(&self)->Result<(),WireErrorCode>{
                if !self.first.swap(true,Ordering::AcqRel) {return Err(WireErrorCode::Busy);}
                let _=self.entered.try_send(());
                self.finish.lock().unwrap().recv_timeout(Duration::from_secs(2)).map_err(|_|WireErrorCode::Busy)
            }
        }
        let (finish,receiver)=std::sync::mpsc::channel();
        let (entered,observed)=std::sync::mpsc::sync_channel(1);
        let retained=LeaseRetention::new(CaptureLease::acquire().unwrap(),Arc::new(Actions {first:AtomicBool::new(false),finish:Mutex::new(receiver),entered}));
        drop(retained);
        observed.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(CaptureLease::acquire().is_err());
        finish.send(()).unwrap();
        let deadline=std::time::Instant::now()+Duration::from_secs(2);
        loop {if let Ok(lease)=CaptureLease::acquire() {drop(lease);break;} assert!(std::time::Instant::now()<deadline);std::thread::yield_now();}
    }
}
