use super::{CoreResult, HistoryService};
use crate::capture::CaptureSettings;

impl HistoryService {
    pub fn default_application(&self, kind: &str) -> CoreResult<Option<String>> {
        self.repository.default_application(kind)
    }
    pub fn feedback_volume(&self, paste: bool) -> CoreResult<u8> {
        self.repository.feedback_volume(paste)
    }
    pub fn desktop_preferences(&self) -> CoreResult<std::collections::HashMap<String, String>> {
        self.repository.desktop_preferences()
    }
    pub fn selected_background(&self) -> CoreResult<String> { self.repository.selected_background() }
    pub fn screen_preferences(&self) -> CoreResult<(bool, String, String)> {
        self.repository.screen_preferences()
    }
    pub fn quick_preferences(&self) -> CoreResult<(bool, String)> {
        self.repository.quick_preferences()
    }
    pub fn queue_preferences(&self) -> CoreResult<(bool, bool, String)> { self.repository.queue_preferences() }

    pub fn update_queue_setting(&self, key: &str, value: &str, epoch: u64) -> CoreResult<()> {
        let valid = match key {
            "app.sequential_mode" | "app.delete_after_paste" => matches!(value, "true" | "false"),
            "app.paste_method" => matches!(value, "shift_insert" | "ctrl_v" | "game_mode"),
            _ => false,
        };
        if !valid { return Err(super::CoreError::validation("QueueSettingsInvalid")); }
        self.repository.set_setting_at_epoch(key, value, epoch)
    }

    pub fn set_window_size(&self, width: u32, height: u32, epoch: u64) -> CoreResult<()> {
        if !(200..=32768).contains(&width) || !(200..=32768).contains(&height) {
            return Err(super::CoreError::validation("WindowSizeInvalid"));
        }
        self.repository.set_settings_at_epoch(&[
            ("app.window_width", &width.to_string()), ("app.window_height", &height.to_string()),
        ], epoch)
    }

    pub fn set_window_pinned(&self, pinned: bool, epoch: u64) -> CoreResult<()> {
        self.repository.set_setting_at_epoch("app.window_pinned", if pinned { "true" } else { "false" }, epoch)
    }

    pub fn is_view_setting(key: &str) -> bool {
        matches!(key, "app.theme" | "app.color_mode" | "app.language" | "app.compact_mode" | "app.show_app_border"
            | "app.show_scroll_top_button" | "app.show_search_box" | "app.tag_manager_enabled" | "app.rich_text_snapshot_preview" | "app.rich_text_snapshot_cache_limit" | "app.arrow_key_selection" | "app.preview_keyboard_enabled" | "app.silent_start" | "app.hide_tray_icon" | "app.edge_docking" | "app.follow_mouse" | "app.disable_webview_gpu"
            | "app.sound_enabled" | "app.sound_paste_enabled" | "app.sound_volume"
            | "app.clipboard_item_font_size" | "app.clipboard_tag_font_size" | "app.surface_opacity" | "app.preview_opacity" | "app.font_main" | "app.font_mono"
            | "app.custom_background" | "app.custom_background_opacity" | "app.emoji_panel_enabled" | "app.emoji_panel_tab" | "app.emoji_favorites")
    }

    pub fn update_view_setting(&self, key: &str, value: &str, epoch: u64) -> CoreResult<()> {
        use crate::domain::theme::{valid_theme_id, BUILTIN_THEMES};
        let valid = match key {
            "app.theme" => BUILTIN_THEMES.contains(&value) || value.strip_prefix("custom:").is_some_and(valid_theme_id),
            "app.font_main" | "app.font_mono" => value.chars().count() <= 256 && !value.chars().any(char::is_control),
            "app.color_mode" => matches!(value, "light" | "dark" | "system"),
            "app.emoji_favorites" => value.len() <= 128 * 1024 && serde_json::from_str::<Vec<String>>(value)
                .is_ok_and(|paths| paths.len() <= 4096 && paths.iter().all(|path| !path.is_empty() && !path.contains('\0'))),
            "app.emoji_panel_tab" => matches!(value, "emoji" | "favorites"),
            "app.language" => matches!(value, "zh" | "tw" | "en"),
            "app.clipboard_item_font_size" => value.parse::<u8>().is_ok_and(|v| (11..=18).contains(&v)),
            "app.rich_text_snapshot_cache_limit" => value.bytes().all(|b| b.is_ascii_digit())
                && value.parse::<u16>().is_ok_and(|v| v <= 500),
            "app.clipboard_tag_font_size" => value.parse::<u8>().is_ok_and(|v| (8..=14).contains(&v)),
            "app.custom_background" => value.is_empty() || (value.len() <= 32_768 && !value.contains('\0') && std::path::Path::new(value).is_absolute()),
            "app.sound_volume" | "app.custom_background_opacity" | "app.surface_opacity" | "app.preview_opacity" => value.parse::<u8>().is_ok_and(|v| v <= 100),
            _ => Self::is_view_setting(key) && matches!(value, "true" | "false"),
        };
        if !valid { return Err(super::CoreError::validation("ViewSettingInvalid")); }
        self.repository.set_setting_at_epoch(key, value, epoch)
    }

