use crate::app::commands::hotkey_cmd::sync_hotkeys_from_settings;
use crate::app_state::SettingsState;
use dzc_core::api::HistoryService;
use crate::error::{AppError, AppResult};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, Manager, State};

async fn persist_host_setting(host: &crate::app::ipc_host::DesktopHost, key: &str, value: String) -> AppResult<()> {
    use dzc_host::wire::{Method, OperationResult, RuntimeState};
    let state: RuntimeState = host.call(Method::Health, serde_json::json!({}), "RuntimeState").await.map_err(AppError::Validation)?;
    let method = if dzc_core::api::HistoryService::is_view_setting(key) { Method::DesktopSettingsViewSet } else { Method::DesktopSettingsCaptureSet };
    let _: OperationResult = host.call(method, serde_json::json!({
        "service_instance_id": state.service_instance_id, "expected_epoch": state.privacy_epoch, "key": key, "value": value
    }), "OperationResult").await.map_err(AppError::Validation)?;
    Ok(())
}

async fn persist_queue_setting(app: &AppHandle, key: &str, value: String) -> AppResult<()> {
    use dzc_host::wire::{Method, OperationResult, QueueSettingParams, RuntimeState};
    let host = app.state::<crate::app::ipc_host::DesktopHost>();
    let state: RuntimeState = host.call(Method::Health, serde_json::json!({}), "RuntimeState").await.map_err(AppError::Validation)?;
    let _: OperationResult = host.call(Method::DesktopQueueConfigure, QueueSettingParams {
        service_instance_id: state.service_instance_id, expected_epoch: state.privacy_epoch, key:key.into(), value,
    }, "OperationResult").await.map_err(AppError::Validation)?;
    Ok(())
}

fn setting_error(error: dzc_core::api::CoreError) -> AppError {
    AppError::Database(error.to_string())
}

fn save_bool_setting(db_state: &HistoryService, key: &str, enabled: bool) -> AppResult<()> {
    db_state
.set_desktop_setting(key, &enabled.to_string())
        .map_err(setting_error)
}

fn apply_setting_state_update(
    app_handle: &AppHandle,
    settings_state: &crate::app_state::SettingsState,
    key: &str,
    value: &str,
) {
    match key {
        "app.arrow_key_selection" => {
            settings_state
                .arrow_key_selection
                .store(value == "true", Ordering::Relaxed);
        }
        "app.sequential_mode" => {
            settings_state
                .sequential_mode
                .store(value == "true", Ordering::Relaxed);
        }
        "app.sound_enabled" => {
            settings_state
                .sound_enabled
                .store(value == "true", Ordering::Relaxed);
        }
        "app.persistent" => {
            settings_state
                .persistent
                .store(value != "false", Ordering::Relaxed);
        }
        "app.capture_files" => {
            settings_state
                .capture_files
                .store(value != "false", Ordering::Relaxed);
        }
        "app.capture_rich_text" => {
            settings_state
                .capture_rich_text
                .store(value == "true", Ordering::Relaxed);
        }
        "app.silent_start" => {
            settings_state
                .silent_start
                .store(value != "false", Ordering::Relaxed);
        }
        "app.delete_after_paste" => {
            settings_state
                .delete_after_paste
                .store(value == "true", Ordering::Relaxed);
        }
        "app.privacy_protection" => {
            settings_state
                .privacy_protection
                .store(value == "true", Ordering::Relaxed);
        }
        "app.excluded_source_apps" => {
            if let Ok(mut rules) = settings_state.excluded_source_apps.lock() {
                *rules = crate::services::source_exclusion::parse_excluded_source_apps(value);
            }
        }
        "app.edge_docking" => {
            settings_state
                .edge_docking
                .store(value == "true", Ordering::Relaxed);
        }
        "app.follow_mouse" => {
            settings_state
                .follow_mouse
                .store(value != "false", Ordering::Relaxed);
        }
        "app.hide_tray_icon" => {
            settings_state
                .hide_tray_icon
                .store(value == "true", Ordering::Relaxed);
        }
        "app.idle_destroy_enabled" => {
            settings_state
                .idle_destroy_enabled
                .store(value == "true", Ordering::Relaxed);
        }
        "app.idle_destroy_seconds" => {
            if let Ok(secs) = value.parse::<u64>() {
                settings_state.idle_destroy_seconds.store(
                    crate::app::idle_destroyer::clamp_idle_seconds(secs),
                    Ordering::Relaxed,
                );
            }
        }
        "app.screenshot_enabled" => {
            settings_state
                .screenshot_enabled
                .store(value != "false", Ordering::Relaxed);
            let _ = crate::app::commands::hotkey_cmd::sync_hotkeys_from_settings(app_handle);
        }
        "app.screenshot_hotkey" => {
            if let Ok(mut guard) = settings_state.screenshot_hotkey.lock() {
                *guard = value.to_string();
            }
            let _ = crate::app::commands::hotkey_cmd::sync_hotkeys_from_settings(app_handle);
        }
        "app.screenshot_full_hotkey" => {
            if let Ok(mut guard) = settings_state.screenshot_full_hotkey.lock() {
                *guard = value.to_string();
            }
            let _ = crate::app::commands::hotkey_cmd::sync_hotkeys_from_settings(app_handle);
        }
        "app.quick_paste_enabled" => {
            settings_state
                .quick_paste_enabled
                .store(value != "false", Ordering::Relaxed);
            let _ = crate::app::commands::hotkey_cmd::sync_hotkeys_from_settings(app_handle);
        }
        "app.quick_paste_hotkey" => {
            if let Ok(mut guard) = settings_state.quick_paste_hotkey.lock() {
                *guard = value.to_string();
            }
            let _ = crate::app::commands::hotkey_cmd::sync_hotkeys_from_settings(app_handle);
        }
        "app.ocr_enabled" => {
            settings_state
                .ocr_enabled
                .store(value != "false", Ordering::Relaxed);
        }
        "app.disable_webview_gpu" => {
            let enabled = value == "true";
            let app = app_handle.clone();
            std::thread::spawn(move || {
                if !crate::app::idle_destroyer::restart_main_window_for_gpu_switch(&app, enabled) {
                    crate::warn!(
                        "[settings] disable_webview_gpu set to {}; WebView restart is still pending",
                        enabled
                    );
                }
            });
        }
        "app.search_fuzzy_enabled" | "app.search_regex_enabled" => {}
        _ => {}
    }
}

