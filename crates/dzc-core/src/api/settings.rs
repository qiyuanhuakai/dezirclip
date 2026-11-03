use super::{CoreResult, HistoryService};
use crate::capture::CaptureSettings;

impl HistoryService {
    pub fn desktop_preferences(&self) -> CoreResult<std::collections::HashMap<String, String>> {
        self.repository.desktop_preferences()
    }
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

    pub fn set_window_pinned(&self, pinned: bool, epoch: u64) -> CoreResult<()> {
        self.repository.set_setting_at_epoch("app.window_pinned", if pinned { "true" } else { "false" }, epoch)
    }

    pub fn is_view_setting(key: &str) -> bool {
        matches!(key, "app.theme" | "app.color_mode" | "app.language" | "app.compact_mode" | "app.show_app_border"
            | "app.show_scroll_top_button" | "app.show_search_box" | "app.arrow_key_selection"
            | "app.clipboard_item_font_size" | "app.clipboard_tag_font_size" | "app.surface_opacity")
    }

    pub fn update_view_setting(&self, key: &str, value: &str, epoch: u64) -> CoreResult<()> {
        use crate::domain::theme::{valid_theme_id, BUILTIN_THEMES};
        let valid = match key {
            "app.theme" => BUILTIN_THEMES.contains(&value) || value.strip_prefix("custom:").is_some_and(valid_theme_id),
            "app.color_mode" => matches!(value, "light" | "dark" | "system"),
            "app.language" => matches!(value, "zh" | "tw" | "en"),
            "app.clipboard_item_font_size" => value.parse::<u8>().is_ok_and(|v| (11..=18).contains(&v)),
            "app.clipboard_tag_font_size" => value.parse::<u8>().is_ok_and(|v| (8..=14).contains(&v)),
            "app.surface_opacity" => value.parse::<u8>().is_ok_and(|v| v <= 100),
            _ => Self::is_view_setting(key) && matches!(value, "true" | "false"),
        };
        if !valid { return Err(super::CoreError::validation("ViewSettingInvalid")); }
        self.repository.set_setting_at_epoch(key, value, epoch)
    }

    pub fn is_capture_setting(key: &str) -> bool {
        matches!(key, "app.persistent" | "app.deduplicate" | "app.capture_files" | "app.capture_rich_text"
            | "app.privacy_protection" | "app.privacy_protection_kinds" | "app.privacy_protection_custom_rules"
            | "app.excluded_source_apps" | "app.ocr_enabled")
    }

    pub fn update_capture_setting(&self, key: &str, value: &str, epoch: u64) -> CoreResult<()> {
        use super::CoreError;
        if !Self::is_capture_setting(key) || value.len() > 64 * 1024 {
            return Err(CoreError::validation("CaptureSettingsInvalid"));
        }
        let list = matches!(key, "app.privacy_protection_kinds" | "app.privacy_protection_custom_rules" | "app.excluded_source_apps");
        if !list && !matches!(value, "true" | "false") {
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
