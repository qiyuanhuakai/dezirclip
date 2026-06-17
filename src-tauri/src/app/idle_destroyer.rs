use std::sync::atomic::Ordering;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager, WebviewWindowBuilder};

use crate::app_state::SettingsState;
use crate::global_state::{
    IS_DESTROYED, LAST_HIDDEN_TIMESTAMP, RECREATE_PENDING, WINDOW_LIFECYCLE,
};
use crate::infrastructure::webview_environment;

pub const LIFECYCLE_OPEN: u8 = 0;
pub const LIFECYCLE_CLOSING: u8 = 1;
pub const LIFECYCLE_CLOSED: u8 = 2;
pub const LIFECYCLE_OPENING: u8 = 3;

pub const DEFAULT_IDLE_DESTROY_SECONDS: u64 = 60;
pub const MIN_IDLE_DESTROY_SECONDS: u64 = 5;
pub const MAX_IDLE_DESTROY_SECONDS: u64 = 3600;
const LABEL_RELEASE_TIMEOUT: Duration = Duration::from_millis(200);
const BROWSER_PROCESS_EXIT_TIMEOUT: Duration = Duration::from_millis(3000);

/// Pure decision: should the idle destroyer tear down the webview right now?
///
/// Returns `true` only when ALL preconditions hold:
/// - feature enabled
/// - window currently hidden (timestamp != 0)
/// - elapsed since hide exceeds the configured timeout
/// - window is not already destroyed (avoids redundant work)
pub fn should_destroy_now(
    hidden_since_ms: u64,
    now_ms: u64,
    timeout_secs: u64,
    enabled: bool,
    is_destroyed: bool,
) -> bool {
    if !enabled {
        return false;
    }
    if is_destroyed {
        return false;
    }
    if hidden_since_ms == 0 {
        return false;
    }
    let elapsed_ms = now_ms.saturating_sub(hidden_since_ms);
    elapsed_ms >= timeout_secs.saturating_mul(1000)
}

