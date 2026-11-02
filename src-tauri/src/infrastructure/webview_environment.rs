#[cfg(target_os = "windows")]
pub use crate::infrastructure::windows_api::webview_environment::{browser_process, BrowserProcess};

#[cfg(not(target_os = "windows"))]
pub struct BrowserProcess;

#[cfg(not(target_os = "windows"))]
impl BrowserProcess {
    pub fn exited_within(&self, _timeout: std::time::Duration) -> bool { true }
}

#[cfg(not(target_os = "windows"))]
pub fn browser_process(_window: &tauri::WebviewWindow) -> Option<BrowserProcess> { Some(BrowserProcess) }