    pub fn set_default_application(&self, key: &str, application: &str, epoch: u64) -> CoreResult<()> {
        if !matches!(key, "app.text" | "app.rich_text" | "app.image" | "app.video" | "app.code" | "app.url")
            || application.len() > 32768 || application.contains('\0') {
            return Err(super::CoreError::validation("InvalidDefaultApplication"));
        }
        self.repository.set_setting_at_epoch(key, application, epoch)
    }

    pub fn is_capture_setting(key: &str) -> bool {
        matches!(key, "app.persistent" | "app.persistent_limit_enabled" | "app.persistent_limit" | "app.deduplicate" | "app.capture_files" | "app.capture_rich_text"
            | "app.privacy_protection" | "app.privacy_protection_kinds" | "app.privacy_protection_custom_rules"
            | "app.excluded_source_apps" | "app.ocr_enabled")
    }

    pub fn update_capture_setting(&self, key: &str, value: &str, epoch: u64) -> CoreResult<()> {
        use super::CoreError;
        if !Self::is_capture_setting(key) || value.len() > 64 * 1024 {
            return Err(CoreError::validation("CaptureSettingsInvalid"));
        }
        let valid = match key {
            "app.persistent_limit" => value.bytes().all(|c| c.is_ascii_digit())
                && value.parse::<u32>().is_ok_and(|n| (50..=99999).contains(&n)),
            "app.privacy_protection_kinds" | "app.privacy_protection_custom_rules" | "app.excluded_source_apps" => true,
            _ => matches!(value, "true" | "false"),
        };
        if !valid {
            return Err(CoreError::validation("CaptureSettingsInvalid"));
        }
        self.repository.set_setting_at_epoch(key, value, epoch)
    }

    /// Existing desktop accelerators reserved by this profile. This fixed list
    /// is used by the trusted Host before registering the privacy shortcut.
    pub fn reserved_shortcuts(&self) -> CoreResult<Vec<String>> {
        self.repository.reserved_shortcuts()
    }
    /// A fixed read-only capture policy snapshot, never arbitrary settings IPC.
    pub fn capture_settings_snapshot(&self) -> CoreResult<CaptureSettings> {
        self.repository.capture_settings_snapshot()
    }
}

#[cfg(test)]
mod tests {
    use super::HistoryService;
    #[test]
    fn storage_limit_validates_bounds_and_epoch() {
        let history = HistoryService::open(std::path::Path::new(":memory:")).unwrap();
        let epoch = history.runtime_snapshot(0).unwrap().privacy_epoch;
        let save = |value| history.update_capture_setting("app.persistent_limit", value, epoch);
        for value in ["50", "99999"] {
            save(value).unwrap();
        }
        for value in ["49", "100000", "-50", "", "5e2"] {
            assert!(save(value).is_err());
        }
        assert!(history.update_capture_setting("app.persistent_limit_enabled", "1", epoch).is_err());
        assert!(history.update_capture_setting("app.persistent_limit", "75", epoch + 1).is_err());
    }

    #[test]
    fn snapshot_cache_matches_shared_view_bounds_and_rejects_stale_writes() {
        let history = HistoryService::open(std::path::Path::new(":memory:")).unwrap();
        let epoch = history.runtime_snapshot(0).unwrap().privacy_epoch;
        let key = "app.rich_text_snapshot_cache_limit";
        for valid in ["0", "120", "500"] {
            history.update_view_setting(key, valid, epoch).unwrap();
        }
        for invalid in ["", "-1", "501", "0.5", "true"] {
            assert!(history.update_view_setting(key, invalid, epoch).is_err());
        }
        assert!(history.update_view_setting(key, "0", epoch + 1).is_err());
        assert_eq!(history.desktop_preferences().unwrap().get(key).map(String::as_str), Some("500"));
    }

    #[test]
    fn feedback_respects_master_switch_paste_switch_and_volume() {
        let history = HistoryService::open(std::path::Path::new(":memory:")).unwrap();
        let epoch = history.runtime_snapshot(0).unwrap().privacy_epoch;
        let save = |key, value| history.update_view_setting(key, value, epoch);
        assert_eq!(history.feedback_volume(false).unwrap(), 0);
        save("app.sound_enabled", "true").unwrap();
        assert_eq!(history.feedback_volume(true).unwrap(), 70);
        save("app.sound_volume", "26").unwrap();
        save("app.sound_paste_enabled", "false").unwrap();
        assert_eq!(history.feedback_volume(false).unwrap(), 26);
        assert_eq!(history.feedback_volume(true).unwrap(), 0);
        assert!(save("app.sound_volume", "101").is_err());
        assert!(save("app.sound_paste_enabled", "1").is_err());
        save("app.sound_volume", "0").unwrap();
        assert_eq!(history.feedback_volume(false).unwrap(), 0);
    }
}
