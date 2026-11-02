use std::sync::atomic::{AtomicUsize, Ordering};
use windows::Win32::Foundation::{GetLastError, SetLastError, HWND, RECT, WIN32_ERROR};
use windows::Win32::System::Threading::AttachThreadInput;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_LWIN,
    VK_RWIN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId, IsIconic,
    IsWindowVisible, MessageBoxW, SetForegroundWindow, SetWindowPos, ShowWindow, HWND_TOPMOST,
    GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE,
    MB_ICONERROR, MB_OK, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SW_RESTORE,
};

/// 安全封装的窗口辅助工具
pub struct WindowExt;

// The keyboard hook must not synchronously ask Tauri's UI thread for an HWND.
static MAIN_NAVIGATION_WINDOW: AtomicUsize = AtomicUsize::new(0);

fn clear_navigation_snapshot(snapshot: &AtomicUsize, expected: Option<usize>) {
    if let Some(expected) = expected {
        let _ = snapshot.compare_exchange(expected, 0, Ordering::AcqRel, Ordering::Acquire);
    } else {
        snapshot.store(0, Ordering::Release);
    }
}

fn native_navigation_visible(handle: usize) -> bool {
    if handle == 0 { return false; }
    let hwnd = HWND(handle as *mut _);
    unsafe {
        let mut process_id = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut process_id)) != 0
            && process_id == std::process::id()
            && IsWindowVisible(hwnd).as_bool()
            && !IsIconic(hwnd).as_bool()
    }
}

impl WindowExt {
    /// Publish only from the UI thread after resolving the current main window.
    pub fn publish_main_navigation_window(hwnd: HWND) -> bool {
        let handle = hwnd.0 as usize;
        if !native_navigation_visible(handle) { return false; }
        MAIN_NAVIGATION_WINDOW.store(handle, Ordering::Release);
        true
    }

    pub fn clear_main_navigation_window(expected: Option<HWND>) {
        clear_navigation_snapshot(&MAIN_NAVIGATION_WINDOW, expected.map(|hwnd| hwnd.0 as usize));
    }

    pub fn main_navigation_window_is_visible() -> bool {
        let handle = MAIN_NAVIGATION_WINDOW.load(Ordering::Acquire);
        native_navigation_visible(handle)
            && MAIN_NAVIGATION_WINDOW.load(Ordering::Acquire) == handle
    }

