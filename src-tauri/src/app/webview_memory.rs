use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;

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

/// `WebviewMemoryTarget` as a number, so "what is applied" fits in one atomic.
/// `0` is not a target: it is "this controller has never been told anything".
const TARGET_UNKNOWN: u8 = 0;
const TARGET_NORMAL: u8 = 1;
const TARGET_LOW: u8 = 2;

/// The target last *successfully* handed to the main WebView2 controller.
///
/// Set only on success, so a COM call that failed is retried on the next hide or
/// show rather than being remembered as done.
static MAIN_MEMORY_TARGET: AtomicU8 = AtomicU8::new(TARGET_UNKNOWN);

/// Whether the main controller's background has been forced transparent since
/// it was last created.
static MAIN_TRANSPARENT: AtomicBool = AtomicBool::new(false);

fn memory_target_code(target: WebviewMemoryTarget) -> u8 {
    match target {
        WebviewMemoryTarget::Normal => TARGET_NORMAL,
        WebviewMemoryTarget::Low => TARGET_LOW,
    }
}

/// Pure decision: does this call have to cross to the webview thread at all?
///
/// Both properties it holds are sticky on an `ICoreWebView2Controller`: they
/// stay in force until the app changes them or the controller is replaced. So
/// asking for the value the controller already has is a round trip that cannot
/// change anything — and it is not cheap, because `with_webview` blocks the
/// calling thread until the webview's own UI thread runs the closure. `set_theme`
/// is a synchronous command, so that wait lands on the main thread.
pub fn needs_memory_target_apply(applied: u8, target: WebviewMemoryTarget) -> bool {
    applied != memory_target_code(target)
}

/// Pure decision: same question for the transparent background.
pub fn needs_transparent_apply(applied: bool) -> bool {
    !applied
}

