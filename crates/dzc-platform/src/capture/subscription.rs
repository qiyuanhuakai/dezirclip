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
    #[cfg(target_os = "linux")]
    fn regression_runtime(flag: &str) -> Option<std::path::PathBuf> {
        if std::env::var(flag).as_deref() != Ok("1") { return None; }
        let root = std::path::PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap());
        assert!(root.is_absolute());
        assert_eq!(std::fs::read(root.join("p94-private-native-fixture")).unwrap(), b"synthetic-p94-native-only\n");
        Some(root)
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn isolated_pending_wayland_setup_cancels_as_a_successful_stop() {
        let Some(root) = regression_runtime("DEZIRCLIP_RUN_STARTUP_CANCEL_FIXTURE") else { return; };
        assert_eq!(std::env::var("WAYLAND_DISPLAY").unwrap(), "wayland-p94-setup");
        assert!(std::env::var_os("DISPLAY").is_none());
        use std::io::Read;
        let listener = std::os::unix::net::UnixListener::bind(root.join("wayland-p94-setup")).unwrap();
        listener.set_nonblocking(true).unwrap();
        let (started_tx, started) = mpsc::sync_channel(1);
        let (finish_tx, finish) = mpsc::sync_channel(1);
        let server = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(std::time::Instant::now() < deadline);
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("Private setup connection failed: {error}"),
                }
            };
            stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut request = [0u8; 64];
            assert!(stream.read(&mut request).unwrap() > 0);
            started_tx.send(()).unwrap();
            let _ = finish.recv_timeout(Duration::from_secs(5));
        });
        let mut subscription = subscribe_clipboard(Arc::new(|_| panic!("Setup must not emit capture events"))).unwrap();
        started.recv_timeout(Duration::from_secs(5)).unwrap();
        subscription.stop().unwrap();
        assert!(subscription.is_stopped());
        assert_eq!(subscription.wait_ready(Duration::from_secs(1)), Err("ClipboardStartupCancelled".into()));
        finish_tx.send(()).unwrap();
        server.join().unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn isolated_established_xfixes_loss_falls_back_and_recovers_after_server_restart() {
        let Some(root) = regression_runtime("DEZIRCLIP_RUN_XFIXES_RESTART_FIXTURE") else { return; };
        assert!(std::env::var_os("WAYLAND_DISPLAY").is_none());
        assert_eq!(super::super::probe_clipboard_monitor().unwrap(), "x11-xfixes");
        let (tx, rx) = mpsc::channel();
        let mut subscription = subscribe_clipboard(Arc::new(move |event| { let _ = tx.send(event.sequence); })).unwrap();
        subscription.wait_ready(Duration::from_secs(5)).unwrap();
        std::fs::write(root.join("p94-xfixes-ready"), b"ready\n").unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !root.join("p94-x11-restarted").is_file() {
            assert!(std::time::Instant::now() < deadline, "Private X server restart was not confirmed");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(subscription.worker.as_ref().is_some_and(|worker| !worker.is_finished()), "Established XFixes failure ended capture instead of polling");
        let mut clipboard = arboard::Clipboard::new().unwrap();
        clipboard.set_text("synthetic p94 restart first").unwrap();
        std::thread::sleep(Duration::from_millis(250));
        clipboard.set_text("synthetic p94 restart second").unwrap();
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        subscription.stop().unwrap();
        assert!(subscription.is_stopped());
    }
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
