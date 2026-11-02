use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::sync::mpsc;
use std::time::Duration;
use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};

pub struct BrowserProcess(OwnedHandle);

pub fn configure_main_surface(context: &mut tauri::Context<tauri::Wry>) {
    if windows_version::OsVersion::current().build >= 22000 {
        if let Some(main) = context.config_mut().app.windows.iter_mut().find(|window| window.label == "main") {
            main.transparent = false;
            main.background_color = Some(tauri::webview::Color(0, 0, 0, 0));
        }
    }
}

pub fn extend_main_frame(window: &tauri::WebviewWindow) {
    use windows::Win32::{Foundation::HWND, Graphics::Dwm::DwmExtendFrameIntoClientArea, UI::Controls::MARGINS};
    if window.label() == "main" && windows_version::OsVersion::current().build >= 22000 {
        if let Ok(hwnd) = window.hwnd() {
            let margins = MARGINS { cxLeftWidth: i32::MAX, ..Default::default() };
            if let Err(error) = unsafe { DwmExtendFrameIntoClientArea(HWND(hwnd.0), &margins) } {
                crate::warn!("[main-window] Failed to extend the native frame: {error}");
            }
        }
    }
}

impl BrowserProcess {
    pub fn exited_within(&self, timeout: Duration) -> bool {
        let handle = HANDLE(self.0.as_raw_handle());
        unsafe { WaitForSingleObject(handle, timeout.as_millis() as u32) == WAIT_OBJECT_0 }
    }
}

// Open the handle while the controller still owns this browser. PID reuse and
// callbacks from an earlier WebView environment cannot satisfy the wait.
pub fn browser_process(window: &tauri::WebviewWindow) -> Option<BrowserProcess> {
    let (tx, rx) = mpsc::sync_channel(1);
    window.with_webview(move |view| {
        let process = unsafe {
            let mut pid = 0;
            view.controller().CoreWebView2()
                .and_then(|core| core.BrowserProcessId(&mut pid))
                .and_then(|_| OpenProcess(PROCESS_SYNCHRONIZE, false, pid))
                .map(|handle| BrowserProcess(OwnedHandle::from_raw_handle(handle.0)))
        };
        let _ = tx.send(process.ok());
    }).ok()?;
    rx.recv_timeout(Duration::from_millis(500)).ok().flatten()
}
