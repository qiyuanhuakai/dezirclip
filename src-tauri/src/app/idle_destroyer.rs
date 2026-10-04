use std::sync::atomic::Ordering;
use std::sync::OnceLock;
use std::thread::ThreadId;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, WebviewWindowBuilder};

use crate::app_state::SettingsState;
use crate::global_state::{
    CLOSING_SINCE_MS, FRONTEND_CATCHUP_PENDING, IS_DESTROYED, LAST_HIDDEN_TIMESTAMP,
    RECREATE_PENDING, WINDOW_LIFECYCLE,
};
use crate::infrastructure::webview_environment;

pub const LIFECYCLE_OPEN: u8 = 0;
pub const LIFECYCLE_CLOSING: u8 = 1;
pub const LIFECYCLE_CLOSED: u8 = 2;
pub const LIFECYCLE_OPENING: u8 = 3;

pub const DEFAULT_IDLE_DESTROY_SECONDS: u64 = 60;
pub const MIN_IDLE_DESTROY_SECONDS: u64 = 5;
pub const MAX_IDLE_DESTROY_SECONDS: u64 = 3600;

/// Hard budget for one teardown, from the `Open → Closing` transition to a
/// settled `Closed` / `Open` state.
///
/// The runtime releases the `main` label and reaps the WebView2 browser process
/// on its own schedule, and both signals can be lost: the label wait competes
/// with a busy main thread, and the browser process can outlive its own exit
/// timeout under memory pressure. Without a budget those cases leave the
/// lifecycle parked in `Closing` indefinitely, and every `ensure_main_window`
/// caller then bails out, so the clipboard window can never be shown again.
pub const CLOSING_WATCHDOG_MS: u64 = 5_000;

const LABEL_RELEASE_TIMEOUT: Duration = Duration::from_millis(800);
const BROWSER_PROCESS_EXIT_TIMEOUT: Duration = Duration::from_millis(3000);

/// Thread Tauri dispatches window teardown and creation on, captured during
/// setup. Tauri does not expose it, and guessing wrong would reintroduce a
/// main-thread wait that can never make progress.
static MAIN_THREAD_ID: OnceLock<ThreadId> = OnceLock::new();

/// What the ticker should do with the hidden timestamp for the window's current
/// visibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HiddenStateAction {
    /// The window is on screen: clear the timestamp so nothing is torn down.
    Clear,
    /// The window is hidden but no hide path recorded it: start the countdown.
    Start,
    /// The window is hidden and already counting: leave the timestamp alone.
    Keep,
}

/// Pure decision reconciling the idle countdown with the window's real visibility.
///
/// Several hide paths (paste, quick-paste, edge docking) never call
/// `mark_hidden`, and the matching show paths never call `mark_shown`. Deriving
/// the countdown from the actual visibility makes the destroyer self-correcting
/// instead of depending on that scattered bookkeeping.
pub fn hidden_state_action(visible: bool, hidden_since_ms: u64) -> HiddenStateAction {
    if visible {
        HiddenStateAction::Clear
    } else if hidden_since_ms == 0 {
        HiddenStateAction::Start
    } else {
        HiddenStateAction::Keep
    }
}

/// Pure decision: has the in-flight teardown outlived its budget?
///
/// `closing_since_ms == 0` means the state machine is not closing, so there is
/// nothing to force.
pub fn closing_watchdog_expired(closing_since_ms: u64, now_ms: u64, watchdog_ms: u64) -> bool {
    if closing_since_ms == 0 {
        return false;
    }
    now_ms.saturating_sub(closing_since_ms) >= watchdog_ms
}

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

/// Pure decision: is the main window on screen right now?
///
/// `LAST_HIDDEN_TIMESTAMP` is already the single source of truth for this: every
/// show path calls `mark_shown`, every hide path calls `mark_hidden`, and the
/// ticker reconciles the two against the real visibility. Reusing it keeps the
/// capture pipeline from needing a second, separately maintained flag that could
/// disagree with the destroyer's.
pub fn main_window_on_screen() -> bool {
    LAST_HIDDEN_TIMESTAMP.load(Ordering::Relaxed) == 0
}

