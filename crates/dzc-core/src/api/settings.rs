use super::{CoreResult, HistoryService};
use crate::capture::CaptureSettings;

impl HistoryService {
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
        self.repository.set_capture_setting(key, value, epoch)
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
