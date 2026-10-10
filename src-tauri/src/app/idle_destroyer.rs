use std::sync::atomic::Ordering;
use std::sync::OnceLock;
use std::thread::ThreadId;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, WebviewWindowBuilder};

use crate::app_state::SettingsState;
use crate::global_state::{
    CLOSING_SINCE_MS, FRONTEND_CATCHUP_PENDING, IS_DESTROYED, LAST_HIDDEN_TIMESTAMP,
    MAIN_WINDOW_PAINTED, RECREATE_PENDING, SHOW_DEADLINE_MS, SHOW_PENDING, SHOW_REQUEST_ID,
    WINDOW_LIFECYCLE,
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

/// How long a rebuilt main window may sit hidden waiting for its first paint
/// before it is shown anyway.
///
/// The wait exists to avoid flashing an empty window, not to delay the app: a
/// frontend that never reaches a paint — a script error, a command that is not
/// registered, a webview that loads and dies — must not leave the clipboard
/// window permanently unopenable. This is the budget for that.
pub const FIRST_PAINT_TIMEOUT_MS: u64 = 1_500;

/// Poll interval for that wait. Short because the whole point is to react within
/// the boot itself; the watcher only runs while a show is actually pending, and
/// recreates are rare.
const FIRST_PAINT_POLL: Duration = Duration::from_millis(8);

/// Pure decision: may a rebuilt main window be put on screen yet?
///
/// `painted` is the frontend reporting that it drew a frame. `deadline_ms == 0`
/// means no deadline was armed, and the answer is then whatever `painted` says:
/// a caller that did not just rebuild must never be made to wait.
pub fn ready_to_show(painted: bool, now_ms: u64, deadline_ms: u64) -> bool {
    painted || (deadline_ms != 0 && now_ms >= deadline_ms)
}

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

    destroy_discardable_overlays(app, true);

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

/// Overlays whose webview is worth tearing down with the main window.
///
/// Both are rebuilt on demand, so keeping them alive only holds a renderer and
/// the compositor surface that goes with it. The measurements on this machine
/// are why: from the same four-window start, destroying only the main window
/// left the GPU process at 96.9 MB working set and 230.8 MB committed, flat for
/// the whole 150-second watch, while destroying these two as well let it fall to
/// 0.9 MB inside 30 seconds. That is what a leaked-looking GPU process looks
/// like.
///
/// `quick-paste` is deliberately not here. Its entire purpose is to answer a
/// hotkey without a round trip through the list, and rebuilding a webview per
/// press would cost exactly the latency the overlay exists to remove.
const DISCARDABLE_OVERLAYS: [&str; 2] = ["compact-preview", "region-select"];

/// Tear down the overlays that will be rebuilt on their next use.
///
/// `keep_visible` spares an overlay the user is looking at. Both of these park
/// themselves the moment they are done, so the next idle tick collects the one
/// that was spared -- but a selection that is still being dragged, or a preview
/// still on screen, is not the app's to take away. The main window has no such
/// question: it is hidden by definition for this to run at all.
///
/// The compact preview is created by the frontend and its handle lives in the
/// main window's JavaScript context, which the idle teardown removes in the same
/// breath -- so the handle cannot be left dangling the way it would if the
/// preview outlived its owner.
fn destroy_discardable_overlays(app: &AppHandle, keep_visible: bool) {
    for label in DISCARDABLE_OVERLAYS {
        let Some(window) = app.get_webview_window(label) else {
            continue;
        };
        if keep_visible && window.is_visible().unwrap_or(false) {
            continue;
        }
        if let Err(err) = window.destroy() {
            crate::warn!("[idle-destroyer] Failed to destroy '{}': {}", label, err);
        }
    }
}

/// Collect overlays that parked themselves after the main window was already gone.
///
/// The idle teardown spares a visible overlay, so a region selection that is
/// still on screen when the app goes idle keeps its webview — and it parks
/// itself moments later, on cancel or once the capture replies. Nothing else
/// would ever collect it: the path that reaches the overlays runs inside
/// `try_destroy_idle`, and "the main window is absent" is precisely the state
/// that stops that from running, because with nothing mounted the app counts as
/// shown and the countdown never comes due.
///
/// The main-window check is what keeps this from reaching into normal use. While
/// the app is up, a parked preview is the hover cache, and destroying it there
/// would mean rebuilding a webview on the next hover — the exact latency the
/// overlay exists to avoid.
fn collect_parked_overlays_while_destroyed(app: &AppHandle) {
    if app.get_webview_window("main").is_some() {
        return;
    }
    destroy_discardable_overlays(app, true);
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

    // The build above returned a window that has not loaded anything yet.
    // Showing it here is what puts an empty frame on screen for the length of a
    // boot; with the idle destroyer set to a few seconds this is the path every
    // hotkey press takes.
    let handle = app.clone();
    show_main_window_when_ready(move || show_main_window_now(&handle));
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
            // A rebuilt webview has drawn nothing yet. `build()` returning only
            // means the window object exists; the page has not even begun to
            // load, so anything that shows it now puts an empty window on screen.
            // Arm the wait before returning, because the caller's very next move
            // is to show it.
            arm_first_paint_wait();
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

/// Start a fresh wait for the main webview's first paint, with its deadline.
///
/// Called from `recreate_main_window` on success, *before* the caller gets a
/// chance to show the window it just built.
fn arm_first_paint_wait() {
    MAIN_WINDOW_PAINTED.store(false, Ordering::SeqCst);
    SHOW_DEADLINE_MS.store(
        now_ms().saturating_add(FIRST_PAINT_TIMEOUT_MS),
        Ordering::SeqCst,
    );
    // Any show still waiting is waiting on the controller that just went away,
    // and its closure holds a handle to it. The caller's own request comes
    // straight after this and opens a fresh one.
    cancel_pending_show();
}

/// Record that the main webview has drawn a frame worth showing.
pub fn mark_main_window_painted() {
    MAIN_WINDOW_PAINTED.store(true, Ordering::SeqCst);
}

fn show_main_window_now(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        notify_main_window_shown(app);
    }
}

/// Open a show request, superseding any that is still waiting.
///
/// Returns the new id. Every caller that wants the window on screen takes one
/// of these, and so does every caller that wants it *not* to appear — that is
/// what makes "the user cancelled" and "someone asked again" the same
/// mechanism rather than two flags that can disagree.
pub fn begin_show_request() -> u64 {
    SHOW_PENDING.store(true, Ordering::SeqCst);
    SHOW_REQUEST_ID.fetch_add(1, Ordering::AcqRel) + 1
}

/// Is a rebuilt window still on its way up?
///
/// `toggle_window` cannot use `window.is_visible()` for this: during the wait
/// the window really is not visible, so a second press would look like another
/// request to show rather than the cancel it is.
pub fn show_is_pending() -> bool {
    SHOW_PENDING.load(Ordering::Acquire)
}

/// Abandon every waiting show. The window stays hidden.
pub fn cancel_pending_show() {
    SHOW_REQUEST_ID.fetch_add(1, Ordering::AcqRel);
    SHOW_PENDING.store(false, Ordering::SeqCst);
}

/// What a waiting watcher should do on one poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatcherAction {
    /// Neither painted nor out of time yet.
    KeepWaiting,
    /// The window is ready, but a newer request — or a cancel — has taken over.
    /// Showing now would put back a window the user has already closed.
    Abort,
    /// Show.
    Show,
}