/// Pure decision: should this capture be delivered to the frontend right now?
///
/// Hidden means "nobody is looking", and the renderer is at its low memory
/// target: waking it to re-sort and re-render a list no one can see is the exact
/// cost the hide just paid to avoid. A destroyed window has no frontend to wake
/// at all, and its replacement fetches the whole history when it mounts.
pub fn should_deliver_capture(on_screen: bool, webview_alive: bool) -> bool {
    on_screen && webview_alive
}

/// Note that the frontend missed at least one capture, so the next show has to
/// hand it a refresh.
pub fn mark_frontend_catchup_pending() {
    FRONTEND_CATCHUP_PENDING.store(true, Ordering::SeqCst);
}

/// Announce that the main window is on screen again, settling the idle countdown
/// and paying off any refresh the hidden window was owed.
///
/// Every show path routes through here rather than `mark_shown` so a new one
/// cannot forget the catch-up: without it the list would silently keep showing
/// whatever was there when the window went down.
pub fn notify_main_window_shown(app: &AppHandle) {
    mark_shown();
    if !FRONTEND_CATCHUP_PENDING.swap(false, Ordering::SeqCst) {
        return;
    }
    crate::info!(
        "[idle-destroyer] Main window shown; flushing the captures it missed while hidden."
    );
    let _ = app.emit_to("main", "clipboard-changed", ());
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

/// Start the watchdog for an in-flight teardown.
fn mark_closing() {
    CLOSING_SINCE_MS.store(now_ms(), Ordering::SeqCst);
}

/// Stop the watchdog; the lifecycle is no longer `Closing`.
fn clear_closing() {
    CLOSING_SINCE_MS.store(0, Ordering::SeqCst);
}

/// Reconcile the idle countdown with the window's real visibility.
///
/// Guarantees two invariants that the manual `mark_hidden` / `mark_shown` calls
/// cannot: a window that is on screen is never torn down, and a window that is
/// hidden always ages out even if its hide path forgot to record the timestamp.
fn sync_hidden_state(app: &AppHandle) {
    let visible = match app.get_webview_window("main") {
        Some(window) => window.is_visible().unwrap_or(false),
        // Nothing is mounted, so there is nothing to tear down or to count down.
        None => true,
    };

    match hidden_state_action(
        visible,
        LAST_HIDDEN_TIMESTAMP.load(Ordering::Relaxed),
    ) {
        HiddenStateAction::Clear => mark_shown(),
        HiddenStateAction::Start => mark_hidden(),
        HiddenStateAction::Keep => {}
    }
}

/// Tear down the main webview if preconditions hold. No-op when already destroyed
/// or when state transitions are unsafe. Safe to call from any thread.
pub fn try_destroy_idle(app: &AppHandle) -> bool {
    if WINDOW_LIFECYCLE.load(Ordering::SeqCst) == LIFECYCLE_CLOSING {
        complete_pending_destroy(app);
        return false;
    }

    // A recreate queued by a hotkey / tray / GPU switch is serviced first so the
    // window is back before we consider tearing it down again.
    if service_pending_recreate(app) {
        return false;
    }

    let settings = match app.try_state::<SettingsState>() {
        Some(s) => s,
        None => return false,
    };

    let enabled = settings.idle_destroy_enabled.load(Ordering::Relaxed);
    let timeout_secs = settings.idle_destroy_seconds.load(Ordering::Relaxed);
    let is_destroyed = IS_DESTROYED.load(Ordering::Relaxed);

    sync_hidden_state(app);
    let hidden_since = LAST_HIDDEN_TIMESTAMP.load(Ordering::Relaxed);

    if !should_destroy_now(hidden_since, now_ms(), timeout_secs, enabled, is_destroyed) {
        return false;
    }

    // Transition Open → Closing atomically; bail if someone else is mid-transition.
    if WINDOW_LIFECYCLE
        .compare_exchange(
            LIFECYCLE_OPEN,
            LIFECYCLE_CLOSING,
            Ordering::SeqCst,
            Ordering::Relaxed,
        )
        .is_err()
    {
        return false;
    }
    mark_closing();

    crate::info!(
        "[idle-destroyer] Destroying main webview after {}s of inactivity",
        now_ms().saturating_sub(hidden_since) / 1000
    );

    if !destroy_main_window(app, false) {
        abort_closing();
        return false;
    }

    finish_main_destroy(app, false);

    if WINDOW_LIFECYCLE.load(Ordering::SeqCst) == LIFECYCLE_CLOSED {
        service_pending_recreate(app);
    }
    true
}

/// Undo a `Closing` transition that never reached a teardown, leaving the window
/// usable and the state machine consistent.
fn abort_closing() {
    clear_closing();
    WINDOW_LIFECYCLE.store(LIFECYCLE_OPEN, Ordering::SeqCst);
}

fn destroy_main_window(app: &AppHandle, wait_for_browser_exit: bool) -> bool {
    let Some(win) = app.get_webview_window("main") else {
        webview_environment::mark_main_browser_process_exited();
        return true;
    };

    if wait_for_browser_exit {
        webview_environment::reset_main_browser_process_exit();
        if !webview_environment::watch_main_browser_process_exit(&win) {
            webview_environment::mark_main_browser_process_exited();
            return false;
        }
    } else {
        webview_environment::mark_main_browser_process_exited();
    }

    // A rejected destroy leaves the label registered, so the caller must unwind
    // instead of waiting for a release that can never arrive.
    if let Err(err) = win.destroy() {
        crate::warn!("[idle-destroyer] destroy() failed for the main window: {err}");
        webview_environment::mark_main_browser_process_exited();
        return false;
    }
    true
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
    let mut browser_exited = if wait_for_browser_exit {
        webview_environment::wait_for_main_browser_process_exit(BROWSER_PROCESS_EXIT_TIMEOUT)
    } else {
        true
    };

    if !label_released {
        crate::warn!("[idle-destroyer] Timed out waiting for label to be freed after destroy.");
    }
    if wait_for_browser_exit && !browser_exited {
        crate::warn!(
            "[idle-destroyer] Timed out waiting for WebView2 browser process exit after destroy."
        );
        // Stop waiting on a browser process that already blew its own budget:
        // `complete_pending_destroy` treats this flag as a hard precondition, so
        // leaving it clear would strand the lifecycle in `Closing` forever.
        webview_environment::mark_main_browser_process_exited();
        browser_exited = true;
    }

    LAST_HIDDEN_TIMESTAMP.store(0, Ordering::SeqCst);
    let (lifecycle, is_destroyed, completed) =
        destroy_completion_state(label_released, browser_exited);
    WINDOW_LIFECYCLE.store(lifecycle, Ordering::SeqCst);
    IS_DESTROYED.store(is_destroyed, Ordering::SeqCst);
    if lifecycle != LIFECYCLE_CLOSING {
        clear_closing();
    }
    completed
}

fn destroy_completion_state(label_released: bool, browser_exited: bool) -> (u8, bool, bool) {
    if label_released && browser_exited {
        (LIFECYCLE_CLOSED, true, true)
    } else {
        (LIFECYCLE_CLOSING, label_released, false)
    }
}

/// Finish a teardown whose runtime signals never fully arrived.
///
/// Runs on the ticker thread, never on a caller's thread, so the bounded waits
/// it performs cannot block the main thread.
fn complete_pending_destroy(app: &AppHandle) -> bool {
    let expired = closing_watchdog_expired(
        CLOSING_SINCE_MS.load(Ordering::SeqCst),
        now_ms(),
        CLOSING_WATCHDOG_MS,
    );

    if expired {
        crate::warn!(
            "[idle-destroyer] Teardown exceeded the {CLOSING_WATCHDOG_MS}ms budget; forcing it to settle."
        );
        webview_environment::mark_main_browser_process_exited();
    }

    if app.get_webview_window("main").is_some() {
        if expired {
            // The runtime kept the window registered, so the teardown did not
            // happen. Treat the window as live again instead of polling for a
            // label release that will never come.
            abort_closing();
            IS_DESTROYED.store(false, Ordering::SeqCst);
            RECREATE_PENDING.store(false, Ordering::SeqCst);
        }
        return false;
    }

    if !webview_environment::main_browser_process_exited() {
        return false;
    }

    WINDOW_LIFECYCLE.store(LIFECYCLE_CLOSED, Ordering::SeqCst);
    IS_DESTROYED.store(true, Ordering::SeqCst);
    LAST_HIDDEN_TIMESTAMP.store(0, Ordering::SeqCst);
    clear_closing();

    if RECREATE_PENDING.load(Ordering::SeqCst) {
        return service_pending_recreate(app);
    }
    true
}

/// Rebuild the window for a recreate that was queued while a teardown was in
/// flight, then show it.
///
/// A queued recreate always originates from a user action that expected the
/// window (hotkey, tray, GPU switch), so serving it on the ticker also repairs
/// the request that a hotkey could not complete inline.
fn service_pending_recreate(app: &AppHandle) -> bool {
    if !RECREATE_PENDING.load(Ordering::SeqCst) {
        return false;
    }
    if WINDOW_LIFECYCLE.load(Ordering::SeqCst) != LIFECYCLE_CLOSED {
        return false;
    }
    if app.get_webview_window("main").is_some() {
        // The window is already mounted; the queued request is stale.
        RECREATE_PENDING.store(false, Ordering::SeqCst);
        return false;
    }
    if !recreate_main_window(app) {
        return false;
    }

    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        notify_main_window_shown(app);
    }
    true
}