/// Clamp user-supplied seconds into a sane range.
pub fn clamp_idle_seconds(raw: u64) -> u64 {
    raw.clamp(MIN_IDLE_DESTROY_SECONDS, MAX_IDLE_DESTROY_SECONDS)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn should_recreate_main_window(window_exists: bool, is_destroyed: bool, lifecycle: u8) -> bool {
    !window_exists || is_destroyed || lifecycle != LIFECYCLE_OPEN
}

fn main_window_ready(window_exists: bool, is_destroyed: bool, lifecycle: u8) -> bool {
    window_exists && !is_destroyed && lifecycle == LIFECYCLE_OPEN
}

/// Record that the main window is currently hidden.
/// Safe to call multiple times; latest timestamp wins.
pub fn mark_hidden() {
    LAST_HIDDEN_TIMESTAMP.store(now_ms(), Ordering::Relaxed);
}

/// Record that the main window is currently visible.
/// Resets the hidden timestamp so the idle countdown restarts on the next hide.
pub fn mark_shown() {
    LAST_HIDDEN_TIMESTAMP.store(0, Ordering::Relaxed);
}

/// Tauri event hook: call when the main window's visibility changes externally
/// (e.g. CloseRequested, focus loss in non-pinned mode).
pub fn on_visibility_changed(visible: bool) {
    if visible {
        mark_shown();
    } else {
        mark_hidden();
    }
}

/// Tear down the main webview if preconditions hold. No-op when already destroyed
/// or when state transitions are unsafe. Safe to call from any thread.
pub fn try_destroy_idle(app: &AppHandle) -> bool {
    if WINDOW_LIFECYCLE.load(Ordering::SeqCst) == LIFECYCLE_CLOSING {
        complete_pending_destroy(app);
        return false;
    }

    let settings = match app.try_state::<SettingsState>() {
        Some(s) => s,
        None => return false,
    };

    let enabled = settings.idle_destroy_enabled.load(Ordering::Relaxed);
    let timeout_secs = settings.idle_destroy_seconds.load(Ordering::Relaxed);
    let hidden_since = LAST_HIDDEN_TIMESTAMP.load(Ordering::Relaxed);
    let is_destroyed = IS_DESTROYED.load(Ordering::Relaxed);

    if !should_destroy_now(hidden_since, now_ms(), timeout_secs, enabled, is_destroyed) {
        return false;
    }

    // Transition Open → Closing atomically; bail if someone else is mid-transition.
    if WINDOW_LIFECYCLE
        .compare_exchange(LIFECYCLE_OPEN, LIFECYCLE_CLOSING, Ordering::SeqCst, Ordering::Relaxed)
        .is_err()
    {
        return false;
    }

    crate::info!(
        "[idle-destroyer] Destroying main webview after {}s of inactivity",
        now_ms().saturating_sub(hidden_since) / 1000
    );

    if !destroy_main_window(app, false) {
        WINDOW_LIFECYCLE.store(LIFECYCLE_OPEN, Ordering::SeqCst);
        return false;
    }

    finish_main_destroy(app, false);

    if WINDOW_LIFECYCLE.load(Ordering::SeqCst) == LIFECYCLE_CLOSED
        && RECREATE_PENDING.swap(false, Ordering::SeqCst)
    {
        let _ = recreate_main_window(app);
    }
    true
}

fn destroy_main_window(app: &AppHandle, wait_for_browser_exit: bool) -> bool {
    if let Some(win) = app.get_webview_window("main") {
        if wait_for_browser_exit {
            webview_environment::reset_main_browser_process_exit();
            if !webview_environment::watch_main_browser_process_exit(&win) {
                webview_environment::mark_main_browser_process_exited();
                return false;
            }
        } else {
            webview_environment::mark_main_browser_process_exited();
        }
        let _ = win.destroy();
        true
    } else {
        webview_environment::mark_main_browser_process_exited();
        true
    }
}

fn wait_for_label_release(app: &AppHandle, timeout: Duration) -> bool {
    let mut waited = Duration::from_millis(0);
    while app.get_webview_window("main").is_some() {
        if waited >= timeout {
            return false;
        }
        std::thread::sleep(Duration::from_millis(5));
        waited += Duration::from_millis(5);
    }
    true
}

fn finish_main_destroy(app: &AppHandle, wait_for_browser_exit: bool) -> bool {
    let label_released = wait_for_label_release(app, LABEL_RELEASE_TIMEOUT);
    let browser_exited = if wait_for_browser_exit {
        webview_environment::wait_for_main_browser_process_exit(BROWSER_PROCESS_EXIT_TIMEOUT)
    } else {
        true
    };

    if !label_released {
        crate::warn!("[idle-destroyer] Timed out waiting for label to be freed after destroy.");
    }
    if wait_for_browser_exit && !browser_exited {
        crate::warn!("[idle-destroyer] Timed out waiting for WebView2 browser process exit after destroy.");
    }

    LAST_HIDDEN_TIMESTAMP.store(0, Ordering::SeqCst);
    let (lifecycle, is_destroyed, completed) =
        destroy_completion_state(label_released, browser_exited);
    WINDOW_LIFECYCLE.store(lifecycle, Ordering::SeqCst);
    IS_DESTROYED.store(is_destroyed, Ordering::SeqCst);
    completed
}

fn destroy_completion_state(label_released: bool, browser_exited: bool) -> (u8, bool, bool) {
    if label_released && browser_exited {
        (LIFECYCLE_CLOSED, true, true)
    } else {
        (LIFECYCLE_CLOSING, label_released, false)
    }
}

fn complete_pending_destroy(app: &AppHandle) -> bool {
    if app.get_webview_window("main").is_some() {
        return false;
    }
    if !webview_environment::main_browser_process_exited() {
        return false;
    }
    WINDOW_LIFECYCLE.store(LIFECYCLE_CLOSED, Ordering::SeqCst);
    IS_DESTROYED.store(true, Ordering::SeqCst);
    LAST_HIDDEN_TIMESTAMP.store(0, Ordering::SeqCst);
    if RECREATE_PENDING.swap(false, Ordering::SeqCst) {
        return recreate_main_window(app);
    }
    true
}

/// Make sure the main window exists and is ready to be shown.
/// Returns `true` if the window was just recreated (caller may want to defer
/// showing it until the webview has loaded), `false` if it already existed.
///
/// When the window was destroyed by the idle destroyer, this rebuilds it from
/// `tauri.conf.json` so config drift is impossible. Polls until the runtime
/// releases the `main` label (destroy() is message-based, not synchronous).
pub fn recreate_main_window(app: &AppHandle) -> bool {
    // Transition Closed → Opening. If anything else is in progress, bail and
    // let the caller retry on the next event.
    if WINDOW_LIFECYCLE
        .compare_exchange(
            LIFECYCLE_CLOSED,
            LIFECYCLE_OPENING,
            Ordering::SeqCst,
            Ordering::Relaxed,
        )
        .is_err()
    {
        return false;
    }

    if !wait_for_label_release(app, LABEL_RELEASE_TIMEOUT) {
        crate::warn!("[idle-destroyer] Timed out waiting for label to be freed; aborting recreate.");
        WINDOW_LIFECYCLE.store(LIFECYCLE_CLOSED, Ordering::SeqCst);
        return false;
    }

    let config = match app.config().app.windows.iter().find(|c| c.label == "main") {
        Some(c) => c,
        None => {
            crate::warn!("[idle-destroyer] No 'main' window in tauri.conf.json; cannot recreate.");
            WINDOW_LIFECYCLE.store(LIFECYCLE_CLOSED, Ordering::SeqCst);
            return false;
        }
    };

    match WebviewWindowBuilder::from_config(app, config)
        .and_then(|b| b.build())
    {
        Ok(_) => {
            WINDOW_LIFECYCLE.store(LIFECYCLE_OPEN, Ordering::SeqCst);
            IS_DESTROYED.store(false, Ordering::SeqCst);
            // Defensive: clear the hidden timestamp so a racing tick from the
            // background destroyer thread does not immediately re-destroy the
            // freshly-recreated window before the caller reaches mark_shown().
            LAST_HIDDEN_TIMESTAMP.store(0, Ordering::SeqCst);
            crate::info!("[idle-destroyer] Main webview recreated successfully.");
            true
        }
        Err(e) => {
            crate::warn!("[idle-destroyer] Failed to recreate main webview: {}", e);
            WINDOW_LIFECYCLE.store(LIFECYCLE_CLOSED, Ordering::SeqCst);
            false
        }
    }
}

/// Public entry for callers (hotkey handler, tray, frontend) that want to
/// guarantee the main window exists before showing it. Idempotent.
pub fn ensure_main_window(app: &AppHandle) -> bool {
    let window_exists = app.get_webview_window("main").is_some();
    let is_destroyed = IS_DESTROYED.load(Ordering::SeqCst);
    let lifecycle = WINDOW_LIFECYCLE.load(Ordering::SeqCst);

    if lifecycle == LIFECYCLE_CLOSING {
        if complete_pending_destroy(app) {
            return main_window_ready(
                app.get_webview_window("main").is_some(),
                IS_DESTROYED.load(Ordering::SeqCst),
                WINDOW_LIFECYCLE.load(Ordering::SeqCst),
            );
        }
        request_recreate_after_destroy();
        return false;
    }

    if !should_recreate_main_window(window_exists, is_destroyed, lifecycle) {
        return true;
    }
    let _ = recreate_main_window(app);
    main_window_ready(
        app.get_webview_window("main").is_some(),
        IS_DESTROYED.load(Ordering::SeqCst),
        WINDOW_LIFECYCLE.load(Ordering::SeqCst),
    )
}

pub fn restart_main_window_for_gpu_switch(app: &AppHandle, disabled: bool) -> bool {
    crate::app::gpu_switcher::apply_gpu_disable_env(disabled);

    if WINDOW_LIFECYCLE
        .compare_exchange(LIFECYCLE_OPEN, LIFECYCLE_CLOSING, Ordering::SeqCst, Ordering::Relaxed)
        .is_err()
    {
        request_recreate_after_destroy();
        return false;
    }

    let was_visible = app
        .get_webview_window("main")
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false);

    if let Some(preview) = app.get_webview_window("compact-preview") {
        let _ = preview.destroy();
    }

    if !destroy_main_window(app, true) {
        WINDOW_LIFECYCLE.store(LIFECYCLE_OPEN, Ordering::SeqCst);
        return false;
    }
    let teardown_completed = finish_main_destroy(app, true);
    if !teardown_completed {
        if was_visible {
            request_recreate_after_destroy();
        }
        return false;
    }

    if !recreate_main_window(app) {
        return false;
    }

    if was_visible {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.show();
            mark_shown();
        }
    }

    true
}