/// Forget what was applied to a main controller that is about to be replaced.
///
/// The idle destroyer and the GPU switch build a new `ICoreWebView2Controller`
/// from `tauri.conf.json`, and a fresh controller starts at its defaults — a new
/// one would skip every apply here and stay at NORMAL memory with an opaque
/// background, which is the exact opposite of what this module is for. This is
/// the only main-window creation point in the app, so it is also the only place
/// the cache has to be dropped.
pub fn forget_main_webview_state() {
    MAIN_MEMORY_TARGET.store(TARGET_UNKNOWN, Ordering::Release);
    MAIN_TRANSPARENT.store(false, Ordering::Release);
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

/// Set the memory usage target, unless the controller already has it.
///
/// Only the main window is tracked. The auxiliary windows (region selector,
/// quick-paste overlay) are built on demand and torn down with their own
/// lifecycle, so remembering anything about their controllers across a call
/// would outlive the controller it describes.
pub fn apply_memory_target(
    window: &tauri::WebviewWindow,
    target: WebviewMemoryTarget,
    reason: &'static str,
) {
    let tracked = window.label() == MAIN_LABEL;
    if tracked && !needs_memory_target_apply(MAIN_MEMORY_TARGET.load(Ordering::Acquire), target) {
        return;
    }
    if apply_memory_target_to_webview(window, target, reason) && tracked {
        MAIN_MEMORY_TARGET.store(memory_target_code(target), Ordering::Release);
    }
}

#[cfg(target_os = "windows")]
fn apply_memory_target_to_webview(
    window: &tauri::WebviewWindow,
    target: WebviewMemoryTarget,
    reason: &'static str,
) -> bool {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
    };
    use windows::core::Interface;

    let label = window.label().to_string();
    let label_for_err = label.clone();
    // `with_webview` runs the closure on the webview's own thread and discards
    // whatever it returns, so the verdict has to travel back through shared
    // state rather than a return value.
    let applied = Arc::new(AtomicBool::new(false));
    let applied_in_closure = Arc::clone(&applied);
    match window.with_webview(move |webview| {
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
            Ok(()) => {
                applied_in_closure.store(true, Ordering::Release);
                crate::info!(
                    "[webview-memory] Set {label} memory target to {target:?} ({reason})"
                );
            }
            Err(err) => crate::warn!(
                "[webview-memory] Failed to set {label} memory target to {target:?} ({reason}): {err:?}"
            ),
        }
    }) {
        Ok(()) => applied.load(Ordering::Acquire),
        Err(err) => {
            crate::warn!(
                "[webview-memory] Failed to schedule {label_for_err} memory target {target:?} ({reason}): {err}"
            );
            false
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn apply_memory_target_to_webview(
    _window: &tauri::WebviewWindow,
    _target: WebviewMemoryTarget,
    _reason: &'static str,
) -> bool {
    false
}

/// Force the WebView2 background transparent, unless it already is.
pub fn apply_transparent_background(window: &tauri::WebviewWindow) {
    let tracked = window.label() == MAIN_LABEL;
    if tracked && !needs_transparent_apply(MAIN_TRANSPARENT.load(Ordering::Acquire)) {
        return;
    }
    if force_transparent_on_webview(window) && tracked {
        MAIN_TRANSPARENT.store(true, Ordering::Release);
    }
}

#[cfg(target_os = "windows")]
fn force_transparent_on_webview(window: &tauri::WebviewWindow) -> bool {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2Controller2, COREWEBVIEW2_COLOR,
    };
    use windows::core::Interface;

    let _ = window
        .as_ref()
        .set_background_color(Some(tauri::webview::Color(0, 0, 0, 0)));

    let label = window.label().to_string();
    let label_for_err = label.clone();
    // See `apply_memory_target_to_webview`: the closure's value is discarded.
    let applied = Arc::new(AtomicBool::new(false));
    let applied_in_closure = Arc::clone(&applied);
    match window.with_webview(move |webview| {
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
            Ok(()) => {
                applied_in_closure.store(true, Ordering::Release);
                crate::info!(
                    "[webview-memory] Forced {label} WebView2 background to (0,0,0,0) for OS material pass-through"
                );
            }
            Err(err) => crate::warn!(
                "[webview-memory] Failed to force {label} WebView2 transparent background: {err:?}"
            ),
        }
    }) {
        Ok(()) => applied.load(Ordering::Acquire),
        Err(err) => {
            crate::warn!(
                "[webview-memory] Failed to schedule {label_for_err} WebView2 transparent background: {err}"
            );
            false
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn force_transparent_on_webview(_window: &tauri::WebviewWindow) -> bool {
    false
}

const MAIN_LABEL: &str = "main";

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

    #[test]
    fn asking_for_the_target_the_controller_already_has_skips_the_crossing() {
        // This is the whole optimization: the property is sticky on the
        // controller, so a repeat cannot change anything and only costs a
        // blocking round trip to the webview thread.
        assert!(!needs_memory_target_apply(
            TARGET_LOW,
            WebviewMemoryTarget::Low
        ));
        assert!(!needs_memory_target_apply(
            TARGET_NORMAL,
            WebviewMemoryTarget::Normal
        ));
        assert!(needs_memory_target_apply(
            TARGET_LOW,
            WebviewMemoryTarget::Normal
        ));
        assert!(needs_memory_target_apply(
            TARGET_NORMAL,
            WebviewMemoryTarget::Low
        ));
    }

    #[test]
    fn a_controller_nobody_has_told_is_always_told() {
        assert!(needs_memory_target_apply(
            TARGET_UNKNOWN,
            WebviewMemoryTarget::Normal
        ));
        assert!(needs_memory_target_apply(
            TARGET_UNKNOWN,
            WebviewMemoryTarget::Low
        ));
        assert!(needs_transparent_apply(false));
    }

    #[test]
    fn a_transparent_background_is_only_forced_once_per_controller() {
        assert!(needs_transparent_apply(false));
        assert!(!needs_transparent_apply(true));
    }

    #[test]
    fn a_rebuilt_controller_has_to_be_told_again() {
        // The dangerous regression: caching across a rebuild would leave a brand
        // new controller at NORMAL memory with an opaque background, undoing
        // what this module exists for. The idle destroyer rebuilds the main
        // window several times an hour, so this is not hypothetical.
        MAIN_MEMORY_TARGET.store(TARGET_LOW, Ordering::Release);
        MAIN_TRANSPARENT.store(true, Ordering::Release);

        forget_main_webview_state();

        assert!(needs_memory_target_apply(
            MAIN_MEMORY_TARGET.load(Ordering::Acquire),
            WebviewMemoryTarget::Normal
        ));
        assert!(needs_transparent_apply(MAIN_TRANSPARENT.load(
            Ordering::Acquire
        )));

        // And the forget leaves nothing behind for the next test to trip over.
        forget_main_webview_state();
    }

    #[test]
    fn the_two_targets_are_distinct_codes() {
        // A collision would make a Low controller look Normal and silently skip
        // the restore on every show.
        assert_ne!(
            memory_target_code(WebviewMemoryTarget::Low),
            memory_target_code(WebviewMemoryTarget::Normal)
        );
        assert_ne!(memory_target_code(WebviewMemoryTarget::Low), TARGET_UNKNOWN);
        assert_ne!(memory_target_code(WebviewMemoryTarget::Normal), TARGET_UNKNOWN);
    }

    #[test]
    fn the_cache_is_dropped_where_the_controller_is_rebuilt() {
        // The tests above call `forget_main_webview_state` themselves, which
        // proves the reset works and proves nothing about whether the rebuild
        // path calls it. That gap is the whole regression: drop the call there
        // and every rebuilt window keeps the dead controller's cache, stays at
        // NORMAL memory and never goes transparent again, with all five tests
        // still green.
        //
        // The body runs to the first `}` in column zero, which is where
        // rustfmt puts the end of a top-level item.
        let source = include_str!("idle_destroyer.rs");
        let body = source
            .split_once("pub fn recreate_main_window")
            .expect("recreate_main_window is what builds the replacement controller")
            .1
            .split_once("\n}\n")
            .expect("a top-level function body ends with `}` in column zero")
            .0;

        assert!(
            body.contains("forget_main_webview_state()"),
            "recreate_main_window builds a fresh ICoreWebView2Controller but never drops the \
             memory-target cache, so the replacement would skip every apply and sit at NORMAL \
             memory with an opaque background"
        );
        assert!(
            body.find("forget_main_webview_state()") < body.find("recreated successfully"),
            "the cache has to be dropped on the success branch, before the window is announced \
             as ready — a later caller may show it the moment that line lands"
        );
    }
}
