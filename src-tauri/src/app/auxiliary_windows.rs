use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::app_state::SettingsState;
use crate::global_state::{IS_DESTROYED, WINDOW_LIFECYCLE};

// Creation and showing must be one operation: the idle collector otherwise
// sees a newly built, still-hidden overlay and destroys it before its first use.
static WINDOW_OPERATIONS: Mutex<()> = Mutex::new(());
static ACTIVITY_EPOCH: OnceLock<Instant> = OnceLock::new();
static QUICK_PASTE_ACTIVITY_MS: AtomicU64 = AtomicU64::new(0);
static REGION_SELECT_ACTIVITY_MS: AtomicU64 = AtomicU64::new(0);
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(0);
static QUICK_PASTE_INSTANCE: AuxiliaryInstance = AuxiliaryInstance::new();
static REGION_SELECT_INSTANCE: AuxiliaryInstance = AuxiliaryInstance::new();
static PREWARM_PENDING: AtomicBool = AtomicBool::new(false);

struct AuxiliaryInstance {
    generation: AtomicU64,
    ready_generation: AtomicU64,
}

impl AuxiliaryInstance {
    const fn new() -> Self {
        Self {
            generation: AtomicU64::new(0),
            ready_generation: AtomicU64::new(0),
        }
    }

    fn mark_ready(&self, generation: u64) -> bool {
        if generation == 0 || self.generation.load(Ordering::SeqCst) != generation {
            return false;
        }
        self.ready_generation.fetch_max(generation, Ordering::SeqCst);
        true
    }

    fn ready(&self, generation: u64) -> bool {
        generation != 0
            && self.generation.load(Ordering::SeqCst) == generation
            && self.ready_generation.load(Ordering::SeqCst) == generation
    }
}

fn instance(label: &str) -> Option<&'static AuxiliaryInstance> {
    match label {
        "quick-paste" => Some(&QUICK_PASTE_INSTANCE),
        "region-select" => Some(&REGION_SELECT_INSTANCE),
        _ => None,
    }
}

/// The renderer's acknowledgement must not take WINDOW_OPERATIONS: a cold
/// opener holds that lock while waiting for this atomic signal.
pub fn mark_ready(label: &str, generation: u64) -> bool {
    instance(label)
        .map(|instance| instance.mark_ready(generation))
        .unwrap_or(false)
}

fn append_instance_query(url: &str, generation: u64) -> String {
    let (path, fragment) = url.split_once('#').unwrap_or((url, ""));
    let separator = if path.contains('?') { '&' } else { '?' };
    let suffix = if fragment.is_empty() {
        String::new()
    } else {
        format!("#{fragment}")
    };
    format!("{path}{separator}instance={generation}{suffix}")
}

fn build_window(app: &AppHandle, label: &str) -> Result<WebviewWindow, String> {
    let mut config = app
        .config()
        .app
        .windows
        .iter()
        .find(|config| config.label == label)
        .cloned()
        .ok_or_else(|| format!("Auxiliary window '{label}' is not configured"))?;
    if label == "region-select" {
        // A parked selector needs its document, not a display-sized GPU surface.
        // Its show path restores monitor geometry before making it visible.
        config.width = 1.0;
        config.height = 1.0;
    }
    if let Some(instance) = instance(label) {
        let generation = NEXT_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
        match &mut config.url {
            WebviewUrl::App(path) => {
                *path = append_instance_query(&path.to_string_lossy(), generation).into();
            }
            WebviewUrl::External(url) | WebviewUrl::CustomProtocol(url) => {
                url.query_pairs_mut()
                    .append_pair("instance", &generation.to_string());
            }
            _ => return Err(format!("Unsupported auxiliary window URL for '{label}'")),
        }
        instance.ready_generation.store(0, Ordering::SeqCst);
        instance.generation.store(generation, Ordering::SeqCst);
    }
    WebviewWindowBuilder::from_config(app, &config)
        .and_then(|builder| builder.build())
        .map_err(|err| format!("Failed to build auxiliary window '{label}': {err}"))
}

fn wait_until_ready(label: &str) -> Result<(), String> {
    let Some(instance) = instance(label) else {
        return Ok(());
    };
    let generation = instance.generation.load(Ordering::SeqCst);
    let started = Instant::now();
    while !instance.ready(generation) {
        if started.elapsed() >= Duration::from_millis(1500) {
            crate::warn!("[auxiliary-windows] '{label}' instance {generation} was not ready in time");
            return Err(format!("Auxiliary window '{label}' is not ready"));
        }
        std::thread::sleep(Duration::from_millis(8));
    }
    Ok(())
}

fn can_prewarm(app: &AppHandle) -> bool {
    WINDOW_LIFECYCLE.load(Ordering::SeqCst) == crate::app::idle_destroyer::LIFECYCLE_OPEN
        && !IS_DESTROYED.load(Ordering::SeqCst)
        && app.get_webview_window("main").is_some()
}

fn prewarm_enabled_windows(app: &AppHandle) {
    for label in ["quick-paste", "region-select"] {
        let enabled = app
            .try_state::<SettingsState>()
            .map(|settings| match label {
                "quick-paste" => settings.quick_paste_enabled.load(Ordering::Relaxed),
                _ => settings.screenshot_enabled.load(Ordering::Relaxed),
            })
            .unwrap_or(false);
        if !enabled || !can_prewarm(app) {
            continue;
        }
        let _guard = match WINDOW_OPERATIONS.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        if !can_prewarm(app) {
            return;
        }
        if app.get_webview_window(label).is_none() {
            match build_window(app, label) {
                Ok(window) => {
                    crate::app::webview_memory::lower_window_memory(&window, "auxiliary-prewarm");
                }
                Err(err) => crate::warn!("[auxiliary-windows] Prewarm failed: {err}"),
            }
        }
    }
}

