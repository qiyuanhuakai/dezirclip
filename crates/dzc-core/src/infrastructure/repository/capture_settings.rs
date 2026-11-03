use super::clipboard_repo::SqliteClipboardRepository;
use crate::{api::{CoreError, CoreResult}, capture::CaptureSettings};
use std::collections::HashMap;

impl SqliteClipboardRepository {
    pub(crate) fn desktop_preferences(&self) -> CoreResult<HashMap<String, String>> {
        let conn = self.conn.lock().map_err(|_| CoreError::validation("SettingsUnavailable"))?;
        let mut query = conn.prepare("SELECT key, value FROM settings WHERE key IN (
            'app.arrow_key_selection','app.capture_files','app.capture_rich_text','app.classification_enabled',
            'app.clipboard_item_font_size','app.clipboard_tag_font_size','app.color_mode','app.compact_mode',
            'app.custom_background','app.custom_background_opacity','app.custom_themes','app.deduplicate',
            'app.delete_after_paste','app.disable_webview_gpu','app.edge_docking','app.emoji_favorites',
            'app.emoji_panel_enabled','app.emoji_panel_tab','app.excluded_source_apps','app.follow_mouse',
            'app.font_main','app.font_mono','app.hidden_filter_chips','app.hide_tray_icon','app.hotkey',
            'app.idle_destroy_enabled','app.idle_destroy_seconds','app.language','app.move_to_top_after_paste',
            'app.ocr_enabled','app.paste_method','app.persistent','app.persistent_limit','app.persistent_limit_enabled',
            'app.preview_keyboard_enabled','app.preview_opacity','app.privacy_protection',
            'app.privacy_protection_custom_rules','app.privacy_protection_kinds','app.privacy_session_hotkey',
            'app.quick_paste_enabled','app.quick_paste_hotkey','app.quick_paste_renderer','app.rich_paste_hotkey',
            'app.rich_text_snapshot_cache_limit','app.rich_text_snapshot_preview','app.screenshot_enabled',
            'app.screenshot_full_hotkey','app.screenshot_hotkey','app.search_fuzzy_enabled','app.search_hotkey',
            'app.search_regex_enabled','app.sequential_hotkey','app.sequential_mode','app.show_app_border',
            'app.show_scroll_top_button','app.show_search_box','app.silent_start','app.sound_enabled',
            'app.sound_paste_enabled','app.sound_volume','app.surface_opacity','app.tag_manager_enabled',
            'app.tag_manager_size','app.theme','app.update_auto_check','app.update_channel','app.use_win_v_shortcut',
            'app.window_pinned')").map_err(|error| CoreError::from(error.to_string()))?;
        let rows = query.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|error| CoreError::from(error.to_string()))?;
        rows.collect::<rusqlite::Result<_>>().map_err(|error| CoreError::from(error.to_string()))
    }
    pub(crate) fn screen_preferences(&self) -> CoreResult<(bool, String, String)> {
        let conn = self.conn.lock().map_err(|_| CoreError::from("SettingsUnavailable".to_owned()))?;
        conn.query_row("SELECT COALESCE((SELECT value FROM settings WHERE key='app.screenshot_enabled'),'true'),
            COALESCE((SELECT value FROM settings WHERE key='app.screenshot_hotkey'),'Ctrl+Shift+A'),
            COALESCE((SELECT value FROM settings WHERE key='app.screenshot_full_hotkey'),'Ctrl+Shift+F')", [],
            |row| Ok((row.get::<_, String>(0)? == "true", row.get(1)?, row.get(2)?)))
            .map_err(|error| CoreError::from(error.to_string()))
    }
    pub(crate) fn quick_preferences(&self) -> CoreResult<(bool, String)> {
        let conn = self.conn.lock().map_err(|_| CoreError::from("SettingsUnavailable".to_owned()))?;
        conn.query_row("SELECT COALESCE((SELECT value FROM settings WHERE key='app.quick_paste_enabled'),'true'),
            COALESCE((SELECT value FROM settings WHERE key='app.quick_paste_hotkey'),'Ctrl+Shift+V')", [],
            |row| Ok((row.get::<_, String>(0)? == "true", row.get(1)?)))
            .map_err(|error| CoreError::from(error.to_string()))
    }
    pub(crate) fn set_setting_at_epoch(&self, key: &str, value: &str, epoch: u64) -> CoreResult<()> {
        let database = |error: rusqlite::Error| CoreError::from(error.to_string());
        let mut connection = self.conn.lock().map_err(|_| CoreError::from("CaptureSettingsUnavailable".to_owned()))?;
        let transaction = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(database)?;
        let state = crate::api::privacy_session::snapshot_with_conn(&transaction, chrono::Utc::now().timestamp_millis())
            .map_err(|error| CoreError::from(error.to_string()))?;
        if state.privacy_epoch != epoch { return Err(CoreError::from("PrivacyEpochChanged".to_owned())); }
        super::settings_repo::SqliteSettingsRepository::write_value(&transaction, key, value).map_err(database)?;
        transaction.commit().map_err(database)
    }

    pub(crate) fn queue_preferences(&self) -> CoreResult<(bool, bool, String)> {
        let database = |error: rusqlite::Error| CoreError::from(error.to_string());
        let conn = self.conn.lock().map_err(|_| CoreError::from("QueueSettingsUnavailable".to_owned()))?;
        let mut query = conn.prepare("SELECT key,value FROM settings WHERE key IN ('app.sequential_mode','app.delete_after_paste','app.paste_method')").map_err(database)?;
        let values: HashMap<String,String> = query.query_map([], |row| Ok((row.get(0)?,row.get(1)?))).map_err(database)?
            .collect::<Result<_,_>>().map_err(database)?;
        Ok((values.get("app.sequential_mode").is_some_and(|s| s == "true"),
            values.get("app.delete_after_paste").is_some_and(|s| s == "true"),
            values.get("app.paste_method").cloned().unwrap_or_else(|| "shift_insert".into())))
    }

    pub(crate) fn reserved_shortcuts(&self) -> CoreResult<Vec<String>> {
        let connection = self.conn.lock().map_err(|_| CoreError::from("ShortcutSettingsUnavailable".to_owned()))?;
        let mut query = connection.prepare("SELECT value FROM settings WHERE key IN ('app.hotkey','app.quick_paste_hotkey','app.sequential_hotkey','app.rich_paste_hotkey','app.search_hotkey','app.screenshot_hotkey','app.screenshot_full_hotkey')")
            .map_err(|error| CoreError::from(error.to_string()))?;
        let values = query.query_map([], |row| row.get::<_,String>(0)).map_err(|error| CoreError::from(error.to_string()))?
            .collect::<Result<Vec<_>,_>>().map_err(|error| CoreError::from(error.to_string()))?;
        let chords: Vec<_> = values.iter().flat_map(|value| value.split(['\r', '\n'])).map(str::trim).filter(|chord| !chord.is_empty()).collect();
        if chords.iter().any(|chord| chord.len() > 128 || chord.chars().any(char::is_control)) { return Err(CoreError::validation("ShortcutSettingsInvalid")); }
        Ok(chords.into_iter().map(str::to_owned).collect())
    }
    pub(crate) fn capture_settings_snapshot(&self) -> CoreResult<CaptureSettings> {
        let connection = self.conn.lock().map_err(|_| CoreError::from("CaptureSettingsUnavailable".to_owned()))?;
        let mut query = connection.prepare("SELECT key,value FROM settings WHERE key IN ('app.persistent','app.deduplicate','app.capture_files','app.capture_rich_text','app.sequential_mode','app.privacy_protection','app.privacy_protection_kinds','app.privacy_protection_custom_rules','app.excluded_source_apps','app.ocr_enabled')").map_err(|error| CoreError::from(error.to_string()))?;
        let rows = query.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))).map_err(|error| CoreError::from(error.to_string()))?;
        let values: HashMap<String, String> = rows.collect::<Result<_, _>>().map_err(|error| CoreError::from(error.to_string()))?;
        if values.values().any(|value| value.len() > 64 * 1024) { return Err(CoreError::validation("CaptureSettingsTooLarge")); }
        let boolean = |key: &str, default| -> CoreResult<bool> {
            match values.get(key).map(String::as_str) {
                None => Ok(default), Some("true") => Ok(true), Some("false") => Ok(false),
                _ => Err(CoreError::validation("CaptureSettingsInvalid")),
            }
        };
        let value = |key: &str, default: &str| values.get(key).cloned().unwrap_or_else(|| default.to_owned());
        Ok(CaptureSettings {
            persistent: boolean("app.persistent", false)?, deduplicate: boolean("app.deduplicate", true)?,
            capture_files: boolean("app.capture_files", false)?, capture_rich_text: boolean("app.capture_rich_text", false)?,
            sequential_mode: boolean("app.sequential_mode", false)?,
            privacy_protection: boolean("app.privacy_protection", true)?, ocr_enabled: boolean("app.ocr_enabled", true)?,
            privacy_kinds: value("app.privacy_protection_kinds", "phone,idcard,email,secret,password").split(',').map(str::trim).filter(|item| !item.is_empty()).map(str::to_owned).collect(),
            privacy_custom_rules: value("app.privacy_protection_custom_rules", "").lines().map(str::trim).filter(|item| !item.is_empty()).map(str::to_owned).collect(),
            excluded_source_apps: crate::services::capture::source_exclusion::parse_excluded_source_apps(&value("app.excluded_source_apps", "")),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    #[test]
    fn desktop_preferences_return_ui_values_without_unregistered_settings() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        crate::infrastructure::repository::migrations::run_migrations(&mut connection).unwrap();
        connection.execute_batch("INSERT OR REPLACE INTO settings(key,value) VALUES
            ('app.language','tw'),('app.custom_themes','[]'),('app.unregistered','private'),('internal.host','private')")
            .unwrap();
        let repository = SqliteClipboardRepository::new(Arc::new(Mutex::new(connection)));
        let values = repository.desktop_preferences().unwrap();
        assert_eq!(values.get("app.language").map(String::as_str), Some("tw"));
        assert_eq!(values.get("app.custom_themes").map(String::as_str), Some("[]"));
        assert!(!values.contains_key("app.unregistered"));
        assert!(!values.contains_key("internal.host"));
    }
    #[test]
    fn capture_policy_is_whitelisted_and_malformed_security_booleans_fail_closed() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        crate::infrastructure::repository::migrations::run_migrations(&mut connection).unwrap();
        let repository = SqliteClipboardRepository::new(Arc::new(Mutex::new(connection)));
        let initial = repository.capture_settings_snapshot().unwrap();
        assert!(initial.privacy_protection && initial.deduplicate);
        assert!(!initial.persistent);
        repository.conn.lock().unwrap().execute("INSERT INTO settings(key,value) VALUES('app.privacy_protection','not-a-boolean')",[]).unwrap();
        assert!(repository.capture_settings_snapshot().is_err());
    }
    #[test]
    fn shortcut_conflicts_read_only_the_existing_accelerator_keys() {
        let mut connection=rusqlite::Connection::open_in_memory().unwrap();
        crate::infrastructure::repository::migrations::run_migrations(&mut connection).unwrap();
        connection.execute("INSERT INTO settings(key,value) VALUES('app.sequential_hotkey','Alt+V'),('app.hotkey','Ctrl+Shift+V'),('app.private_credential','not-an-accelerator')",[]).unwrap();
        let repository=SqliteClipboardRepository::new(Arc::new(Mutex::new(connection)));
        let mut values=repository.reserved_shortcuts().unwrap();values.sort();
        assert_eq!(values,vec!["Alt+V","Ctrl+Shift+V"]);
        repository.conn.lock().unwrap().execute("UPDATE settings SET value='Win+V'||char(13)||char(10)||'Ctrl+V' WHERE key='app.hotkey'",[]).unwrap();
        let mut chords = repository.reserved_shortcuts().unwrap(); chords.sort();
        assert_eq!(chords, vec!["Alt+V","Ctrl+V","Win+V"]);
        repository.conn.lock().unwrap().execute("UPDATE settings SET value='Ctrl+'||char(0)||'V' WHERE key='app.hotkey'",[]).unwrap();
        assert!(repository.reserved_shortcuts().is_err());
    }
}
