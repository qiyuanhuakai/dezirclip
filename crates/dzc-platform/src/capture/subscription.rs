use super::ClipboardChange;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
};
use std::time::Duration;

pub(super) type Callback = Arc<dyn Fn(ClipboardChange) + Send + Sync + 'static>;
type Wake = Box<dyn Fn() -> Result<(), String> + Send + Sync>;
#[derive(Default)]
pub(super) struct Control {
    requested: AtomicBool,
    wake: Mutex<Option<Wake>>,
}
impl Control {
    pub(super) fn requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }
    pub(super) fn clear(&self) {
        if let Ok(mut wake) = self.wake.lock() {
            *wake = None;
        }
    }
    pub(super) fn install(&self, wake: Wake) -> Result<(), String> {
        let mut slot = self
            .wake
            .lock()
            .map_err(|_| "ClipboardCancellationUnavailable")?;
        *slot = Some(wake);
        if self.requested() {
            if let Some(wake) = &*slot {
                wake()?;
            }
        }
        Ok(())
    }
    fn cancel(&self) -> Result<(), String> {
        self.requested.store(true, Ordering::Release);
        if let Some(wake) = &*self
            .wake
            .lock()
            .map_err(|_| "ClipboardCancellationUnavailable")?
        {
            wake()?;
        }
        Ok(())
    }
}

/// A worker remains owned even when startup or stop times out. The Host must
/// retain its capture lock until stop confirms the thread has actually joined.
pub struct ClipboardSubscription {
    control: Arc<Control>,
    ready: mpsc::Receiver<Result<(), String>>,
    done: mpsc::Receiver<()>,
    worker: Option<std::thread::JoinHandle<Result<(), String>>>,
    ready_result: Option<Result<(), String>>,
}
impl ClipboardSubscription {
    pub fn wait_ready(&mut self, timeout: Duration) -> Result<(), String> {
        if let Some(result) = &self.ready_result {
            return result.clone();
        }
        let result = self
            .ready
            .recv_timeout(timeout)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => "ClipboardStartupTimedOut".to_owned(),
                mpsc::RecvTimeoutError::Disconnected => "ClipboardStartupFailed".to_owned(),
            })?;
        self.ready_result = Some(result.clone());
        result
    }
    /// A timeout retains the worker and allows stop to be retried. Success
    /// confirms native event-loop cleanup and joins the thread.
    pub fn stop(&mut self) -> Result<(), String> {
        self.stop_with_timeout(Duration::from_secs(5))
    }
    fn stop_with_timeout(&mut self, timeout: Duration) -> Result<(), String> {
        if self.worker.is_none() {
            return Ok(());
        }
        let signal = self.control.cancel();
        if self.done.recv_timeout(timeout).is_err()
            && !self
                .worker
                .as_ref()
                .is_some_and(|worker| worker.is_finished())
        {
            return Err("ClipboardStopNotConfirmed".into());
        }
        let worker = self.worker.take().expect("worker exists until joined");
        let result = worker
            .join()
            .map_err(|_| "ClipboardWorkerPanicked".to_owned())?;
        signal?;
        result
    }
    pub fn is_stopped(&self) -> bool {
        self.worker.is_none()
    }
    /// Observes liveness only. Ownership may be released only after stop joins.
    pub fn is_finished(&self) -> bool {
        self.worker
            .as_ref()
            .is_none_or(std::thread::JoinHandle::is_finished)
    }
}
impl Drop for ClipboardSubscription {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

pub fn subscribe_clipboard(callback: Callback) -> Result<ClipboardSubscription, String> {
    let control = Arc::new(Control::default());
    let worker_control = control.clone();
    let (ready_tx, ready) = mpsc::sync_channel(1);
    let (done_tx, done) = mpsc::sync_channel(1);
    let worker = std::thread::Builder::new()
        .name("clipboard-native-subscription".into())
        .spawn(move || {
            let ready_sent = Arc::new(AtomicBool::new(false));
            let sent = ready_sent.clone();
            let notify = ready_tx.clone();
            let result =
                super::listener::run_controlled(callback, worker_control.clone(), move || {
                    sent.store(true, Ordering::Release);
                    let _ = notify.send(Ok(()));
                });
            worker_control.clear();
            if !ready_sent.load(Ordering::Acquire) {
                let _ = ready_tx.send(Err(result
                    .clone()
                    .err()
                    .unwrap_or("ClipboardStartupCancelled".into())));
            }
            let _ = done_tx.send(());
            result
        })
        .map_err(|_| "ClipboardWorkerStartFailed".to_owned())?;
    Ok(ClipboardSubscription {
        control,
        ready,
        done,
        worker: Some(worker),
        ready_result: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unconfirmed_stop_keeps_worker_owned_until_retry_joins() {
        let control = Arc::new(Control::default());
        let (_ready_tx, ready) = mpsc::sync_channel(1);
        let (done_tx, done) = mpsc::sync_channel(1);
        let (release_tx, release) = mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            release.recv().unwrap();
            done_tx.send(()).unwrap();
            Ok(())
        });
        let mut subscription = ClipboardSubscription {
            control,
            ready,
            done,
            worker: Some(worker),
            ready_result: None,
        };
        assert_eq!(
            subscription.stop_with_timeout(Duration::from_millis(5)),
            Err("ClipboardStopNotConfirmed".into())
        );
        assert!(!subscription.is_stopped());
        release_tx.send(()).unwrap();
        subscription.stop().unwrap();
        assert!(subscription.is_stopped());
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn isolated_x11_subscription_stops_and_joins() {
        if std::env::var("DEZIRCLIP_RUN_LINUX_CLIPBOARD_SMOKE_TESTS").as_deref() != Ok("1") {
            return;
        }
        assert!(std::env::var_os("WAYLAND_DISPLAY").is_none());
        let marker = std::path::PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap())
            .join("dezirclip-isolated-wayland-fixture");
        assert!(marker.is_file());
        let (tx, rx) = mpsc::channel();
        let mut subscription = subscribe_clipboard(Arc::new(move |change| {
            let _ = tx.send(change.sequence);
        }))
        .unwrap();
        subscription.wait_ready(Duration::from_secs(5)).unwrap();
        // The X11 fallback deliberately establishes an initial poll before the synthetic copy.
        if std::env::var("DEZIRCLIP_DISABLE_LINUX_SERVICES").is_ok() {
            std::thread::sleep(Duration::from_millis(150));
        }
        let mut clipboard = arboard::Clipboard::new().unwrap();
        clipboard
            .set_text("isolated subscription synthetic text")
            .unwrap();
        rx.recv_timeout(Duration::from_secs(3)).unwrap();
        subscription.stop().unwrap();
        assert!(subscription.is_stopped());
        assert!(matches!(
            rx.recv_timeout(Duration::from_millis(20)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
    }
}