/// Warm enabled overlays after the main frontend is ready, keeping the first
/// frame and fully idle periods free of auxiliary creation work.
pub fn schedule_prewarm(app: AppHandle) {
    if PREWARM_PENDING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(250));
        prewarm_enabled_windows(&app);
        PREWARM_PENDING.store(false, Ordering::SeqCst);
    });
}

fn activity_clock_ms() -> u64 {
    (ACTIVITY_EPOCH.get_or_init(Instant::now).elapsed().as_millis() as u64).saturating_add(1)
}

fn last_activity(label: &str) -> Option<&'static AtomicU64> {
    match label {
        "quick-paste" => Some(&QUICK_PASTE_ACTIVITY_MS),
        "region-select" => Some(&REGION_SELECT_ACTIVITY_MS),
        _ => None,
    }
}

/// Show and hide paths can call this while already holding WINDOW_OPERATIONS.
pub fn record_activity(label: &str) {
    if let Some(activity) = last_activity(label) {
        activity.fetch_max(activity_clock_ms(), Ordering::SeqCst);
    }
}

fn should_collect(
    visible: bool,
    force: bool,
    last_activity_ms: u64,
    now_ms: u64,
    grace_ms: u64,
) -> bool {
    force
        || (!visible
            && (last_activity_ms == 0 || now_ms.saturating_sub(last_activity_ms) >= grace_ms))
}

/// Run off the main thread: building and querying windows dispatch to it.
pub fn with_window<T>(
    app: &AppHandle,
    label: &str,
    action: impl FnOnce(&WebviewWindow, bool) -> Result<T, String>,
) -> Result<T, String> {
    if crate::app::idle_destroyer::runs_on_main_thread() {
        return Err("Auxiliary windows must be opened off the main thread".to_string());
    }
    let _guard = WINDOW_OPERATIONS
        .lock()
        .map_err(|err| format!("Auxiliary window lock poisoned: {err}"))?;
    let (window, created) = match app.get_webview_window(label) {
        Some(window) => (window, false),
        None => (build_window(app, label)?, true),
    };
    wait_until_ready(label)?;
    action(&window, created)
}

/// Serialize collection with creation/showing, then wait for asynchronous label
/// release so the next hotkey cannot reuse a handle already being destroyed.
pub fn destroy_windows(
    app: &AppHandle,
    labels: &[&str],
    keep_visible: bool,
    idle_grace_seconds: u64,
) {
    if crate::app::idle_destroyer::runs_on_main_thread() {
        crate::warn!("[auxiliary-windows] Skipped collection on the main thread");
        return;
    }
    let _guard = match WINDOW_OPERATIONS.lock() {
        Ok(guard) => guard,
        Err(err) => {
            crate::warn!("[auxiliary-windows] Window lock poisoned: {err}");
            return;
        }
    };
    for label in labels {
        let Some(window) = app.get_webview_window(label) else {
            continue;
        };
        let visible = window.is_visible().unwrap_or(true);
        // A capture can hide its native surface before its IPC hide arrives.
        // Keep long visible sessions recent during that brief handoff too.
        if visible {
            record_activity(label);
        }
        let activity_ms = last_activity(label)
            .map(|activity| activity.load(Ordering::SeqCst))
            .unwrap_or(0);
        if !should_collect(
            visible,
            !keep_visible,
            activity_ms,
            activity_clock_ms(),
            idle_grace_seconds.saturating_mul(1000),
        ) {
            continue;
        }
        if let Err(err) = window.destroy() {
            crate::warn!("[auxiliary-windows] Failed to destroy '{label}': {err}");
            continue;
        }
        let started = Instant::now();
        while app.get_webview_window(label).is_some() {
            if started.elapsed() >= Duration::from_millis(800) {
                crate::warn!("[auxiliary-windows] Timed out releasing '{label}'");
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{append_instance_query, should_collect, AuxiliaryInstance, Ordering};

    #[test]
    fn only_the_current_instance_acknowledgement_allows_showing() {
        let instance = AuxiliaryInstance::new();
        instance.generation.store(11, Ordering::SeqCst);
        assert!(!instance.mark_ready(10));
        assert!(!instance.ready(11));
        assert!(instance.mark_ready(11));
        assert!(instance.ready(11));

        instance.ready_generation.store(0, Ordering::SeqCst);
        instance.generation.store(12, Ordering::SeqCst);
        assert!(!instance.mark_ready(11));
        assert!(!instance.ready(12));
        assert!(instance.mark_ready(12));
        assert!(instance.ready(12));
    }

    #[test]
    fn instance_query_preserves_the_window_route_and_fragment() {
        assert_eq!(
            append_instance_query("index.html?window=quick-paste#search", 42),
            "index.html?window=quick-paste&instance=42#search"
        );
        assert_eq!(append_instance_query("index.html", 43), "index.html?instance=43");
    }

    #[test]
    fn a_recently_hidden_overlay_keeps_its_full_idle_grace() {
        assert!(!should_collect(false, false, 9_000, 10_000, 60_000));
        assert!(!should_collect(false, false, 9_000, 68_999, 60_000));
        assert!(!should_collect(true, false, 9_000, 100_000, 60_000));
    }

    #[test]
    fn an_expired_or_untracked_hidden_overlay_can_be_collected() {
        assert!(should_collect(false, false, 9_000, 69_000, 60_000));
        assert!(should_collect(false, false, 0, 10_000, 60_000));
    }

    #[test]
    fn an_explicit_gpu_restart_bypasses_visibility_and_idle_grace() {
        assert!(should_collect(false, true, 9_000, 10_000, 60_000));
        assert!(should_collect(true, true, 9_000, 10_000, 60_000));
    }
}