/// Pure decision for the watcher, so the ordering it depends on can be tested
/// without a thread and a webview.
pub fn watcher_action(
    painted: bool,
    now_ms: u64,
    deadline_ms: u64,
    request: u64,
    current_request: u64,
) -> WatcherAction {
    if !ready_to_show(painted, now_ms, deadline_ms) {
        return WatcherAction::KeepWaiting;
    }
    if request != current_request {
        return WatcherAction::Abort;
    }
    WatcherAction::Show
}

/// Put the main window on screen, but not before its webview has drawn.
///
/// `build()` returning says the window object exists — nothing more. The page
/// has not started loading, so showing straight after it puts an empty window on
/// screen for the length of a boot. With the idle destroyer set to a few seconds
/// that is the path *every* hotkey press takes, which is why the flash was
/// constant rather than occasional.
///
/// The caller's own show rules travel with the closure, because they are not the
/// same everywhere: the hotkey path adjusts Windows Z order, the tray path emits
/// a frontend event, and only now may the window claim the keyboard. Deferring
/// just `show()` and running the rest immediately would let a window that is not
/// on screen yet swallow the foreground app's arrow keys — the global hook gates
/// on `NAVIGATION_ENABLED` and `IS_HIDDEN`, not on visibility.
///
/// Windows that were not just rebuilt are unaffected: `MAIN_WINDOW_PAINTED`
/// starts `true`, so they show synchronously as before.
pub fn show_main_window_when_ready<F>(show: F)
where
    F: Fn() + Send + 'static,
{
    let request = begin_show_request();
    if ready_to_show(
        MAIN_WINDOW_PAINTED.load(Ordering::Acquire),
        now_ms(),
        SHOW_DEADLINE_MS.load(Ordering::SeqCst),
    ) {
        SHOW_PENDING.store(false, Ordering::SeqCst);
        show();
        return;
    }

    std::thread::spawn(move || loop {
        std::thread::sleep(FIRST_PAINT_POLL);
        let painted = MAIN_WINDOW_PAINTED.load(Ordering::Acquire);
        match watcher_action(
            painted,
            now_ms(),
            SHOW_DEADLINE_MS.load(Ordering::SeqCst),
            request,
            SHOW_REQUEST_ID.load(Ordering::SeqCst),
        ) {
            WatcherAction::KeepWaiting => continue,
            // A hide, or a second toggle, took over while this one waited. The
            // replacement owns the decision now.
            WatcherAction::Abort => {
                crate::info!(
                    "[idle-destroyer] Abandoned a pending show: the window was hidden or the \
                     request was superseded before its first paint."
                );
                return;
            }
            WatcherAction::Show => {
                SHOW_PENDING.store(false, Ordering::SeqCst);
                if painted {
                    crate::info!("[idle-destroyer] Showing rebuilt window on its first painted frame.");
                } else {
                    crate::warn!(
                        "[idle-destroyer] Rebuilt window never reported a paint; showing it anyway after \
                         {FIRST_PAINT_TIMEOUT_MS}ms."
                    );
                }
                show();
                return;
            }
        }
    });
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

    destroy_discardable_overlays(app, false);

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
        // Same reason as the recreate path: the GPU switch tore the webview down
        // and the replacement has drawn nothing yet.
        let handle = app.clone();
        show_main_window_when_ready(move || show_main_window_now(&handle));
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
            collect_parked_overlays_while_destroyed(&app);
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

    // The gate that keeps a rebuilt window off screen until it has drawn.
    //
    // What went wrong: `service_pending_recreate` called `build()` and then
    // `show()` back to back, and the log showed the two one millisecond apart.
    // `build()` returning says the window object exists — the page had not
    // started loading — so every hotkey press after an idle teardown put an
    // empty window on screen for the length of a boot.

    #[test]
    fn a_rebuilt_window_waits_for_its_first_paint() {
        let now = 10_000;
        let deadline = now + FIRST_PAINT_TIMEOUT_MS;
        assert!(
            !ready_to_show(false, now, deadline),
            "showing here is exactly the empty-window flash this gate exists to prevent"
        );
    }

    #[test]
    fn a_window_that_has_painted_shows_without_waiting() {
        // The ordinary path — a window that was not rebuilt — must not be made
        // to wait for anything, or every show would gain a frame of latency.
        assert!(ready_to_show(true, 10_000, 10_000 + FIRST_PAINT_TIMEOUT_MS));
    }

    #[test]
    fn the_deadline_shows_a_window_that_never_painted() {
        // Without this the wait could turn a frontend error into a clipboard
        // window that can never be opened again, which is worse than the flash.
        let armed = 10_000;
        let deadline = armed + FIRST_PAINT_TIMEOUT_MS;
        assert!(!ready_to_show(false, armed, deadline));
        assert!(!ready_to_show(false, deadline - 1, deadline));
        assert!(ready_to_show(false, deadline, deadline));
        assert!(ready_to_show(false, deadline + 1, deadline));
    }

    #[test]
    fn no_deadline_means_never_wait() {
        // `0` is "this caller did not rebuild anything". A false `painted` must
        // not be able to stall it — the state is process-wide, so a stale or
        // cleared flag would otherwise wedge every later show.
        assert!(!ready_to_show(false, 10_000, 0));
        assert!(ready_to_show(true, 10_000, 0));
        assert!(!ready_to_show(false, u64::MAX, 0));
    }

    #[test]
    fn the_paint_budget_is_long_enough_to_beat_a_cold_boot() {
        // Shorter than this and the window shows before React has mounted, which
        // is the same empty frame under a different name. Longer and a genuinely
        // broken frontend keeps the user waiting for nothing.
        assert!(FIRST_PAINT_TIMEOUT_MS >= 500);
        assert!(FIRST_PAINT_TIMEOUT_MS <= 3_000);
    }

    // Cancelling a show that has not landed yet.
    //
    // What went wrong: the wait was unconditional. A second hotkey press, an
    // Escape, or a hide from anywhere else all left the original watcher armed,
    // and it put the window on screen a moment later regardless. Because
    // `toggle_window` decides show-or-hide from `window.is_visible()`, and a
    // waiting window genuinely is not visible, the second press was read as
    // *another* request to show rather than as the cancel it was.

    #[test]
    fn a_waiting_show_is_abandoned_when_the_window_is_hidden() {
        let request = begin_show_request();
        assert!(show_is_pending());

        cancel_pending_show();
        assert!(!show_is_pending(), "a hidden window must not still be on its way up");

        // The first paint arrives after the user closed it.
        assert_eq!(
            watcher_action(true, 10_000, 11_500, request, SHOW_REQUEST_ID.load(Ordering::SeqCst)),
            WatcherAction::Abort,
            "a request the user cancelled must not reopen the window"
        );
    }

    #[test]
    fn the_timeout_fallback_respects_a_cancelled_show() {
        // The deadline is there so a broken frontend cannot wedge the window
        // shut. It is not a licence to resurrect one the user closed, and that
        // distinction is invisible unless both halves are asserted.
        let request = begin_show_request();
        cancel_pending_show();

        assert_eq!(
            watcher_action(
                false,
                99_999,
                11_500,
                request,
                SHOW_REQUEST_ID.load(Ordering::SeqCst),
            ),
            WatcherAction::Abort,
            "the fallback must not show a window the user already closed"
        );
    }

    #[test]
    fn a_rebuilt_window_still_shows_when_nothing_cancels_it() {
        cancel_pending_show();
        let request = begin_show_request();
        let current = SHOW_REQUEST_ID.load(Ordering::SeqCst);
        assert_eq!(request, current);

        assert_eq!(
            watcher_action(false, 10_000, 11_500, request, current),
            WatcherAction::KeepWaiting
        );
        assert_eq!(
            watcher_action(true, 10_000, 11_500, request, current),
            WatcherAction::Show
        );
        assert_eq!(
            watcher_action(false, 11_500, 11_500, request, current),
            WatcherAction::Show,
            "a window that never painted is shown anyway once the budget is spent"
        );
    }

    #[test]
    fn a_later_request_supersedes_an_earlier_one() {
        cancel_pending_show();
        let first = begin_show_request();
        let second = begin_show_request();
        assert_ne!(first, second);

        assert_eq!(
            watcher_action(true, 10_000, 11_500, first, second),
            WatcherAction::Abort,
            "two shows in a row must end with exactly one window on screen"
        );
        assert_eq!(
            watcher_action(true, 10_000, 11_500, second, second),
            WatcherAction::Show
        );
        cancel_pending_show();
    }

    #[test]
    fn the_ways_a_user_closes_a_waiting_window_all_reach_the_gate() {
        // `watcher_action` proves the watcher honours a cancel, and the tests
        // above prove `cancel_pending_show` produces one. Neither says that
        // anything calls it — the same gap that let the unconditional wait
        // through in the first place.
        let source = include_str!("window_manager.rs");
        let body = source
            .split_once("pub fn toggle_window")
            .expect("toggle_window is the hotkey entry point")
            .1
            .split_once("\n}\n")
            .expect("a top-level function body ends with `}` in column zero")
            .0;
        assert!(
            body.contains("cancel_pending_show()"),
            "a second hotkey press reads as another request to show while the window is waiting, \
             so toggle_window has to recognise and cancel it"
        );

        let hide = source
            .split_once("pub fn hide_window_cmd")
            .expect("hide_window_cmd is the frontend's hide")
            .1
            .split_once("\n}\n")
            .expect("a top-level function body ends with `}` in column zero")
            .0;
        assert!(
            hide.contains("cancel_pending_show()"),
            "hiding a window that is still waiting for its first paint has to abandon that show"
        );
    }

    #[test]
    fn a_window_that_has_not_shown_yet_may_not_claim_the_keyboard() {
        // The global hook gates on `NAVIGATION_ENABLED` and `IS_HIDDEN`, never on
        // visibility. Raising either for a window that is still waiting means
        // swallowing the foreground app's arrow keys and Escape until the paint
        // lands — up to the whole budget.
        let source = include_str!("window_manager.rs");
        let body = source
            .split_once("pub fn toggle_window")
            .expect("toggle_window is the hotkey entry point")
            .1
            .split_once("\n}\n")
            .expect("a top-level function body ends with `}` in column zero")
            .0;

        let gate = body
            .find("show_main_window_when_ready(move || {")
            .expect("toggle_window routes its show through the gate");
        // Everything from the gate onwards is what runs at the moment the window
        // actually appears. The cancel branch above the gate clears the same
        // flags, and legitimately so — it is describing a window that stays
        // hidden — which is why this asks where each claim is committed rather
        // than whether it appears at all.
        let at_show = &body[gate..];
        for claim in [
            "NAVIGATION_ENABLED.store(true",
            "IS_HIDDEN.store(false",
            "notify_main_window_shown",
            "LAST_SHOW_TIMESTAMP.store",
        ] {
            assert!(
                at_show.contains(claim),
                "{claim} is not committed inside the gate, so it claims a window that may never \
                 reach the screen"
            );
        }
    }

    #[test]
    fn a_path_that_may_rebuild_the_window_gates_its_show() {
        // Regression guard for the shape this bug actually took. The decision
        // function was gated and its pure tests passed, yet three of the four
        // paths that can run straight after a rebuild still called
        // `window.show()` on their own — including the hotkey, which is the one
        // the user actually saw the flash on. Unit tests on `ready_to_show`
        // cannot see that, because the bug is in what the callers never asked
        // it.
        //
        // So this checks the callers instead: `ensure_main_window` is the single
        // entry point that can build a fresh webview, and a function that calls
        // it must reach its gate before it reaches a show.
        for (file, source) in [
            ("window_manager.rs", include_str!("window_manager.rs")),
            ("setup.rs", include_str!("setup.rs")),
            ("idle_destroyer.rs", include_str!("idle_destroyer.rs")),
        ] {
            let lines: Vec<&str> = source.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                let trimmed = line.trim_start();
                if trimmed.starts_with("//") || !line.contains("ensure_main_window(") {
                    continue;
                }
                let rest = &lines[i..];
                let gate = rest
                    .iter()
                    .position(|l| l.contains("show_main_window_when_ready("));
                let show = rest.iter().position(|l| l.contains(".show()"));
                match (gate, show) {
                    (Some(g), Some(s)) => assert!(
                        g < s,
                        "{file}:{} rebuilds the webview and then shows it before the gate",
                        i + 1
                    ),
                    (None, Some(_)) => panic!(
                        "{file}:{} rebuilds the webview and shows it with no gate at all",
                        i + 1
                    ),
                    // No show after this call site, or a gate and no show: nothing
                    // on screen to flash.
                    _ => {}
                }
            }
        }

        // The gate is not magic: it defers a call the caller hands it. So the one
        // raw show left in this file is the helper the gate and the two internal
        // callers share, and there must not be a second one. The test module is
        // cut off first — its own matchers contain the literal it looks for.
        let module = include_str!("idle_destroyer.rs");
        let code = module.split_once("mod tests {").map_or(module, |(head, _)| head);
        let raw_shows = code
            .lines()
            .filter(|line| line.contains(".show()") && !line.contains("show_window"))
            .count();
        assert_eq!(raw_shows, 1, "idle_destroyer.rs should show main in exactly one place");
    }
}
