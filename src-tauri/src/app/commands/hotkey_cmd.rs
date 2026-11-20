use crate::app_state::SettingsState;
use crate::error::{AppError, AppResult};
use crate::global_state::HOTKEY_STRING;
use dzc_core::ports::{Shortcut as PlatformShortcut, ShortcutProvider, ShortcutRegistration};
use dzc_platform::shortcut::{normalize_shortcut, NativeShortcutProvider};
use std::sync::{atomic::Ordering, Mutex};
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

pub(crate) fn parse_hotkey_list(raw: &str) -> Vec<String> {
    raw.split(['\n', '\r'])
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(|item| item.to_string())
        .collect()
}

// These keys have no mapping (or an incorrect mapping) in the plugin.
fn native_shortcut(canonical: &str) -> bool {
    let key = canonical.rsplit('+').next().unwrap_or_default();
    key == "ContextMenu"
        || cfg!(target_os = "linux")
            && matches!(
                key,
                "CapsLock" | "NumLock" | "NumpadEnter" | "NumpadEqual" | "MediaPlayPause"
            )
}

fn unavailable_on_windows(canonical: &str) -> bool {
    cfg!(target_os = "windows")
        && matches!(
            canonical.rsplit('+').next(),
            Some("F12" | "NumpadEnter" | "NumpadEqual")
        )
}

static NATIVE_SHORTCUTS: Mutex<Vec<Box<dyn ShortcutRegistration>>> = Mutex::new(Vec::new());

fn register_native(
    canonical: &str,
    pressed: impl FnMut() + Send + 'static,
) -> AppResult<Box<dyn ShortcutRegistration>> {
    static PROVIDER: std::sync::OnceLock<NativeShortcutProvider> = std::sync::OnceLock::new();
    PROVIDER
        .get_or_init(NativeShortcutProvider::default)
        .register(
            &PlatformShortcut {
                id: canonical.to_owned(),
                accelerator: canonical.to_owned(),
            },
            Box::new(pressed),
        )
        .map_err(|error| AppError::Validation(format!("Shortcut unavailable: {error:?}")))
}

pub(crate) fn desktop_shortcut(raw: &str) -> Option<Shortcut> {
    match normalize_shortcut(raw) {
        Ok(canonical) if native_shortcut(&canonical) || unavailable_on_windows(&canonical) => None,
        Ok(canonical) => canonical.parse().ok(),
        Err(_) => raw.replace("Win", "Super").parse().ok(),
    }
}

fn register_shortcut_if_valid(app: &AppHandle, raw_hotkey: &str) {
    if dzc_platform::runtime::is_native_wayland() {
        return;
    }
    if raw_hotkey.is_empty()
        || raw_hotkey.eq_ignore_ascii_case("MouseMiddle")
        || raw_hotkey.eq_ignore_ascii_case("MButton")
    {
        return;
    }
    if let Ok(canonical) = normalize_shortcut(raw_hotkey) {
        if native_shortcut(&canonical) {
            let app = app.clone();
            let dispatched = canonical.clone();
            match register_native(&canonical, move || {
                let dispatch_app = app.clone();
                let chord = dispatched.clone();
                let _ = app.run_on_main_thread(move || {
                    crate::app::setup::dispatch_global_shortcut(&dispatch_app, |raw| {
                        normalize_shortcut(raw).is_ok_and(|candidate| candidate == chord)
                    });
                });
            }) {
                Ok(registration) => NATIVE_SHORTCUTS.lock().unwrap().push(registration),
                Err(error) => crate::warn!(
                    "[global-shortcut] Failed to register '{}': {:?}",
                    canonical,
                    error
                ),
            }
            return;
        }
    }
    if let Some(shortcut) = desktop_shortcut(raw_hotkey) {
        match app.global_shortcut().register(shortcut.clone()) {
            Ok(_) => crate::info!("[global-shortcut] Registered: {}", raw_hotkey),
            Err(e) => crate::warn!(
                "[global-shortcut] Failed to register '{}': {:?}",
                raw_hotkey,
                e
            ),
        }
    } else {
        crate::warn!("[global-shortcut] Failed to parse hotkey: {}", raw_hotkey);
    }
}

