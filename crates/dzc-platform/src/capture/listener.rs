#[cfg(target_os = "windows")]
use super::sequence_number;
use super::ClipboardChange;
use std::sync::Arc;
#[cfg(target_os = "windows")]
use windows::core::PCWSTR;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
#[cfg(target_os = "windows")]
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, RemoveClipboardFormatListener,
};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    GetWindowLongPtrW, PostMessageW, PostQuitMessage, RegisterClassW, SetWindowLongPtrW,
    GWLP_USERDATA, HWND_MESSAGE, MSG, WM_CLIPBOARDUPDATE, WM_CLOSE, WNDCLASSW,
};

type Callback = Arc<dyn Fn(ClipboardChange) + Send + Sync + 'static>;

/// Negotiate the native notification backend without starting a worker or reading data.
pub fn probe_clipboard_monitor() -> Result<&'static str, String> {
    #[cfg(target_os = "linux")]
    {
        if crate::wayland::requested() {
            if crate::wayland::service_disabled("clipboard_listener")
                || crate::wayland::service_disabled("wayland_clipboard")
            {
                return Err("ClipboardMonitorDisabled".into());
            }
            return crate::wayland::WaylandClipboardBackend::probe()
                .map(|status| match status.protocol {
                    crate::wayland::DataControlProtocol::Ext => "wayland-ext-data-control-v1",
                    crate::wayland::DataControlProtocol::Wlr => "wayland-wlr-data-control",
                })
                .map_err(|error| error.to_string());
        }
        super::x11_listener::probe_clipboard_monitor()
    }
    #[cfg(target_os = "windows")]
    unsafe {
        let window = create_listener_window()?;
        let result = AddClipboardFormatListener(window).map_err(|error| error.to_string());
        if result.is_ok() {
            let _ = RemoveClipboardFormatListener(window);
        }
        let _ = DestroyWindow(window);
        result.map(|_| "win32-message-window")
    }
}

/// Keep the existing one-worker desktop lifecycle; embedded callers may run directly.
pub fn listen_clipboard(callback: Callback) {
    let _ = std::thread::Builder::new()
        .name("clipboard-native-listener".into())
        .spawn(move || {
            use dzc_core::ports::ClipboardMonitor;
            let monitor = crate::runtime::NativeClipboardMonitor::default();
            if let Err(error) = monitor.run(Box::new(move |change| {
                callback(ClipboardChange {
                    sequence: change.sequence as u32,
                })
            })) {
                eprintln!("[clipboard-listener] {error}");
            }
        });
}

/// Blocking native event loop. Cancellation is not claimed by this interface.
pub fn run_clipboard_monitor(callback: Callback) -> Result<(), String> {
    run_controlled(
        callback,
        Arc::new(super::subscription::Control::default()),
        || {},
    )
}

pub(super) fn run_controlled(
    callback: Callback,
    control: Arc<super::subscription::Control>,
    ready: impl FnOnce(),
) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        use std::io::Write;
        let (cancel, wake) = crate::wayland::WaylandClipboardBackend::cancellation_pair()
            .map_err(|error| error.to_string())?;
        control.install(Box::new(move || {
            (&wake)
                .write_all(&[1])
                .map_err(|_| "ClipboardCancellationFailed".to_owned())
        }))?;
        if crate::wayland::requested() {
            crate::wayland::WaylandClipboardBackend::monitor_ready(cancel, ready, |sequence| {
                callback(ClipboardChange {
                    sequence: sequence as u32,
                })
            })
            .map_err(|error| error.to_string())
        } else {
            super::x11_listener::run_controlled(callback, cancel, ready)
        }
    }
    #[cfg(target_os = "windows")]
    unsafe {
        let window = create_listener_window()?;
        let pointer = Box::into_raw(Box::new(callback));
        SetWindowLongPtrW(window, GWLP_USERDATA, pointer as isize);
        if let Err(error) = AddClipboardFormatListener(window) {
            SetWindowLongPtrW(window, GWLP_USERDATA, 0);
            let _ = Box::from_raw(pointer);
            let _ = DestroyWindow(window);
            return Err(error.to_string());
        }
        let window_id = window.0 as usize;
        if let Err(error) = control.install(Box::new(move || {
            PostMessageW(
                Some(HWND(window_id as *mut std::ffi::c_void)),
                WM_CLOSE,
                WPARAM(0),
                LPARAM(0),
            )
            .map_err(|_| "ClipboardCancellationFailed".to_owned())
        })) {
            control.clear();
            let _ = RemoveClipboardFormatListener(window);
            SetWindowLongPtrW(window, GWLP_USERDATA, 0);
            let _ = Box::from_raw(pointer);
            let _ = DestroyWindow(window);
            return Err(error);
        }
        if control.requested() {
            control.clear();
            let _ = RemoveClipboardFormatListener(window);
            SetWindowLongPtrW(window, GWLP_USERDATA, 0);
            let _ = Box::from_raw(pointer);
            let _ = DestroyWindow(window);
            return Ok(());
        }
        ready();
        let mut message = MSG::default();
        let result = loop {
            let result = GetMessageW(&mut message, None, 0, 0).0;
            if result == 0 {
                break Ok(());
            }
            if result == -1 {
                break Err("ClipboardEventLoopFailed".to_string());
            }
            DispatchMessageW(&message);
        };
        control.clear();
        let _ = RemoveClipboardFormatListener(window);
        SetWindowLongPtrW(window, GWLP_USERDATA, 0);
        let _ = Box::from_raw(pointer);
        let _ = DestroyWindow(window);
        result
    }
}