    pub fn allow_activation(hwnd: HWND) -> Result<(), String> {
        unsafe {
            SetLastError(WIN32_ERROR(0));
            let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let error = GetLastError();
            if style == 0 && error.0 != 0 { return Err(format!("MainWindowActivationFailed: {}", error.0)); }
            SetLastError(WIN32_ERROR(0));
            let previous = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style & !(WS_EX_NOACTIVATE.0 as isize));
            let error = GetLastError();
            if previous == 0 && error.0 != 0 { return Err(format!("MainWindowActivationFailed: {}", error.0)); }
        }
        Ok(())
    }

    /// 获取当前前台窗口句柄
    pub fn get_foreground_window() -> HWND {
        unsafe { GetForegroundWindow() }
    }

    /// 检查窗口是否可见
    pub fn is_window_visible(hwnd: HWND) -> bool {
        unsafe { IsWindowVisible(hwnd).as_bool() }
    }

    /// 获取窗口矩形区域
    pub fn get_window_rect(hwnd: HWND) -> Option<RECT> {
        let mut rect = RECT::default();
        unsafe {
            if GetWindowRect(hwnd, &mut rect).is_ok() {
                Some(rect)
            } else {
                None
            }
        }
    }

    /// 释放 Windows 键（防止开始菜单弹出）
    pub fn release_win_keys() {
        unsafe {
            let dummy_vk = VIRTUAL_KEY(0xFF);
            let inputs = [
                Self::create_key_input(dummy_vk, false),
                Self::create_key_input(dummy_vk, true),
                Self::create_key_input(VK_LWIN, true),
                Self::create_key_input(VK_RWIN, true),
            ];
            SendInput(&inputs, core::mem::size_of::<INPUT>() as i32);
        }
    }

    /// 强力恢复窗口焦点（处理跨线程输入附加）
    pub fn force_focus_window(hwnd: HWND) {
        if hwnd.0.is_null() {
            return;
        }

        unsafe {
            if !IsWindowVisible(hwnd).as_bool() {
                return;
            }
            let should_restore = IsIconic(hwnd).as_bool();

            let fg_hwnd = GetForegroundWindow();
            if fg_hwnd != hwnd {
                let fg_thread_id = GetWindowThreadProcessId(fg_hwnd, None);
                let target_thread_id = GetWindowThreadProcessId(hwnd, None);

                if fg_thread_id != 0 && target_thread_id != 0 && fg_thread_id != target_thread_id {
                    let _ = AttachThreadInput(fg_thread_id, target_thread_id, true);
                    let _ = SetForegroundWindow(hwnd);
                    if should_restore {
                        let _ = ShowWindow(hwnd, SW_RESTORE);
                    }
                    let _ = BringWindowToTop(hwnd);
                    let _ = AttachThreadInput(fg_thread_id, target_thread_id, false);
                } else {
                    let _ = SetForegroundWindow(hwnd);
                    if should_restore {
                        let _ = ShowWindow(hwnd, SW_RESTORE);
                    }
                    let _ = BringWindowToTop(hwnd);
                }
            }
        }
    }

    /// 无感置顶一个**已经显示**的窗口（不夺取焦点）
    ///
    /// 只调整 Z 序，不负责显示。显示交给 Tauri 的 `show()`，否则窗口的
    /// 显隐会绕过 tao 的状态跟踪：实测那样之后每次热键都判定窗口仍可见，
    /// 于是只隐藏、再也不显示。
    pub fn raise_topmost_no_activate(hwnd: HWND) {
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW | SWP_NOACTIVATE,
            );
        }
    }

    /// 无激活把一个**已经显示**的普通窗口提到前面（不保持置顶）
    pub fn raise_front_no_activate(hwnd: HWND) {
        unsafe {
            // Bring to front without activation by temporarily toggling TOPMOST.
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW | SWP_NOACTIVATE,
            );
            let _ = SetWindowPos(
                hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW | SWP_NOACTIVATE,
            );
        }
    }

    /// 弹出错误消息框
    pub fn show_error_box(title: &str, msg: &str) {
        use windows::core::PCWSTR;
        let title_w: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        let msg_w: Vec<u16> = msg.encode_utf16().chain(std::iter::once(0)).collect();

        unsafe {
            let _ = MessageBoxW(
                None,
                PCWSTR(msg_w.as_ptr()),
                PCWSTR(title_w.as_ptr()),
                MB_ICONERROR | MB_OK,
            );
        }
    }

    fn create_key_input(vk: VIRTUAL_KEY, is_up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    dwFlags: if is_up {
                        KEYEVENTF_KEYUP
                    } else {
                        windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS(0)
                    },
                    ..Default::default()
                },
            },
        }
    }
}

#[cfg(test)]
mod navigation_snapshot_tests {
    use super::*;

    #[test]
    fn late_clear_does_not_revoke_replacement_window() {
        let snapshot = AtomicUsize::new(200);
        clear_navigation_snapshot(&snapshot, Some(100));
        assert_eq!(snapshot.load(Ordering::Acquire), 200);
        clear_navigation_snapshot(&snapshot, Some(200));
        assert_eq!(snapshot.load(Ordering::Acquire), 0);
    }

    #[test]
    fn disable_revokes_current_window_snapshot() {
        let snapshot = AtomicUsize::new(200);
        clear_navigation_snapshot(&snapshot, None);
        assert_eq!(snapshot.load(Ordering::Acquire), 0);
    }
}