/// Make sure the main window exists and is ready to be shown.
/// Returns `true` if the window is usable, `false` if the caller should give up
/// for now (a teardown is still settling).
///
/// When the window was destroyed by the idle destroyer, this rebuilds it from
/// `tauri.conf.json` so config drift is impossible.
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

    // Building on the main thread would deadlock the event loop, so a
    // main-thread caller hands the work to the ticker instead: roll the state
    // machine back, let the caller queue the request with
    // `request_recreate_after_destroy`, and the ticker rebuilds from a thread
    // where `build()` is safe.
    if !can_build_window_here(runs_on_main_thread()) {
        crate::warn!(
            "[idle-destroyer] Recreate requested from the main thread; deferring to the ticker."
        );
        WINDOW_LIFECYCLE.store(LIFECYCLE_CLOSED, Ordering::SeqCst);
        return false;
    }

    // The runtime frees the `main` label on the main thread, so a caller that
    // already runs there could only spin until its own timeout. Defer to the
    // ticker instead of blocking here.
    if !label_is_free_for_recreate(app) {
        crate::warn!(
            "[idle-destroyer] Label 'main' is still registered; deferring recreate to the next tick."
        );
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

    match WebviewWindowBuilder::from_config(app, config).and_then(|b| b.build()) {
        Ok(_) => {
            WINDOW_LIFECYCLE.store(LIFECYCLE_OPEN, Ordering::SeqCst);
            IS_DESTROYED.store(false, Ordering::SeqCst);
            // Defensive: clear the hidden timestamp so a racing tick from the
            // background destroyer thread does not immediately re-destroy the
            // freshly-recreated window before the caller reaches mark_shown().
            LAST_HIDDEN_TIMESTAMP.store(0, Ordering::SeqCst);
            clear_closing();
            consume_recreate_request();
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

/// Retire a queued recreate request, because the build that satisfied it just
/// landed.
///
/// The flag has to be consumed here and nowhere else. Every caller that defers a
/// recreate sets it, but if it survives a successful build then the next teardown
/// undoes itself: `try_destroy_idle` ends a destroy by calling
/// `service_pending_recreate`, which builds a window and *shows* it. The user
/// hides the window, one tick later it reopens by itself, and their next hotkey
/// press then hides it instead of showing it — the memory is never reclaimed and
/// the hotkey appears to be stuck.
fn consume_recreate_request() {
    RECREATE_PENDING.store(false, Ordering::SeqCst);
}

/// Pure decision: may this thread call `WebviewWindowBuilder::build()` now?
///
/// `build()` hands the window to the event loop and waits for it. On the thread
/// that *is* the event loop that wait can never be satisfied: the window
/// half-registers, `build()` never returns, and the lifecycle is parked in
/// `Opening` with `IS_DESTROYED` still set. Neither is ever cleared again, so
/// the idle destroyer stops reclaiming memory for the rest of the process
/// lifetime and the half-built webview keeps a renderer and GPU process alive.
///
/// This is the ordinary path, not an exotic one: `tauri-plugin-global-shortcut`
/// dispatches its `Pressed` callback on the main thread, so every toggle from
/// the hotkey or the tray lands here on the one thread that cannot build.
pub fn can_build_window_here(on_main_thread: bool) -> bool {
    !on_main_thread
}

/// Report whether the `main` label is free for a rebuild right now.
///
/// Waiting for the release is only safe off the main thread: Tauri dispatches
/// the teardown there, and sync commands run inline on it, so a main-thread
/// caller blocking here would starve the only thread able to free the label.
/// Callers that cannot proceed queue a recreate and let the ticker retry.
fn label_is_free_for_recreate(app: &AppHandle) -> bool {
    if runs_on_main_thread() {
        return app.get_webview_window("main").is_none();
    }
    wait_for_label_release(app, LABEL_RELEASE_TIMEOUT)
}

/// Record the thread Tauri runs the event loop on.
///
/// Call once from the setup hook, which executes on the main thread. Window
/// teardown and window creation both dispatch there, which is what makes the
/// main-thread check in `label_is_free_for_recreate` meaningful.
pub fn note_main_thread() {
    let _ = MAIN_THREAD_ID.set(std::thread::current().id());
}

fn runs_on_main_thread() -> bool {
    MAIN_THREAD_ID.get() == Some(&std::thread::current().id())
}

/// Public entry for callers (hotkey handler, tray, frontend) that want to
/// guarantee the main window exists before showing it. Idempotent.
pub fn ensure_main_window(app: &AppHandle) -> bool {
    if WINDOW_LIFECYCLE.load(Ordering::SeqCst) == LIFECYCLE_CLOSING {
        // Runs the watchdog, so this cannot stay stuck behind a teardown that
        // the runtime never confirmed.
        complete_pending_destroy(app);
    }

    let window_exists = app.get_webview_window("main").is_some();
    let is_destroyed = IS_DESTROYED.load(Ordering::SeqCst);
    let lifecycle = WINDOW_LIFECYCLE.load(Ordering::SeqCst);

    if !should_recreate_main_window(window_exists, is_destroyed, lifecycle) {
        return true;
    }

    if !recreate_main_window(app) {
        // The label is still held or the runtime refused the rebuild. Queue the
        // request so the ticker finishes it instead of dropping this one.
        request_recreate_after_destroy();
        return false;
    }

    main_window_ready(
        app.get_webview_window("main").is_some(),
        IS_DESTROYED.load(Ordering::SeqCst),
        WINDOW_LIFECYCLE.load(Ordering::SeqCst),
    )
}

pub fn restart_main_window_for_gpu_switch(app: &AppHandle, disabled: bool) -> bool {
    crate::app::gpu_switcher::apply_gpu_disable_env(disabled);

    if WINDOW_LIFECYCLE
        .compare_exchange(
            LIFECYCLE_OPEN,
            LIFECYCLE_CLOSING,
            Ordering::SeqCst,
            Ordering::Relaxed,
        )
        .is_err()
    {
        request_recreate_after_destroy();
        return false;
    }
    mark_closing();

    let was_visible = app
        .get_webview_window("main")
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false);

    if let Some(preview) = app.get_webview_window("compact-preview") {
        let _ = preview.destroy();
    }

    if !destroy_main_window(app, true) {
        abort_closing();
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
        // The label was still held; the ticker will finish the rebuild and show
        // the window, because the recreate request is already queued.
        if was_visible {
            request_recreate_after_destroy();
        }
        return false;
    }

    if was_visible {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.show();
            notify_main_window_shown(app);
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
    clear_closing();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_destroy_now_disabled_when_feature_off() {
        assert!(!should_destroy_now(1000, 61_000, 60, false, false));
    }

    #[test]
    fn main_thread_must_not_build_the_window() {
        assert!(!can_build_window_here(true));
    }

    #[test]
    fn background_thread_may_build_the_window() {
        assert!(can_build_window_here(false));
    }

    #[test]
    fn captures_reach_a_visible_window() {
        assert!(should_deliver_capture(true, true));
    }

    #[test]
    fn captures_wait_while_the_window_is_hidden() {
        assert!(!should_deliver_capture(false, true));
    }

    #[test]
    fn captures_wait_while_the_webview_is_torn_down() {
        // A destroyed window has no frontend to wake; its replacement fetches the
        // whole history when it mounts, so an emit here would be pure waste.
        assert!(!should_deliver_capture(true, false));
        assert!(!should_deliver_capture(false, false));
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
        assert!(should_recreate_main_window(true, true, LIFECYCLE_CLOSED));
    }

    #[test]
    fn main_window_not_ready_when_destroyed_state_keeps_stale_label() {
        assert!(!main_window_ready(true, true, LIFECYCLE_CLOSED));
    }

    #[test]
    fn should_not_recreate_when_open_state_has_window() {
        assert!(!should_recreate_main_window(true, false, LIFECYCLE_OPEN));
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

    #[test]
    fn hidden_state_clears_while_the_window_is_on_screen() {
        assert_eq!(hidden_state_action(true, 0), HiddenStateAction::Clear);
        assert_eq!(hidden_state_action(true, 5_000), HiddenStateAction::Clear);
    }

    #[test]
    fn hidden_state_starts_when_a_hide_path_forgot_the_timestamp() {
        assert_eq!(hidden_state_action(false, 0), HiddenStateAction::Start);
    }

    #[test]
    fn hidden_state_keeps_an_in_flight_countdown() {
        assert_eq!(hidden_state_action(false, 5_000), HiddenStateAction::Keep);
    }

    #[test]
    fn closing_watchdog_never_fires_outside_the_closing_state() {
        assert!(!closing_watchdog_expired(0, 60_000, CLOSING_WATCHDOG_MS));
    }

    #[test]
    fn closing_watchdog_waits_for_the_full_budget() {
        let started = 1_000;
        assert!(!closing_watchdog_expired(
            started,
            started + CLOSING_WATCHDOG_MS - 1,
            CLOSING_WATCHDOG_MS
        ));
        assert!(closing_watchdog_expired(
            started,
            started + CLOSING_WATCHDOG_MS,
            CLOSING_WATCHDOG_MS
        ));
    }

    #[test]
    fn closing_watchdog_tolerates_a_backwards_clock() {
        assert!(!closing_watchdog_expired(50_000, 10_000, CLOSING_WATCHDOG_MS));
    }

    #[test]
    fn label_release_budget_exceeds_the_common_case_teardown() {
        // The label wait runs on the ticker thread; it has to outlast a main
        // thread that is busy reaping the WebView2 renderer, or the recreate is
        // abandoned and the next hotkey press is swallowed.
        assert!(LABEL_RELEASE_TIMEOUT >= Duration::from_millis(500));
    }

    #[test]
    fn closing_budget_outlasts_the_browser_process_exit_timeout() {
        // Forcing the lifecycle closed before the browser process gives up would
        // recreate a window on top of a browser process that is still alive.
        assert!(Duration::from_millis(CLOSING_WATCHDOG_MS) > BROWSER_PROCESS_EXIT_TIMEOUT);
    }
}