#[cfg(target_os = "windows")]
unsafe fn create_listener_window() -> Result<HWND, String> {
    let instance = windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
        .map_err(|error| error.to_string())?;
    let class: Vec<u16> = "DezirClipClipboardListener"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let window_class = WNDCLASSW {
        lpfnWndProc: Some(wnd_proc),
        hInstance: instance.into(),
        lpszClassName: PCWSTR(class.as_ptr()),
        ..Default::default()
    };
    RegisterClassW(&window_class);
    CreateWindowExW(
        Default::default(),
        PCWSTR(class.as_ptr()),
        PCWSTR(std::ptr::null()),
        Default::default(),
        0,
        0,
        0,
        0,
        Some(HWND_MESSAGE),
        None,
        Some(HINSTANCE(instance.0)),
        None,
    )
    .map_err(|error| error.to_string())
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn wnd_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_CLOSE {
        PostQuitMessage(0);
        LRESULT(0)
    } else if message == WM_CLIPBOARDUPDATE {
        let pointer = GetWindowLongPtrW(window, GWLP_USERDATA);
        if pointer != 0 {
            let callback = &*(pointer as *const Callback);
            callback(ClipboardChange {
                sequence: sequence_number(),
            });
        }
        LRESULT(0)
    } else {
        DefWindowProcW(window, message, wparam, lparam)
    }
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    #[test]
    fn invalid_display_is_rejected_before_native_connections() {
        const CHILD: &str = "DEZIRCLIP_INVALID_DISPLAY_TEST_CHILD";
        if std::env::var_os(CHILD).is_some() {
            use dzc_core::capture::{CaptureReadRequest, CaptureReader};
            assert_eq!(
                super::probe_clipboard_monitor().unwrap_err(),
                "X11DisplayInvalid"
            );
            assert_eq!(
                super::run_clipboard_monitor(std::sync::Arc::new(|_| {})).unwrap_err(),
                "X11DisplayInvalid"
            );
            let error = super::super::NativeCaptureReader::default()
                .read(CaptureReadRequest {
                    files: true,
                    rich_text: true,
                })
                .unwrap_err();
            assert_eq!(error.code, "X11DisplayInvalid");
            use dzc_core::ports::ClipboardMonitor;
            let monitor = crate::runtime::NativeClipboardMonitor::default();
            let error = monitor.run(Box::new(|_| {})).unwrap_err();
            assert_eq!(
                error.reason,
                Some(dzc_protocol::UnsupportedReason::ConnectionUnavailable)
            );
            assert!(!monitor.capability().active);
            return;
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "capture::listener::tests::invalid_display_is_rejected_before_native_connections",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("DISPLAY", ":65530")
            .env_remove("DEZIRCLIP_DISABLE_LINUX_SERVICES")
            .env_remove("TIEZ_DISABLE_LINUX_SERVICES")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn native_monitor_probe_negotiates_and_releases_a_message_window() {
        for _ in 0..2 {
            assert_eq!(
                super::probe_clipboard_monitor().unwrap(),
                "win32-message-window"
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn isolated_x11_monitor_probe_negotiates_without_a_worker() {
        if std::env::var("DEZIRCLIP_RUN_LINUX_CLIPBOARD_SMOKE_TESTS").as_deref() != Ok("1") {
            return;
        }
        for _ in 0..2 {
            assert_eq!(super::probe_clipboard_monitor().unwrap(), "x11-xfixes");
        }
    }
}
