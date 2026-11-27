use super::clipboard_repo::SqliteClipboardRepository;
use super::settings_repo::{SettingsRepository, SqliteSettingsRepository};
use crate::api::{CoreError, CoreResult, DesktopStartupPreferences};

impl SqliteClipboardRepository {
    /// Read preferences from one connection while preserving the desktop's
    /// existing fallback for missing, malformed or unreadable settings.
    pub(crate) fn desktop_startup_preferences(&self) -> DesktopStartupPreferences {
        let connection = self.conn.lock().ok();
        let get = |key| {
            connection
                .as_deref()
                .and_then(|conn| SqliteSettingsRepository::get_raw(conn, key).ok().flatten())
        };
        let value = |key, default: &str| get(key).unwrap_or_else(|| default.to_owned());
        let boolean = |key, default| get(key).map(|value| value == "true").unwrap_or(default);
        DesktopStartupPreferences {
            theme: value("app.theme", "retro"),
            persistent: boolean("app.persistent", true),
            capture_files: boolean("app.capture_files", true),
            capture_rich_text: boolean("app.capture_rich_text", false),
            deduplicate: boolean("app.deduplicate", true),
            silent_start: boolean("app.silent_start", true),
            delete_after_paste: boolean("app.delete_after_paste", false),
            privacy_protection: boolean("app.privacy_protection", true),
            privacy_kinds: value(
                "app.privacy_protection_kinds",
                "phone,idcard,email,secret,password",
            ),
            privacy_custom: value("app.privacy_protection_custom_rules", ""),
            excluded_source_apps: value("app.excluded_source_apps", ""),
            sequential_mode: boolean("app.sequential_mode", false),
            sequential_hotkey: value("app.sequential_hotkey", "Alt+V"),
            rich_paste_hotkey: value("app.rich_paste_hotkey", "Ctrl+Shift+Z"),
            search_hotkey: value("app.search_hotkey", "Alt+F"),
            screenshot_enabled: boolean("app.screenshot_enabled", true),
            screenshot_hotkey: value("app.screenshot_hotkey", "Ctrl+Shift+A"),
            screenshot_full_hotkey: value("app.screenshot_full_hotkey", "Ctrl+Shift+F"),
            quick_paste_enabled: boolean("app.quick_paste_enabled", true),
            quick_paste_hotkey: value("app.quick_paste_hotkey", "Ctrl+Shift+V"),
            ocr_enabled: boolean("app.ocr_enabled", true),
            sound_enabled: boolean("app.sound_enabled", false),
            hide_tray_icon: boolean("app.hide_tray_icon", false),
            edge_docking: boolean("app.edge_docking", false),
            follow_mouse: boolean("app.follow_mouse", true),
            window_pinned: boolean("app.window_pinned", false),
            main_hotkey: value("app.hotkey", "Win+V"),
            arrow_key_selection: boolean("app.arrow_key_selection", false),
            idle_destroy_enabled: boolean("app.idle_destroy_enabled", false),
            window_size: connection.as_deref().and_then(|conn| {
                SqliteSettingsRepository::read_window_size(conn)
                    .ok()
                    .flatten()
            }),
            idle_destroy_seconds: get("app.idle_destroy_seconds")
                .and_then(|value| value.parse::<u64>().ok()),
        }
    }

    pub(crate) fn saved_window_size(&self) -> CoreResult<Option<(u32, u32)>> {
        let connection = self
            .conn
            .lock()
            .map_err(|_| CoreError::validation("SettingsUnavailable"))?;
        SqliteSettingsRepository::read_window_size(&connection)
            .map_err(|error| CoreError::from(error.to_string()))
    }

    pub(crate) fn window_pinned(&self) -> CoreResult<bool> {
        let connection = self
            .conn
            .lock()
            .map_err(|_| CoreError::validation("SettingsUnavailable"))?;
        SqliteSettingsRepository::get_raw(&connection, "app.window_pinned")
            .map(|value| value.as_deref() == Some("true"))
            .map_err(|error| CoreError::from(error.to_string()))
    }

    pub(crate) fn reset_unelevated_game_mode(&self, is_elevated: impl FnOnce() -> bool) -> bool {
        let settings = SqliteSettingsRepository::new(self.conn.clone());
        if settings.get("app.paste_method").ok().flatten().as_deref() == Some("game_mode")
            && !is_elevated()
        {
            let _ = settings.set("app.paste_method", "shift_insert");
            return true;
        }
        false
    }
}
