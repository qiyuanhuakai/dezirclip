use tauri::WebviewWindow;

pub fn caller_allowed(label: &str) -> bool { label == "main" }

#[cfg(any(target_os = "windows", test))]
fn blocks_event(target: usize, code: i32, candidate: usize, child: bool) -> bool {
    target != 0 && candidate != 0
        && ((code == 5 && candidate == target) || (code == 9 && (candidate == target || child)))
}

#[cfg(any(target_os = "windows", test))]
fn finish_presentation(shown: Result<(), String>, unhooked: Result<(), String>, hide: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    match (shown, unhooked) {
        (Ok(()), Ok(())) => Ok(()),
        (shown, unhooked) => {
            let mut errors = Vec::new();
            if let Err(error) = shown { errors.push(error); }
            if let Err(error) = unhooked { errors.push(error); }
            if let Err(error) = hide() { errors.push(format!("PreviewHideFailed: {error}")); }
            Err(errors.join("; "))
        }
    }
}

pub fn show(window: &WebviewWindow) -> Result<(), String> {
    if window.label() != "compact-preview" { return Err("CompactPreviewRequired".into()); }
    #[cfg(target_os = "windows")]
    { windows::show(window) }
    #[cfg(not(target_os = "windows"))]
    { window.show().map_err(|error| error.to_string()) }
}

#[cfg(target_os = "windows")]
mod windows {
    use super::{blocks_event, finish_presentation};
    use std::cell::Cell;
    use tauri::WebviewWindow;
    use ::windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use ::windows::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
    use ::windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GetForegroundWindow, GetWindowThreadProcessId, IsChild, IsWindowVisible, SetWindowsHookExW,
        UnhookWindowsHookEx, HHOOK, WH_CBT,
    };

    thread_local! {
        static TARGET: Cell<usize> = const { Cell::new(0) };
        static PENDING_CLEANUP: Cell<usize> = const { Cell::new(0) };
    }

    unsafe extern "system" fn callback(code: i32, value: WPARAM, extra: LPARAM) -> LRESULT {
        let target = TARGET.try_with(Cell::get).unwrap_or(0);
        let child = code == 9 && target != 0 && value.0 != 0
            && unsafe { IsChild(HWND(target as *mut _), HWND(value.0 as *mut _)).as_bool() };
        if blocks_event(target, code, value.0, child) { LRESULT(1) }
        else { unsafe { CallNextHookEx(None, code, value, extra) } }
    }

    struct Guard { hook: HHOOK, active: bool }

    impl Guard {
        fn install(window: HWND) -> Result<Self, String> {
            if TARGET.with(Cell::get) != 0 { return Err("PreviewActivationReentry".into()); }
            let pending = PENDING_CLEANUP.with(Cell::get);
            if pending != 0 {
                unsafe { UnhookWindowsHookEx(HHOOK(pending as *mut _)) }
                    .map_err(|error| format!("PreviewActivationCleanupFailed: {error}"))?;
                PENDING_CLEANUP.with(|value| value.set(0));
            }
            let mut process = 0;
            let thread = unsafe { GetWindowThreadProcessId(window, Some(&mut process)) };
            if window.0.is_null() || thread == 0 || thread != unsafe { GetCurrentThreadId() }
                || process != unsafe { GetCurrentProcessId() } {
                return Err("PreviewActivationThreadMismatch".into());
            }
            let hook = unsafe { SetWindowsHookExW(WH_CBT, Some(callback), None, thread) }
                .map_err(|error| format!("PreviewActivationInstallFailed: {error}"))?;
            TARGET.with(|value| value.set(window.0 as usize));
            Ok(Self { hook, active: true })
        }

        fn finish(&mut self) -> Result<(), String> {
            if !self.active { return Ok(()); }
            self.active = false;
            TARGET.with(|value| value.set(0));
            if let Err(error) = unsafe { UnhookWindowsHookEx(self.hook) } {
                PENDING_CLEANUP.with(|value| value.set(self.hook.0 as usize));
                return Err(format!("PreviewActivationUnhookFailed: {error}"));
            }
            Ok(())
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) { let _ = self.finish(); }
    }

    pub(super) fn show(window: &WebviewWindow) -> Result<(), String> {
        let handle = window.hwnd().map_err(|error| error.to_string())?;
        let mut guard = Guard::install(handle)?;
        let foreground = unsafe { GetForegroundWindow() };
        let shown = window.show().map_err(|error| format!("PreviewShowFailed: {error}"))
            .and_then(|()| {
                if !unsafe { IsWindowVisible(handle).as_bool() }
                    || !window.is_visible().map_err(|error| error.to_string())? {
                    return Err("PreviewNotVisibleAfterShow".into());
                }
                if unsafe { GetForegroundWindow() } != foreground {
                    return Err("PreviewShowChangedForeground".into());
                }
                Ok(())
            });
        let unhooked = guard.finish();
        finish_presentation(shown, unhooked, || window.hide().map_err(|error| error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::{blocks_event, caller_allowed, finish_presentation};

    #[test]
    fn only_main_can_request_the_fixed_preview_show() {
        assert!(caller_allowed("main"));
        for label in ["compact-preview", "quick-paste", "region-select", "pinned-preview-test", "", "Main"] {
            assert!(!caller_allowed(label));
        }
    }

    #[test]
    fn activation_is_blocked_only_for_the_exact_scoped_window() {
        assert!(blocks_event(12, 5, 12, false));
        assert!(!blocks_event(12, 5, 13, true));
        assert!(!blocks_event(12, 5, 0, false));
    }

    #[test]
    fn focus_is_blocked_only_for_the_scoped_window_or_its_child() {
        assert!(blocks_event(12, 9, 12, false));
        assert!(blocks_event(12, 9, 13, true));
        assert!(!blocks_event(12, 9, 13, false));
    }

    #[test]
    fn neutral_or_other_hook_notifications_never_block() {
        assert!(!blocks_event(0, 5, 12, false));
        assert!(!blocks_event(0, 9, 12, true));
        for code in [-1, 0, 1, 2, 3, 4, 6, 7, 8] {
            assert!(!blocks_event(12, code, 12, true));
        }
    }

    #[test]
    fn successful_show_and_unhook_do_not_hide_the_preview() {
        assert_eq!(finish_presentation(Ok(()), Ok(()), || panic!("successful show must stay visible")), Ok(()));
    }

    #[test]
    fn unhook_failure_hides_and_is_not_reported_as_a_successful_show() {
        let hidden = std::cell::Cell::new(false);
        let result = finish_presentation(Ok(()), Err("unhook".into()), || { hidden.set(true); Ok(()) });
        assert!(hidden.get());
        assert_eq!(result, Err("unhook".into()));
    }

    #[test]
    fn cleanup_failure_preserves_both_initial_errors_and_hide_error() {
        assert_eq!(finish_presentation(Err("show".into()), Err("unhook".into()), || Err("hide".into())),
                   Err("show; unhook; PreviewHideFailed: hide".into()));
    }
}
