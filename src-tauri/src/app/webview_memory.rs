#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebviewMemoryTarget {
    Normal,
    Low,
}

pub fn memory_target_for_visibility(visible: bool) -> WebviewMemoryTarget {
    if visible {
        WebviewMemoryTarget::Normal
    } else {
        WebviewMemoryTarget::Low
    }
}

pub fn lower_window_memory(window: &tauri::WebviewWindow, reason: &'static str) {
    apply_memory_target(window, memory_target_for_visibility(false), reason);
}

pub fn restore_window_memory(window: &tauri::WebviewWindow, reason: &'static str) {
    apply_memory_target(window, memory_target_for_visibility(true), reason);
}

pub fn force_transparent_background(window: &tauri::WebviewWindow) {
    #[cfg(target_os = "windows")]
    {
        apply_transparent_background(window);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = window;
    }
}

#[cfg(target_os = "windows")]
fn apply_transparent_background(window: &tauri::WebviewWindow) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_COLOR, ICoreWebView2Controller2,
    };
    use windows::core::Interface;

    let label = window.label().to_string();
    let label_for_err = label.clone();
    if let Err(err) = window.with_webview(move |webview| {
        let result = unsafe {
            webview
                .controller()
                .cast::<ICoreWebView2Controller2>()
                .and_then(|controller| {
                    controller.SetDefaultBackgroundColor(COREWEBVIEW2_COLOR {
                        R: 0,
                        G: 0,
                        B: 0,
                        A: 0,
                    })
                })
        };

        match result {
            Ok(()) => crate::info!(
                "[webview-memory] Forced {label} WebView2 background to (0,0,0,0) for OS material pass-through"
            ),
            Err(err) => crate::warn!(
                "[webview-memory] Failed to force {label} WebView2 transparent background: {err:?}"
            ),
        }
    }) {
        crate::warn!(
            "[webview-memory] Failed to schedule {label_for_err} WebView2 transparent background: {err}"
        );
    }
}

#[cfg(target_os = "windows")]
fn apply_memory_target(
    window: &tauri::WebviewWindow,
    target: WebviewMemoryTarget,
    reason: &'static str,
) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
    };
    use windows::core::Interface;

    let label = window.label().to_string();
    let label_for_err = label.clone();
    if let Err(err) = window.with_webview(move |webview| {
        let result = unsafe {
            webview
                .controller()
                .CoreWebView2()
                .and_then(|core| core.cast::<ICoreWebView2_19>())
                .and_then(|core| {
                    core.SetMemoryUsageTargetLevel(match target {
                        WebviewMemoryTarget::Normal => COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
                        WebviewMemoryTarget::Low => COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
                    })
                })
        };

        match result {
            Ok(()) => crate::info!(
                "[webview-memory] Set {label} memory target to {target:?} ({reason})"
            ),
            Err(err) => crate::warn!(
                "[webview-memory] Failed to set {label} memory target to {target:?} ({reason}): {err:?}"
            ),
        }
    }) {
        crate::warn!(
            "[webview-memory] Failed to schedule {label_for_err} memory target {target:?} ({reason}): {err}"
        );
    }
}

#[cfg(not(target_os = "windows"))]
fn apply_memory_target(
    _window: &tauri::WebviewWindow,
    _target: WebviewMemoryTarget,
    _reason: &'static str,
) {
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_windows_request_low_memory() {
        assert_eq!(
            memory_target_for_visibility(false),
            WebviewMemoryTarget::Low
        );
    }

    #[test]
    fn visible_windows_request_normal_memory() {
        assert_eq!(
            memory_target_for_visibility(true),
            WebviewMemoryTarget::Normal
        );
    }
}
