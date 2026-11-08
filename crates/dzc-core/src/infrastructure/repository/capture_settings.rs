use super::clipboard_repo::SqliteClipboardRepository;
use crate::{api::{CoreError, CoreResult}, capture::CaptureSettings};
use std::collections::HashMap;

#[cfg(test)]
mod shortcut_tests {
    use super::SqliteClipboardRepository;
    use std::sync::{Arc, Mutex};

    #[test]
    fn reserved_shortcuts_include_active_defaults_but_respect_explicit_empty_values() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)").unwrap();
        let connection = Arc::new(Mutex::new(connection));
        let repository = SqliteClipboardRepository::new(connection.clone());
        let mut values = repository.reserved_shortcuts().unwrap();
        values.sort();
        assert_eq!(values, ["Ctrl+Shift+A", "Ctrl+Shift+F", "Ctrl+Shift+V"]);
        connection.lock().unwrap().execute_batch("INSERT INTO settings VALUES
            ('app.quick_paste_hotkey',''), ('app.screenshot_hotkey','Ctrl+Alt+A'),
            ('app.hotkey','Alt+C\nCtrl+Alt+C')").unwrap();
        let mut values = repository.reserved_shortcuts().unwrap();
        values.sort();
        assert_eq!(values, ["Alt+C", "Ctrl+Alt+A", "Ctrl+Alt+C", "Ctrl+Shift+F"]);
    }
}