pub fn sync_hotkeys_from_settings(app: &AppHandle) -> AppResult<()> {
    if dzc_platform::runtime::is_native_wayland() {
        return Ok(());
    }
    let settings = app
        .try_state::<SettingsState>()
        .ok_or_else(|| AppError::Internal("SettingsState 不可用".to_string()))?;

    let main_hotkey = settings.main_hotkey.lock().unwrap().clone();
    let sequential_hotkey = settings.sequential_paste_hotkey.lock().unwrap().clone();
    let rich_hotkey = settings.rich_paste_hotkey.lock().unwrap().clone();
    let search_hotkey = settings.search_hotkey.lock().unwrap().clone();
    let screenshot_hotkey = settings.screenshot_hotkey.lock().unwrap().clone();
    let screenshot_full_hotkey = settings.screenshot_full_hotkey.lock().unwrap().clone();
    let quick_paste_hotkey = settings.quick_paste_hotkey.lock().unwrap().clone();

    {
        let mut guard = HOTKEY_STRING.lock().unwrap();
        *guard = main_hotkey.clone();
    }

    let _ = app.global_shortcut().unregister_all();
    NATIVE_SHORTCUTS.lock().unwrap().clear();
    for item in parse_hotkey_list(&main_hotkey) {
        register_shortcut_if_valid(app, &item);
    }
    register_shortcut_if_valid(app, &sequential_hotkey);
    register_shortcut_if_valid(app, &rich_hotkey);
    register_shortcut_if_valid(app, &search_hotkey);

    // A feature that is switched off must not merely ignore its key when it
    // arrives -- it has to hand the key back. `unregister_all` above has
    // already dropped everything, so skipping the re-registration is what
    // releases the OS-level claim; the dispatch handler's own check in
    // `setup.rs` only stops the action, while the shortcut stays registered
    // and keeps the combination unavailable to every other application.
    // Quick paste has the same shape and was quietly doing this too.
    if settings.screenshot_enabled.load(Ordering::Relaxed) {
        register_shortcut_if_valid(app, &screenshot_hotkey);
        register_shortcut_if_valid(app, &screenshot_full_hotkey);
    }
    if settings.quick_paste_enabled.load(Ordering::Relaxed) {
        register_shortcut_if_valid(app, &quick_paste_hotkey);
    }
    Ok(())
}

#[tauri::command]
pub fn register_hotkey(app_handle: AppHandle, hotkey: String) -> AppResult<()> {
    super::capability_cmd::require_desktop_interaction().map_err(AppError::Validation)?;
    if let Some(settings) = app_handle.try_state::<SettingsState>() {
        let mut guard = settings.main_hotkey.lock().unwrap();
        *guard = hotkey.clone();
    }

    sync_hotkeys_from_settings(&app_handle)
}

#[tauri::command]
pub fn test_hotkey_available(app_handle: AppHandle, hotkey: String) -> AppResult<bool> {
    super::capability_cmd::require_desktop_interaction().map_err(AppError::Validation)?;
    if hotkey.is_empty()
        || hotkey.eq_ignore_ascii_case("MouseMiddle")
        || hotkey.eq_ignore_ascii_case("MButton")
    {
        return Ok(true);
    }

    if let Ok(canonical) = normalize_shortcut(&hotkey) {
        if native_shortcut(&canonical) {
            return register_native(&canonical, || {}).map(|_| true);
        }
    }
    let shortcut = desktop_shortcut(&hotkey)
        .ok_or_else(|| AppError::Validation("快捷键格式无效或不受当前系统支持".to_string()))?;

    match app_handle.global_shortcut().register(shortcut.clone()) {
        Ok(_) => {
            let _ = app_handle.global_shortcut().unregister(shortcut);
            Ok(true)
        }
        Err(e) => {
            let err_str = format!("{:?}", e);
            let user_msg = if err_str.contains("AlreadyRegistered") {
                "该快捷键已被其他程序占用".to_string()
            } else {
                "快捷键不可用".to_string()
            };
            Err(AppError::Internal(user_msg))
        }
    }
}
