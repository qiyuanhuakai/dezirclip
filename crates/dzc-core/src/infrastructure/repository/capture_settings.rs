use super::clipboard_repo::SqliteClipboardRepository;
use crate::{api::{CoreError, CoreResult}, capture::CaptureSettings};
use std::collections::HashMap;

impl SqliteClipboardRepository {
    pub(crate) fn capture_settings_snapshot(&self) -> CoreResult<CaptureSettings> {
        let connection = self.conn.lock().map_err(|_| CoreError::from("CaptureSettingsUnavailable".to_owned()))?;
        let mut query = connection.prepare("SELECT key,value FROM settings WHERE key IN ('app.persistent','app.deduplicate','app.capture_files','app.capture_rich_text','app.privacy_protection','app.privacy_protection_kinds','app.privacy_protection_custom_rules','app.excluded_source_apps','app.ocr_enabled')").map_err(|error| CoreError::from(error.to_string()))?;
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
}