impl SqliteClipboardRepository {
    pub(crate) fn feedback_volume(&self, paste: bool) -> CoreResult<u8> {
        let conn = self.conn.lock().map_err(|_| CoreError::validation("SettingsUnavailable"))?;
        conn.query_row("SELECT CASE WHEN
            (SELECT value FROM settings WHERE key='app.sound_enabled')='true' AND
            (NOT ?1 OR COALESCE((SELECT value FROM settings WHERE key='app.sound_paste_enabled'),'true')<>'false')
            THEN COALESCE((SELECT value FROM settings WHERE key='app.sound_volume'),'70') ELSE '0' END",
            [paste], |row| row.get::<_, String>(0))
            .map(|v| v.parse::<u8>().unwrap_or(70).min(100))
            .map_err(|error| CoreError::from(error.to_string()))
    }
    pub(crate) fn desktop_preferences(&self) -> CoreResult<HashMap<String, String>> {
        let conn = self.conn.lock().map_err(|_| CoreError::validation("SettingsUnavailable"))?;
        let mut query = conn.prepare("SELECT key, value FROM settings WHERE key IN (
            'app.text','app.rich_text','app.image','app.video','app.code','app.url',
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
            'app.window_width','app.window_height','app.window_pinned')").map_err(|error| CoreError::from(error.to_string()))?;
        let rows = query.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|error| CoreError::from(error.to_string()))?;
        rows.collect::<rusqlite::Result<_>>().map_err(|error| CoreError::from(error.to_string()))
    }
    pub(crate) fn default_application(&self, kind: &str) -> CoreResult<Option<String>> {
        use rusqlite::OptionalExtension;
        let key = match kind {
            "text" => "app.text", "image" => "app.image", "code" => "app.code",
            "video" => "app.video", "rich_text" => "app.rich_text", "link" | "url" => "app.url",
            _ => return Ok(None),
        };
        let fallback = if kind == "code" { "app.text" } else { key };
        let conn = self.conn.lock().map_err(|_| CoreError::validation("SettingsUnavailable"))?;
        conn.query_row("SELECT value FROM settings WHERE key IN (?1,?2) AND trim(value) <> ''
            ORDER BY (key = ?1) DESC LIMIT 1", [key, fallback], |row| row.get(0))
            .optional().map_err(|error| CoreError::from(error.to_string()))
    }

    pub(crate) fn selected_background(&self) -> CoreResult<String> {
        let connection = self.conn.lock().map_err(|_| CoreError::validation("SettingsUnavailable"))?;
        connection.query_row("SELECT COALESCE((SELECT value FROM settings WHERE key='app.custom_background'),'')",
            [], |row| row.get(0)).map_err(|error| CoreError::from(error.to_string()))
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
        self.set_settings_at_epoch(&[(key, value)], epoch)
    }
    pub(crate) fn reset_application_preferences(&self, epoch: u64) -> CoreResult<()> {
        let database = |error: rusqlite::Error| CoreError::from(error.to_string());
        let mut connection = self.conn.lock().map_err(|_| CoreError::from("SettingsUnavailable".to_owned()))?;
        let transaction = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(database)?;
        let state = crate::api::privacy_session::snapshot_with_conn(&transaction, chrono::Utc::now().timestamp_millis())
            .map_err(|error| CoreError::from(error.to_string()))?;
        if state.privacy_epoch != epoch { return Err(CoreError::from("PrivacyEpochChanged".to_owned())); }
        // The Host stops native shortcut registrations before this transaction.
        // Identity and internal state are not application preferences.
        transaction.execute("DELETE FROM settings WHERE substr(key,1,4)='app.' AND key <> 'app.anon_id'", []).map_err(database)?;
        crate::database::seed_preference_defaults(&transaction).map_err(database)?;
        super::sensitive_tags::ensure_system_tags(&transaction).map_err(database)?;
        transaction.commit().map_err(database)
    }
    pub(crate) fn set_settings_at_epoch(&self, settings: &[(&str, &str)], epoch: u64) -> CoreResult<()> {
        let database = |error: rusqlite::Error| CoreError::from(error.to_string());
        let mut connection = self.conn.lock().map_err(|_| CoreError::from("CaptureSettingsUnavailable".to_owned()))?;
        let transaction = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(database)?;
        let state = crate::api::privacy_session::snapshot_with_conn(&transaction, chrono::Utc::now().timestamp_millis())
            .map_err(|error| CoreError::from(error.to_string()))?;
        if state.privacy_epoch != epoch { return Err(CoreError::from("PrivacyEpochChanged".to_owned())); }
        for &(key, value) in settings {
            super::settings_repo::SqliteSettingsRepository::write_value(&transaction, key, value).map_err(database)?;
        }
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
        let mut query = connection.prepare("SELECT key,value FROM settings WHERE key IN ('app.hotkey','app.quick_paste_hotkey','app.sequential_hotkey','app.rich_paste_hotkey','app.search_hotkey','app.screenshot_hotkey','app.screenshot_full_hotkey')")
            .map_err(|error| CoreError::from(error.to_string()))?;
        let mut values = query.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .map_err(|error| CoreError::from(error.to_string()))?
            .collect::<Result<HashMap<_, _>, _>>().map_err(|error| CoreError::from(error.to_string()))?;
        // These defaults are active even before a standalone profile saves any settings.
        for (key, fallback) in [("app.quick_paste_hotkey", "Ctrl+Shift+V"),
            ("app.screenshot_hotkey", "Ctrl+Shift+A"), ("app.screenshot_full_hotkey", "Ctrl+Shift+F")] {
            values.entry(key.into()).or_insert_with(|| fallback.into());
        }
        let chords: Vec<_> = values.values().flat_map(|value| value.split(['\r', '\n'])).map(str::trim).filter(|chord| !chord.is_empty()).collect();
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
    fn reset_preferences_is_atomic_and_preserves_host_identity_and_privacy() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        crate::infrastructure::repository::migrations::run_migrations(&mut connection).unwrap();
        connection.execute_batch("INSERT OR REPLACE INTO settings(key,value) VALUES
            ('app.theme','retro'), ('app.hotkey','Ctrl+Shift+F8'), ('app.custom_themes','[]'),
            ('app.anon_id','stable-identity'), ('app.privacy_session_hotkey','Ctrl+Alt+P'), ('internal.fixture','keep');
            UPDATE privacy_runtime_state SET session_enabled=1,session_expires_at_ms=NULL;
            CREATE TRIGGER reject_default_font BEFORE INSERT ON settings
            WHEN NEW.key='app.font_main' BEGIN SELECT RAISE(ABORT,'rejected default'); END;").unwrap();
        let before = crate::api::privacy_session::snapshot_with_conn(&connection, 0).unwrap();
        let shared = Arc::new(Mutex::new(connection));
        let repository = SqliteClipboardRepository::new(shared.clone());
        let values = || {
            let conn = shared.lock().unwrap();
            let mut query = conn.prepare("SELECT key,value FROM settings ORDER BY key").unwrap();
            query.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))).unwrap()
                .collect::<Result<Vec<_>, _>>().unwrap()
        };
        let original = values();
        assert!(repository.reset_application_preferences(before.privacy_epoch).is_err());
        assert_eq!(values(), original, "a failed default insert must roll back the delete and earlier inserts");
        assert_eq!(crate::api::privacy_session::snapshot_with_conn(&shared.lock().unwrap(), 0).unwrap().privacy_epoch, before.privacy_epoch);
        shared.lock().unwrap().execute_batch("DROP TRIGGER reject_default_font").unwrap();
        assert!(repository.reset_application_preferences(before.privacy_epoch + 1).is_err());
        assert_eq!(values(), original, "an obsolete renderer must not reset preferences");
        repository.reset_application_preferences(before.privacy_epoch).unwrap();
        let settings: HashMap<_, _> = values().into_iter().collect();
        assert_eq!(settings["app.theme"], "mica");
        assert_eq!(settings["app.hotkey"], "Alt+C");
        assert!(!settings.contains_key("app.custom_themes"));
        assert_eq!(settings["app.anon_id"], "stable-identity");
        assert!(!settings.contains_key("app.privacy_session_hotkey"));
        assert_eq!(settings["internal.fixture"], "keep");
        let after = crate::api::privacy_session::snapshot_with_conn(&shared.lock().unwrap(), 0).unwrap();
        assert!(after.privacy_epoch > before.privacy_epoch, "resetting privacy preferences invalidates old permits");
        assert_eq!(after.session, before.session);
    }

    #[test]
    fn rejected_height_rolls_back_width_in_the_same_transaction() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        crate::infrastructure::repository::migrations::run_migrations(&mut connection).unwrap();
        connection.execute_batch("INSERT OR REPLACE INTO settings(key,value) VALUES
            ('app.window_width','660'), ('app.window_height','620');
            CREATE TRIGGER reject_new_height BEFORE INSERT ON settings
            WHEN NEW.key='app.window_height' BEGIN SELECT RAISE(ABORT,'rejected height'); END;").unwrap();
        let epoch = crate::api::privacy_session::snapshot_with_conn(&connection, 0).unwrap().privacy_epoch;
        let repository = SqliteClipboardRepository::new(Arc::new(Mutex::new(connection)));
        assert!(repository.set_settings_at_epoch(&[("app.window_width","690"), ("app.window_height","590")], epoch).is_err());
        let values = repository.desktop_preferences().unwrap();
        assert_eq!(values["app.window_width"], "660");
        assert_eq!(values["app.window_height"], "620");
    }
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
        assert_eq!(values,vec!["Alt+V","Ctrl+Shift+A","Ctrl+Shift+F","Ctrl+Shift+V","Ctrl+Shift+V"]);
        repository.conn.lock().unwrap().execute("UPDATE settings SET value='Win+V'||char(13)||char(10)||'Ctrl+V' WHERE key='app.hotkey'",[]).unwrap();
        let mut chords = repository.reserved_shortcuts().unwrap(); chords.sort();
        assert_eq!(chords, vec!["Alt+V","Ctrl+Shift+A","Ctrl+Shift+F","Ctrl+Shift+V","Ctrl+V","Win+V"]);
        repository.conn.lock().unwrap().execute("UPDATE settings SET value='Ctrl+'||char(0)||'V' WHERE key='app.hotkey'",[]).unwrap();
        assert!(repository.reserved_shortcuts().is_err());
    }
}
