use super::clipboard_repo::SqliteClipboardRepository;
use crate::{api::{AutomaticUpdatePreference, CoreError, CoreResult, QuickPastePresentation, SurfaceAppearancePreferences, UpdateAttemptChannel}, capture::CaptureSettings};
use super::settings_repo::SqliteSettingsRepository;
use dzc_protocol::settings::{QUICK_PASTE_LAYOUT, UPDATE_AUTO_CHECK, UPDATE_CHANNEL};
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
            'app.quick_paste_enabled','app.quick_paste_hotkey','app.quick_paste_renderer',?3,'app.quick_paste_full_sectors','app.quick_paste_half_sectors','app.rich_paste_hotkey',
            'app.rich_text_snapshot_cache_limit','app.rich_text_snapshot_preview','app.screenshot_enabled',
            'app.screenshot_full_hotkey','app.screenshot_hotkey','app.search_fuzzy_enabled','app.search_hotkey',
            'app.search_regex_enabled','app.sequential_hotkey','app.sequential_mode','app.show_app_border',
            'app.show_scroll_top_button','app.show_search_box','app.silent_start','app.sound_enabled',
            'app.sound_paste_enabled','app.sound_volume','app.surface_opacity','app.tag_manager_enabled',
            'app.tag_manager_size','app.theme',?1,?2,'app.use_win_v_shortcut',
            'app.window_width','app.window_height','app.window_pinned')").map_err(|error| CoreError::from(error.to_string()))?;
        let rows = query.query_map([UPDATE_AUTO_CHECK.key, UPDATE_CHANNEL.key, QUICK_PASTE_LAYOUT.key], |row| Ok((row.get(0)?, row.get(1)?)))
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
    pub(crate) fn quick_preferences(&self) -> CoreResult<(bool, String, String, u8, u8)> {
        let conn = self.conn.lock().map_err(|_| CoreError::from("SettingsUnavailable".to_owned()))?;
        conn.query_row("SELECT COALESCE((SELECT value FROM settings WHERE key='app.quick_paste_enabled'),'true'),
            COALESCE((SELECT value FROM settings WHERE key='app.quick_paste_hotkey'),'Ctrl+Shift+V'),
            COALESCE((SELECT value FROM settings WHERE key=?1),'list'),
            (SELECT value FROM settings WHERE key='app.quick_paste_full_sectors'),
            (SELECT value FROM settings WHERE key='app.quick_paste_half_sectors')", [QUICK_PASTE_LAYOUT.key],
            |row| {
                let full: Option<String> = row.get(3)?;
                let half: Option<String> = row.get(4)?;
                use crate::api::HistoryService;
                Ok((row.get::<_, String>(0)? == "true", row.get(1)?, row.get(2)?,
                    HistoryService::quick_paste_sectors(full.as_deref(), false),
                    HistoryService::quick_paste_sectors(half.as_deref(), true)))
            })
            .map_err(|error| CoreError::from(error.to_string()))
    }
    pub(crate) fn surface_appearance_preferences(&self) -> CoreResult<SurfaceAppearancePreferences> {
        use crate::domain::theme::{parse_theme_library, BUILTIN_THEMES};
        let conn = self.conn.lock().map_err(|_| CoreError::validation("SettingsUnavailable"))?;
        let (locale, color, opacity, selected, custom): (Option<String>, Option<String>, Option<String>, String, Option<String>) =
            conn.query_row("WITH theme AS (SELECT COALESCE((SELECT value FROM settings WHERE key='app.theme'), '') AS selected)
                SELECT (SELECT value FROM settings WHERE key='app.language'),
                       (SELECT value FROM settings WHERE key='app.color_mode'),
                       (SELECT value FROM settings WHERE key='app.surface_opacity'),
                       theme.selected,
                       CASE WHEN substr(theme.selected, 1, 7) = 'custom:' THEN (SELECT value FROM settings WHERE key='app.custom_themes') END
                FROM theme", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)))
                .map_err(|error| CoreError::from(error.to_string()))?;
        drop(conn);
        let defaults = SurfaceAppearancePreferences::default();
        let base = if let Some(id) = selected.strip_prefix("custom:") {
            parse_theme_library(custom.as_deref()).unwrap_or_default()
                .into_iter().find(|theme| theme.id == id).map(|theme| theme.base).unwrap_or_default()
        } else {
            selected
        };
        Ok(SurfaceAppearancePreferences {
            locale: locale.filter(|value| value.len() <= 32).unwrap_or(defaults.locale),
            dark_mode: match color.as_deref() { Some("dark") => Some(true), Some("light") => Some(false), _ => None },
            opacity: opacity.and_then(|value| value.parse::<f64>().ok()).filter(|value| value.is_finite())
                .map(|value| value.clamp(0.0, 100.0) / 100.0).unwrap_or(defaults.opacity),
            base_theme: if BUILTIN_THEMES.contains(&base.as_str()) { base } else { defaults.base_theme },
        })
    }

    pub(crate) fn quick_paste_presentation(&self) -> CoreResult<QuickPastePresentation> {
        let conn = self.conn.lock().map_err(|_| CoreError::validation("SettingsUnavailable"))?;
        conn.query_row("SELECT (SELECT value FROM settings WHERE key=?1),
            (SELECT value FROM settings WHERE key='app.quick_paste_full_sectors'),
            (SELECT value FROM settings WHERE key='app.quick_paste_half_sectors')", [QUICK_PASTE_LAYOUT.key], |row| {
            let value = |column| row.get::<_, Option<String>>(column).ok().flatten();
            Ok(QuickPastePresentation::from_settings(value(0).as_deref(), value(1).as_deref(), value(2).as_deref()))
        }).map_err(|error| CoreError::from(error.to_string()))
    }
    pub(crate) fn uses_external_quick_paste(&self) -> CoreResult<bool> {
        let conn = self.conn.lock().map_err(|_| CoreError::validation("SettingsUnavailable"))?;
        conn.query_row("SELECT EXISTS(SELECT 1 FROM settings WHERE key='app.quick_paste_renderer' AND value='mygo')",
            [], |row| row.get(0)).map_err(|error| CoreError::from(error.to_string()))
    }
    pub(crate) fn automatic_update_preference(&self) -> CoreResult<Option<AutomaticUpdatePreference>> {
        let conn = self.conn.lock().map_err(|_| CoreError::validation("SettingsUnavailable"))?;
        let enabled = SqliteSettingsRepository::get_raw(&conn, UPDATE_AUTO_CHECK.key)
            .map_err(|error| CoreError::from(error.to_string()))?;
        Ok((enabled.as_deref() == Some("true")).then(|| AutomaticUpdatePreference {
            saved_channel: SqliteSettingsRepository::get_raw(&conn, UPDATE_CHANNEL.key).ok().flatten(),
        }))
    }
    pub(crate) fn last_update_attempt(&self, channel: UpdateAttemptChannel) -> CoreResult<Option<String>> {
        let conn = self.conn.lock().map_err(|_| CoreError::validation("SettingsUnavailable"))?;
        SqliteSettingsRepository::get_raw(&conn, channel.storage_key()).map_err(|error| CoreError::from(error.to_string()))
    }
    pub(crate) fn record_update_attempt(&self, channel: UpdateAttemptChannel, when_ms: i64) -> CoreResult<()> {
        let database = |error: rusqlite::Error| CoreError::from(error.to_string());
        let mut conn = self.conn.lock().map_err(|_| CoreError::validation("SettingsUnavailable"))?;
        let transaction = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(database)?;
        SqliteSettingsRepository::write_value(&transaction, channel.storage_key(), &when_ms.to_string()).map_err(database)?;
        transaction.commit().map_err(database)
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

    #[cfg(any(feature = "legacy-bridge", test))]
    pub(crate) fn reset_desktop_preferences(&self, anonymous_id: &str) -> CoreResult<()> {
        let database = |error: rusqlite::Error| CoreError::from(error.to_string());
        let mut connection = self.conn.lock().map_err(|_| CoreError::validation("SettingsUnavailable"))?;
        let transaction = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(database)?;
        transaction.execute("DELETE FROM settings", []).map_err(database)?;
        crate::database::seed_defaults(&transaction).map_err(database)?;
        SqliteSettingsRepository::write_value(&transaction, "app.anon_id", anonymous_id).map_err(database)?;
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
    fn desktop_reset_rolls_back_defaults_when_replacing_identity_fails() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        crate::infrastructure::repository::migrations::run_migrations(&mut connection).unwrap();
        connection.execute_batch("INSERT OR REPLACE INTO settings(key,value) VALUES
            ('app.theme','retro'), ('app.anon_id','old-identity'), ('internal.fixture','before-reset');
            CREATE TRIGGER reject_anonymous_identity BEFORE INSERT ON settings
            WHEN NEW.key='app.anon_id' BEGIN SELECT RAISE(ABORT,'identity rejected'); END;").unwrap();
        let shared = Arc::new(Mutex::new(connection));
        let repository = SqliteClipboardRepository::new(shared.clone());
        let value = |key| SqliteSettingsRepository::get_raw(&shared.lock().unwrap(), key).unwrap();
        assert!(repository.reset_desktop_preferences("new-identity").is_err());
        assert_eq!(value("app.theme").as_deref(), Some("retro"));
        assert_eq!(value("app.anon_id").as_deref(), Some("old-identity"));
        assert_eq!(value("internal.fixture").as_deref(), Some("before-reset"));
        shared.lock().unwrap().execute_batch("DROP TRIGGER reject_anonymous_identity").unwrap();
        repository.reset_desktop_preferences("new-identity").unwrap();
        assert_eq!(value("app.theme").as_deref(), Some("mica"));
        assert_eq!(value("app.anon_id").as_deref(), Some("new-identity"));
        assert_eq!(value("internal.fixture"), None);
    }

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
    fn quick_paste_presentation_reads_layout_and_slots_from_the_same_profile() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        crate::infrastructure::repository::migrations::run_migrations(&mut connection).unwrap();
        let shared = Arc::new(Mutex::new(connection));
        let repository = SqliteClipboardRepository::new(shared.clone());
        let default = repository.quick_paste_presentation().unwrap();
        assert_eq!(default.layout(), "list");
        assert_eq!(default.entry_slots(false), None);
        assert!(!repository.uses_external_quick_paste().unwrap());
        shared.lock().unwrap().execute_batch("INSERT OR REPLACE INTO settings VALUES
            ('app.quick_paste_layout', 'ring'),
            ('app.quick_paste_full_sectors', '12'),
            ('app.quick_paste_half_sectors', 'invalid old value'),
            ('app.quick_paste_renderer', 'mygo');").unwrap();
        let saved = repository.quick_paste_presentation().unwrap();
        assert_eq!(saved.layout(), "ring");
        assert_eq!(saved.entry_slots(false), Some(12));
        assert_eq!(saved.entry_slots(true), Some(5));
        assert!(repository.uses_external_quick_paste().unwrap());
    }

    #[test]
    fn surface_appearance_reads_a_selected_custom_theme_and_recovers_invalid_legacy_values() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        crate::infrastructure::repository::migrations::run_migrations(&mut connection).unwrap();
        let custom = serde_json::json!([{
            "format": "dezirclip-theme", "version": 1, "id": "fixture", "name": "Fixture",
            "base": "retro", "css": ".theme-custom { --dc-text-primary: #202020; }"
        }]).to_string();
        connection.execute("INSERT OR REPLACE INTO settings VALUES
            ('app.theme','custom:fixture'), ('app.custom_themes',?1), ('app.language','tw'),
            ('app.color_mode','dark'), ('app.surface_opacity','75')", [custom]).unwrap();
        let shared = Arc::new(Mutex::new(connection));
        let repository = SqliteClipboardRepository::new(shared.clone());
        let saved = repository.surface_appearance_preferences().unwrap();
        assert_eq!((saved.locale.as_str(), saved.dark_mode, saved.opacity, saved.base_theme.as_str()),
            ("tw", Some(true), 0.75, "retro"));
        shared.lock().unwrap().execute("INSERT OR REPLACE INTO settings VALUES
            ('app.custom_themes','invalid'), ('app.color_mode','old-mode'), ('app.surface_opacity','NaN')", []).unwrap();
        let fallback = repository.surface_appearance_preferences().unwrap();
        assert_eq!(fallback.base_theme, crate::domain::theme::DEFAULT_BUILTIN_THEME);
        assert_eq!(fallback.dark_mode, None);
        assert_eq!(fallback.opacity, 0.65);
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