#[tauri::command]
pub async fn set_sequential_mode(app: AppHandle, state: State<'_, SettingsState>, enabled: bool) -> AppResult<()> {
    persist_queue_setting(&app, "app.sequential_mode", enabled.to_string()).await?;
    state.sequential_mode.store(enabled, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub fn set_sequential_hotkey(
    app_handle: AppHandle,
    state: State<'_, SettingsState>,
    hotkey: String,
) -> AppResult<()> {
    if let Ok(mut guard) = state.sequential_paste_hotkey.lock() {
        *guard = hotkey.clone();
    }

    let db_state = app_handle.state::<HistoryService>();
    db_state
.set_desktop_setting("app.sequential_hotkey", &hotkey)
        .map_err(setting_error)?;
    sync_hotkeys_from_settings(&app_handle)
}

#[tauri::command]
pub fn set_rich_paste_hotkey(
    app_handle: AppHandle,
    state: State<'_, SettingsState>,
    hotkey: String,
) -> AppResult<()> {
    if let Ok(mut guard) = state.rich_paste_hotkey.lock() {
        *guard = hotkey.clone();
    }

    let db_state = app_handle.state::<HistoryService>();
    db_state
.set_desktop_setting("app.rich_paste_hotkey", &hotkey)
        .map_err(setting_error)?;
    sync_hotkeys_from_settings(&app_handle)
}

#[tauri::command]
pub fn set_search_hotkey(
    app_handle: AppHandle,
    state: State<'_, SettingsState>,
    hotkey: String,
) -> AppResult<()> {
    if let Ok(mut guard) = state.search_hotkey.lock() {
        *guard = hotkey.clone();
    }

    let db_state = app_handle.state::<HistoryService>();
    db_state
.set_desktop_setting("app.search_hotkey", &hotkey)
        .map_err(setting_error)?;
    sync_hotkeys_from_settings(&app_handle)
}

#[tauri::command]
pub async fn set_deduplication(host: State<'_, crate::app::ipc_host::DesktopHost>, state: State<'_, SettingsState>, enabled: bool) -> AppResult<()> {
    persist_host_setting(&host, "app.deduplicate", enabled.to_string()).await?;
    state.deduplicate.store(enabled, Ordering::Relaxed);
    Ok(())
}

/// Settings that any webview re-renders itself from. Every window is a separate
/// webview with a separate document, so none of them can observe what another
/// one applied in memory; the database is the only shared source of truth.
///
/// Theme and colour mode are deliberately absent: `set_theme` broadcasts those on
/// `theme-changed` with a payload, so a theme switch needs no database read.
fn is_appearance_override_key(key: &str) -> bool {
    matches!(
        key,
        "app.font_main"
            | "app.font_mono"
            | "app.surface_opacity"
            | "app.clipboard_item_font_size"
            | "app.clipboard_tag_font_size"
            | "app.show_app_border"
            | "app.compact_mode"
            // Not an appearance setting, but it has the same shape of problem
            // and no other route: the rich-text snapshot cache limit is held by
            // a module-level Map in each webview, so the compact preview would
            // otherwise keep its own copy pinned at the default and ignore the
            // setting entirely. This channel is what makes it re-read.
            | "app.rich_text_snapshot_cache_limit"
    )
}

#[tauri::command]
pub async fn save_setting(
    app_handle: AppHandle,
    db_state: State<'_, HistoryService>,
    settings_state: State<'_, crate::app_state::SettingsState>,
    key: String,
    value: String,
) -> AppResult<()> {
    if key=="app.quick_paste_layout" && !matches!(value.as_str(),"list"|"ring") {return Err(AppError::Validation("InvalidQuickPasteLayout".into()));}
    if matches!(key.as_str(), "app.quick_paste_full_sectors" | "app.quick_paste_half_sectors") {
        let limit = dzc_core::api::HistoryService::quick_paste_sector_limit(key == "app.quick_paste_half_sectors");
        if !value.parse::<u8>().is_ok_and(|count| (3..=limit).contains(&count)) {
            return Err(AppError::Validation("InvalidQuickPasteSectorCount".into()));
        }
    }
    if key=="app.quick_paste_renderer" && !matches!(value.as_str(),"tauri"|"mygo") {return Err(AppError::Validation("InvalidQuickPasteRenderer".into()));}
    if key=="app.quick_paste_renderer" || key=="app.quick_paste_enabled" && value=="false" {
        crate::app::surface_supervisor::stop_blocking(&app_handle,"disabled",true).map_err(AppError::Validation)?;
        if let Some(window)=app_handle.get_webview_window("quick-paste") {window.hide()?;}
    }
    if matches!(key.as_str(), "app.sequential_mode" | "app.delete_after_paste" | "app.move_to_top_after_paste" | "app.paste_method") {
        persist_queue_setting(&app_handle, &key, value.clone()).await?;
    } else if dzc_core::api::HistoryService::is_capture_setting(&key) || dzc_core::api::HistoryService::is_view_setting(&key) {
        persist_host_setting(&app_handle.state::<crate::app::ipc_host::DesktopHost>(), &key, value.clone()).await?;
    } else {
        db_state.set_desktop_setting(&key, &value).map_err(setting_error)?;
    }
    apply_setting_state_update(&app_handle, &settings_state, &key, &value);
    if dzc_core::api::HistoryService::is_update_preference(&key) {
        app_handle.state::<crate::app::updater::UpdaterState>().preferences_changed();
    }
    if key == "app.quick_paste_renderer" && value == "mygo" {
        crate::app::auxiliary_windows::release_unused_quick_paste(app_handle.clone());
    }
    if key == "app.custom_background" {
        let _ = app_handle.emit("custom-background-changed", &value);
    }
    if key == "app.privacy_protection" {
        let _ = app_handle.emit("clipboard-changed", ());
    }
    if is_appearance_override_key(&key) {
        let _ = app_handle.emit("appearance-changed", key);
    }
    Ok(())
}

#[tauri::command]
pub fn set_ignore_blur(ignore: bool) {
    crate::IGNORE_BLUR.store(ignore, Ordering::Relaxed);
}

#[tauri::command]
pub fn set_window_pinned(app_handle: AppHandle, state: State<'_, HistoryService>, pinned: bool) -> AppResult<()> {
    super::capability_cmd::require_desktop_interaction().map_err(AppError::Validation)?;
    crate::WINDOW_PINNED.store(pinned, Ordering::Relaxed);
    if let Some(window) = app_handle.get_webview_window("main") {
        let _ = window.set_always_on_top(pinned);
        let _ = window.set_focusable(false);
        #[cfg(windows)]
        {
            use windows::Win32::Foundation::HWND;
            use windows::Win32::UI::WindowsAndMessaging::{
                GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE,
            };
            if let Ok(hwnd) = window.hwnd() {
                unsafe {
                    let ex_style = GetWindowLongPtrW(HWND(hwnd.0), GWL_EXSTYLE);
                    let _ = SetWindowLongPtrW(
                        HWND(hwnd.0),
                        GWL_EXSTYLE,
                        ex_style | WS_EX_NOACTIVATE.0 as isize,
                    );
                }
            }
        }
    }
    let _ = state
.set_desktop_setting("app.window_pinned", &pinned.to_string());
    Ok(())
}

#[tauri::command]
pub fn get_settings(
    state: State<'_, HistoryService>,
) -> AppResult<std::collections::HashMap<String, String>> {
    state.desktop_settings_snapshot().map_err(setting_error)
}

#[tauri::command]
pub fn set_arrow_key_selection(
    state: State<'_, crate::app_state::SettingsState>,
    enabled: bool,
) -> AppResult<()> {
    state.arrow_key_selection.store(enabled, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub async fn set_persistence(host: State<'_, crate::app::ipc_host::DesktopHost>, state: State<'_, SettingsState>, enabled: bool) -> AppResult<()> {
    persist_host_setting(&host, "app.persistent", enabled.to_string()).await?;
    state.persistent.store(enabled, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub async fn set_capture_files(host: State<'_, crate::app::ipc_host::DesktopHost>, state: State<'_, SettingsState>, enabled: bool) -> AppResult<()> {
    persist_host_setting(&host, "app.capture_files", enabled.to_string()).await?;
    state.capture_files.store(enabled, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub async fn set_capture_rich_text(host: State<'_, crate::app::ipc_host::DesktopHost>, state: State<'_, SettingsState>, enabled: bool) -> AppResult<()> {
    persist_host_setting(&host, "app.capture_rich_text", enabled.to_string()).await?;
    state.capture_rich_text.store(enabled, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub fn set_silent_start(
    state: State<'_, crate::app_state::SettingsState>,
    db_state: State<'_, HistoryService>,
    enabled: bool,
) -> AppResult<()> {
    state.silent_start.store(enabled, Ordering::Relaxed);
    save_bool_setting(&db_state, "app.silent_start", enabled)
}

#[tauri::command]
pub async fn set_delete_after_paste(app: AppHandle, state: State<'_, SettingsState>, enabled: bool) -> AppResult<()> {
    persist_queue_setting(&app, "app.delete_after_paste", enabled.to_string()).await?;
    state.delete_after_paste.store(enabled, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub async fn set_privacy_protection(
    app: AppHandle,
    state: State<'_, crate::app_state::SettingsState>,
    enabled: bool,
) -> AppResult<()> {
    persist_host_setting(&app.state::<crate::app::ipc_host::DesktopHost>(), "app.privacy_protection", enabled.to_string()).await?;
    state.privacy_protection.store(enabled, Ordering::Relaxed);
    crate::app::ipc_host::notify_privacy_change(&app).await.map_err(AppError::Validation)?;
    let _ = app.emit("clipboard-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn set_privacy_protection_kinds(
    app:AppHandle,
    state: State<'_, crate::app_state::SettingsState>,
    kinds: Vec<String>,
) -> AppResult<()> {
    let serialized = kinds.join(",");
    persist_host_setting(&app.state::<crate::app::ipc_host::DesktopHost>(), "app.privacy_protection_kinds", serialized).await?;
    *state.privacy_protection_kinds.lock().unwrap()=kinds;
    crate::app::ipc_host::notify_privacy_change(&app).await.map_err(AppError::Validation)?;
    Ok(())
}

#[tauri::command]
pub async fn set_privacy_protection_custom_rules(
    app:AppHandle,
    state: State<'_, crate::app_state::SettingsState>,
    rules: String,
) -> AppResult<()> {
    let list = rules
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    persist_host_setting(&app.state::<crate::app::ipc_host::DesktopHost>(), "app.privacy_protection_custom_rules", rules).await?;
    *state.privacy_protection_custom_rules.lock().unwrap()=list;
    crate::app::ipc_host::notify_privacy_change(&app).await.map_err(AppError::Validation)?;
    Ok(())
}

#[tauri::command]
pub fn set_sound_enabled(
    state: State<'_, crate::app_state::SettingsState>,
    db_state: State<'_, HistoryService>,
    enabled: bool,
) -> AppResult<()> {
    state.sound_enabled.store(enabled, Ordering::Relaxed);
    save_bool_setting(&db_state, "app.sound_enabled", enabled)
}

async fn reset_host_preferences(host: &crate::app::ipc_host::DesktopHost) -> AppResult<()> {
    use dzc_host::wire::{LibraryStateParams, Method, OperationResult, RuntimeState};

    let state: RuntimeState = host
        .call(Method::Health, serde_json::json!({}), "RuntimeState")
        .await
        .map_err(AppError::Validation)?;
    let result: OperationResult = host
        .blocking_call(
            Method::DesktopSettingsReset,
            LibraryStateParams {
                service_instance_id: state.service_instance_id,
                expected_epoch: state.privacy_epoch,
            },
            "OperationResult",
        )
        .await
        .map_err(AppError::Validation)?;
    if !result.performed {
        return Err(AppError::Validation("SettingsResetUnconfirmed".into()));
    }
    Ok(())
}

#[tauri::command]
pub async fn reset_settings(
    app: AppHandle,
    host: State<'_, crate::app::ipc_host::DesktopHost>,
    history: State<'_, HistoryService>,
    settings: State<'_, SettingsState>,
) -> AppResult<()> {
    reset_host_preferences(&host).await?;

    let preferences = history.desktop_startup_preferences();
    settings.apply_preferences(&preferences);
    apply_tray_visibility(&app, !preferences.hide_tray_icon);
    let previously_pinned = crate::WINDOW_PINNED.swap(preferences.window_pinned, Ordering::Relaxed);
    if preferences.window_pinned != previously_pinned {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.set_always_on_top(preferences.window_pinned);
        }
    }
    app.state::<crate::app::updater::UpdaterState>().preferences_changed();
    sync_hotkeys_from_settings(&app)
}

fn apply_tray_visibility(_app: &AppHandle, visible: bool) {
    #[cfg(target_os = "linux")]
    crate::infrastructure::linux_api::tray::set_tray_visible(visible);
    #[cfg(not(target_os = "linux"))]
    if let Some(tray) = _app.tray_by_id("main_tray") {
        let _ = tray.set_visible(visible);
    }
}

#[tauri::command]
pub fn set_tray_visible(
    app_handle: AppHandle,
    state: State<'_, crate::app_state::SettingsState>,
    visible: bool,
) -> AppResult<()> {
    state.hide_tray_icon.store(!visible, Ordering::Relaxed);
    apply_tray_visibility(&app_handle, visible);
    let db_state = app_handle.state::<HistoryService>();
    save_bool_setting(&db_state, "app.hide_tray_icon", !visible)
}

#[tauri::command]
pub fn set_edge_docking(
    app_handle: AppHandle,
    state: State<'_, crate::app_state::SettingsState>,
    enabled: bool,
) -> AppResult<()> {
    if enabled { super::capability_cmd::require_desktop_interaction().map_err(AppError::Validation)?; }
    state.edge_docking.store(enabled, Ordering::Relaxed);
    let db_state = app_handle.state::<HistoryService>();
    save_bool_setting(&db_state, "app.edge_docking", enabled)
}

#[tauri::command]
pub fn set_follow_mouse(
    app_handle: AppHandle,
    state: State<'_, crate::app_state::SettingsState>,
    enabled: bool,
) -> AppResult<()> {
    if enabled { super::capability_cmd::require_desktop_interaction().map_err(AppError::Validation)?; }
    state.follow_mouse.store(enabled, Ordering::Relaxed);
    let db_state = app_handle.state::<HistoryService>();
    save_bool_setting(&db_state, "app.follow_mouse", enabled)
}

#[tauri::command]
pub fn set_idle_destroy_enabled(
    state: State<'_, crate::app_state::SettingsState>,
    db_state: State<'_, HistoryService>,
    enabled: bool,
) -> AppResult<()> {
    state.idle_destroy_enabled.store(enabled, Ordering::Relaxed);
    save_bool_setting(&db_state, "app.idle_destroy_enabled", enabled)
}

#[tauri::command]
pub fn set_idle_destroy_seconds(
    state: State<'_, crate::app_state::SettingsState>,
    db_state: State<'_, HistoryService>,
    seconds: u64,
) -> AppResult<()> {
    let clamped = crate::app::idle_destroyer::clamp_idle_seconds(seconds);
    state.idle_destroy_seconds.store(clamped, Ordering::Relaxed);
    db_state
.set_desktop_setting("app.idle_destroy_seconds", &clamped.to_string())
        .map_err(setting_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn tauri_resets_preferences_through_the_host_and_refreshes_runtime_state() {
        use dzc_core::api::{ClipboardEntry, ImportMode, SessionHistoryStore};
        use dzc_host::wire::{EventsResult, Method, OperationResult, QueueReplaceParams, RuntimeState, SubscribeParams};
        use std::{collections::VecDeque, path::Path, sync::Arc};

        let history = HistoryService::open_desktop(Path::new(":memory:")).unwrap();
        let entry: ClipboardEntry = serde_json::from_value(serde_json::json!({
            "id": 1, "content_type": "text", "content": "reset fixture", "preview": "reset fixture",
            "timestamp": 1, "source_app": "test", "is_pinned": false
        })).unwrap();
        history.restore(&[entry], ImportMode::Merge).unwrap();
        for (key, value) in [
            ("app.theme", "retro"), ("app.hotkey", "Ctrl+Shift+F8"),
            ("app.sound_enabled", "true"), ("app.hide_tray_icon", "true"),
            ("app.anon_id", "existing-identity"), ("internal.fixture", "retained"),
        ] {
            history.set_desktop_setting(key, value).unwrap();
        }
        let state = SettingsState::default();
        state.apply_preferences(&history.desktop_startup_preferences());
        let host = crate::app::ipc_host::DesktopHost::synthetic_fixture(
            history.clone(),
            Arc::new(SessionHistoryStore::new(VecDeque::new())),
        );
        let before: RuntimeState = host.call(Method::Health, serde_json::json!({}), "RuntimeState").await.unwrap();
        let queued: OperationResult = host.call(
            Method::DesktopQueueSet,
            QueueReplaceParams {
                service_instance_id: before.service_instance_id.clone(),
                expected_epoch: before.privacy_epoch,
                ids: vec![1],
            },
            "OperationResult",
        ).await.unwrap();
        assert!(queued.performed);
        assert!(state.sound_enabled.load(Ordering::Relaxed));
        assert!(state.hide_tray_icon.load(Ordering::Relaxed));

        reset_host_preferences(&host).await.unwrap();
        state.apply_preferences(&history.desktop_startup_preferences());
        assert!(!state.sound_enabled.load(Ordering::Relaxed));
        assert!(!state.hide_tray_icon.load(Ordering::Relaxed));
        assert_eq!(*state.theme.lock().unwrap(), "mica");
        assert_eq!(*state.main_hotkey.lock().unwrap(), "Alt+C");
        assert_eq!(history.desktop_setting("app.anon_id").unwrap().as_deref(), Some("existing-identity"));
        assert_eq!(history.desktop_setting("internal.fixture").unwrap().as_deref(), Some("retained"));
        assert_eq!(history.get_entry_by_id(1).unwrap().unwrap().content, "reset fixture");

        let events: EventsResult = host.call(
            Method::EventsSubscribe,
            SubscribeParams { after_sequence: before.event_sequence, wait_ms: Some(0), settings_only: None },
            "EventsResult",
        ).await.unwrap();
        assert!(events.state.privacy_epoch > before.privacy_epoch);
        for status in ["paste_queue_changed:0", "application_settings_reset"] {
            assert!(events.events.iter().any(|event| event.data.status.as_deref() == Some(status)), "{status}");
        }
    }

    #[test]
    fn appearance_keys_are_broadcast() {
        assert!(is_appearance_override_key("app.font_main"));
        assert!(is_appearance_override_key("app.font_mono"));
        assert!(is_appearance_override_key("app.surface_opacity"));
        assert!(is_appearance_override_key("app.clipboard_item_font_size"));
        assert!(is_appearance_override_key("app.clipboard_tag_font_size"));
        assert!(is_appearance_override_key("app.show_app_border"));
        assert!(is_appearance_override_key("app.compact_mode"));
    }

    #[test]
    fn theme_keys_are_not_broadcast_here() {
        // `set_theme` already broadcasts these on `theme-changed` with a payload.
        // Emitting them again here would make every window do a needless database
        // read on each theme switch.
        assert!(!is_appearance_override_key("app.theme"));
        assert!(!is_appearance_override_key("app.color_mode"));
    }

    #[test]
    fn unrelated_keys_are_not_broadcast() {
        // Waking every auxiliary webview on a hotkey or storage-limit change
        // would cost a settings reload per keystroke for no visual effect.
        assert!(!is_appearance_override_key("app.hotkey"));
        assert!(!is_appearance_override_key("app.custom_background_opacity"));
    }

    #[test]
    fn similar_prefixes_do_not_match() {
        assert!(!is_appearance_override_key("app.font_main_backup"));
        assert!(!is_appearance_override_key("app.surface_opacity_scale"));
    }
}
