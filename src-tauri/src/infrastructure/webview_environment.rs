use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

static MAIN_BROWSER_PROCESS_EXITED: AtomicBool = AtomicBool::new(true);

pub fn reset_main_browser_process_exit() {
    MAIN_BROWSER_PROCESS_EXITED.store(false, Ordering::SeqCst);
}

pub fn main_browser_process_exited() -> bool {
    MAIN_BROWSER_PROCESS_EXITED.load(Ordering::SeqCst)
}

pub fn mark_main_browser_process_exited() {
    MAIN_BROWSER_PROCESS_EXITED.store(true, Ordering::SeqCst);
}

pub fn wait_for_main_browser_process_exit(timeout: Duration) -> bool {
    let started = Instant::now();
    while !main_browser_process_exited() {
        if started.elapsed() >= timeout {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    true
}

#[cfg(target_os = "windows")]
pub fn watch_main_browser_process_exit(window: &tauri::WebviewWindow) -> bool {
    crate::infrastructure::windows_api::webview_environment::watch_main_browser_process_exit(window)
}

#[cfg(not(target_os = "windows"))]
pub fn watch_main_browser_process_exit(_window: &tauri::WebviewWindow) -> bool {
    MAIN_BROWSER_PROCESS_EXITED.store(true, Ordering::SeqCst);
    true
}