/// Spawn the background ticker that calls `try_destroy_idle` once per second.
/// Returns immediately; runs until the process exits.
pub fn spawn_idle_destroyer(app: AppHandle) {
    std::thread::spawn(move || {
        let mut interval = tick_interval();
        loop {
            std::thread::sleep(interval);
            try_destroy_idle(&app);
            interval = tick_interval();
        }
    });
}

fn tick_interval() -> Duration {
    Duration::from_millis(1000)
}

/// Mark that a recreate was requested while the destroy was in flight. The
/// destroy path will pick this up and trigger a recreate immediately after
/// the runtime finishes tearing down the window.
pub fn request_recreate_after_destroy() {
    RECREATE_PENDING.store(true, Ordering::SeqCst);
}

pub fn mark_destroyed_after_managed_destroy() {
    WINDOW_LIFECYCLE.store(LIFECYCLE_CLOSED, Ordering::SeqCst);
    IS_DESTROYED.store(true, Ordering::SeqCst);
    LAST_HIDDEN_TIMESTAMP.store(0, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_destroy_now_disabled_when_feature_off() {
        assert!(!should_destroy_now(1000, 61_000, 60, false, false));
    }

    #[test]
    fn should_destroy_now_disabled_when_never_hidden() {
        assert!(!should_destroy_now(0, 61_000, 60, true, false));
    }

    #[test]
    fn should_destroy_now_disabled_when_already_destroyed() {
        assert!(!should_destroy_now(1000, 61_000, 60, true, true));
    }

    #[test]
    fn should_destroy_now_disabled_before_timeout() {
        // 30s elapsed < 60s timeout
        assert!(!should_destroy_now(1_000, 31_000, 60, true, false));
    }

    #[test]
    fn should_destroy_now_enabled_at_exact_timeout() {
        // exactly 60s elapsed
        assert!(should_destroy_now(1_000, 61_000, 60, true, false));
    }

    #[test]
    fn should_destroy_now_enabled_past_timeout() {
        assert!(should_destroy_now(1_000, 120_000, 60, true, false));
    }

    #[test]
    fn clamp_idle_seconds_clamps_below_minimum() {
        assert_eq!(clamp_idle_seconds(0), MIN_IDLE_DESTROY_SECONDS);
        assert_eq!(clamp_idle_seconds(4), MIN_IDLE_DESTROY_SECONDS);
    }

    #[test]
    fn clamp_idle_seconds_clamps_above_maximum() {
        assert_eq!(clamp_idle_seconds(9999), MAX_IDLE_DESTROY_SECONDS);
    }

    #[test]
    fn clamp_idle_seconds_passes_through_valid_values() {
        assert_eq!(clamp_idle_seconds(60), 60);
        assert_eq!(clamp_idle_seconds(300), 300);
    }

    #[test]
    fn lifecycle_states_are_distinct() {
        // These constants are used as u8 discriminators in atomic CAS; collisions
        // would silently corrupt the state machine.
        let states = [
            LIFECYCLE_OPEN,
            LIFECYCLE_CLOSING,
            LIFECYCLE_CLOSED,
            LIFECYCLE_OPENING,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j], "lifecycle constants must be distinct");
            }
        }
    }

    #[test]
    fn should_destroy_now_handles_overflow_safely() {
        // now_ms earlier than hidden_since (clock skew, monotonic regression)
        // must NOT panic and must return false (no destroy).
        assert!(!should_destroy_now(u64::MAX / 2, 0, 60, true, false));
        // Huge timeout that would overflow u64 when multiplied by 1000
        let huge = u64::MAX / 1000;
        assert!(!should_destroy_now(1, 1 + huge, huge, true, false));
    }

    #[test]
    fn should_destroy_now_boundary_at_zero_timeout() {
        // timeout = 0 with elapsed > 0 should destroy (instant after hide)
        // Currently MIN is 5 but the pure function should still behave
        // consistently if called with smaller values.
        assert!(should_destroy_now(1_000, 1_001, 0, true, false));
        assert!(!should_destroy_now(0, 0, 0, true, false));
    }

    #[test]
    fn should_recreate_when_destroyed_state_keeps_stale_label() {
        assert!(should_recreate_main_window(
            true,
            true,
            LIFECYCLE_CLOSED
        ));
    }

    #[test]
    fn main_window_not_ready_when_destroyed_state_keeps_stale_label() {
        assert!(!main_window_ready(true, true, LIFECYCLE_CLOSED));
    }

    #[test]
    fn should_not_recreate_when_open_state_has_window() {
        assert!(!should_recreate_main_window(
            true,
            false,
            LIFECYCLE_OPEN
        ));
        assert!(main_window_ready(true, false, LIFECYCLE_OPEN));
    }

    #[test]
    fn destroy_completion_waits_for_browser_process_exit() {
        assert_eq!(
            destroy_completion_state(true, false),
            (LIFECYCLE_CLOSING, true, false)
        );
    }

    #[test]
    fn destroy_completion_closes_only_after_label_and_browser_exit() {
        assert_eq!(
            destroy_completion_state(true, true),
            (LIFECYCLE_CLOSED, true, true)
        );
    }
}
