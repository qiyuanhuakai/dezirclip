pub fn start(window: &tauri::WebviewWindow, paths: Vec<String>, done: impl FnOnce(Result<(), String>) + Send + 'static) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static ACTIVE: AtomicBool = AtomicBool::new(false);
    if ACTIVE.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
        done(Err("FileDragBusy".into()));
        return;
    }
    let done = move |result| {
        ACTIVE.store(false, Ordering::Release);
        done(result);
    };
    #[cfg(target_os = "windows")]
    super::windows_api::file_drag::start(window, paths, done);
    #[cfg(target_os = "linux")]
    super::linux_api::file_drag::start(window, paths, done);
}
