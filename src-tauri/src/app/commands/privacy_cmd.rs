//! Trusted desktop requests use the same persistent state and opaque permissions as the Host.
use crate::app_state::SessionHistory;
use crate::database::DbState;
use crate::domain::models::ClipboardEntry;
use dzc_core::api::{PrivacyAccessIntent, PrivacySessionCommand, RuntimeSnapshot, SensitiveAccess};
use tauri::{AppHandle, Emitter, Manager};

fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn publish(app: &AppHandle, snapshot: RuntimeSnapshot) -> Result<RuntimeSnapshot, String> {
    dzc_platform::runtime::revoke_private_exports()
        .map_err(|_| "PrivateExportRevocationUnconfirmed".to_owned())?;
    let _ = app.emit("privacy-state-changed", &snapshot);
    Ok(snapshot)
}

#[tauri::command]
pub fn get_privacy_status(app: AppHandle) -> Result<RuntimeSnapshot, String> {
    app.state::<DbState>()
        .history_service()
        .runtime_snapshot(now())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn set_privacy_session(
    app: AppHandle,
    enabled: bool,
    duration_ms: Option<u64>,
) -> Result<RuntimeSnapshot, String> {
    let command = if enabled {
        PrivacySessionCommand::On { duration_ms }
    } else {
        if duration_ms.is_some() {
            return Err("PrivacyDurationInvalid".into());
        }
        PrivacySessionCommand::Off
    };
    let snapshot = app
        .state::<DbState>()
        .history_service()
        .privacy()
        .set_session(command, now())
        .map_err(|error| error.to_string())?;
    publish(&app, snapshot)
}

#[tauri::command]
pub fn toggle_privacy_session(app: AppHandle) -> Result<RuntimeSnapshot, String> {
    let history = app.state::<DbState>().history_service();
    let snapshot = history
        .privacy()
        .toggle(now())
        .map_err(|error| error.to_string())?;
    publish(&app, snapshot)
}

/// Invoked only by the local GUI's explicit reveal action. IPC roles do not grant this intent.
#[tauri::command]
pub fn get_privacy_entry(
    app: AppHandle,
    id: i64,
    expected_epoch: u64,
) -> Result<ClipboardEntry, String> {
    let history = app.state::<DbState>().history_service();
    let permit = history
        .privacy()
        .authorize_read(
            PrivacyAccessIntent::InteractiveRead,
            SensitiveAccess::ExplicitUserAction,
            Some(expected_epoch),
            now(),
        )
        .map_err(|error| error.to_string())?;
    let entry = if id < 0 {
        let session = app.state::<SessionHistory>();
        let entry = session
            .0
            .lock()
            .map_err(|_| "PrivacyStateUnavailable")?
            .iter()
            .find(|entry| entry.id == id)
            .cloned()
            .ok_or("ClipboardRecordUnavailable")?;
        history
            .session_entry_for_preview_authorized(&entry, &permit, now())
            .map_err(|error| error.to_string())?
    } else {
        history
            .get_entry_for_preview_authorized(id, &permit, now())
            .map_err(|error| error.to_string())?
            .ok_or("ClipboardRecordUnavailable")?
    };
    history
        .privacy()
        .check_read(&permit, now())
        .map_err(|error| error.to_string())?;
    Ok(entry)
}

#[tauri::command]
pub fn get_history_resource_preview(
    app: AppHandle,
    id: i64,
    expected_epoch: u64,
    max_dimension: Option<u32>,
) -> Result<ClipboardEntry, String> {
    let history = app.state::<DbState>().history_service();
    let permit = history
        .privacy()
        .authorize_read(
            PrivacyAccessIntent::InteractiveRead,
            SensitiveAccess::Denied,
            Some(expected_epoch),
            now(),
        )
        .map_err(|error| error.to_string())?;
    let maximum = max_dimension.unwrap_or(384).clamp(64, 1024);
    if id > 0 {
        history
            .get_entry_thumbnail_authorized(id, &permit, now(), maximum)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "SensitiveAccessDenied".into())
    } else {
        let entry = app
            .state::<SessionHistory>()
            .0
            .lock()
            .map_err(|_| "PrivacyStateUnavailable")?
            .iter()
            .find(|entry| entry.id == id)
            .cloned()
            .ok_or("ClipboardRecordUnavailable")?;
        history
            .session_entry_thumbnail_authorized(&entry, &permit, now(), maximum)
            .map_err(|error| error.to_string())
    }
}

#[cfg(all(test, target_os = "windows", not(feature = "portable")))]
mod native_protected_resource_tests {
    use dzc_core::api::{
        HistoryService, PrivacyAccessIntent, PrivacySessionCommand, SensitiveAccess,
    };
    #[test]
    fn native_dpapi_protected_files_remain_ciphertext_and_external_lease_revocation_closes_new_readers(
    ) {
        dzc_platform::install_default_key_provider();
        let directory =
            std::env::temp_dir().join(format!("dzc-native-private-{}", uuid::Uuid::new_v4()));
        let database = directory.join("synthetic.db");
        let history = HistoryService::open(&database).unwrap();
        let source = directory.join("synthetic.txt");
        std::fs::write(&source, b"qvxnativesyntheticfile").unwrap();
        let entry=serde_json::from_value(serde_json::json!({"id":0,"content_type":"file","content":source.to_string_lossy(),"preview":"synthetic","source_app":"Synthetic Native","timestamp":1,"is_pinned":false,"tags":["sensitive"]})).unwrap();
        let id = history.save(&entry, Some(&directory)).unwrap();
        let conn = rusqlite::Connection::open(&database).unwrap();
        let raw: String = conn
            .query_row(
                "SELECT content FROM clipboard_history WHERE id=?1",
                [id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(raw.starts_with("dpapi:"));
        assert!(!raw.contains("qvxnativesyntheticfile"));
        assert!(!directory.join("attachments").exists());
        let permit = history
            .privacy()
            .authorize_read(
                PrivacyAccessIntent::SelectedCopy,
                SensitiveAccess::ExplicitUserAction,
                None,
                1,
            )
            .unwrap();
        let preview = history
            .get_entry_for_preview_authorized(id, &permit, 1)
            .unwrap()
            .unwrap();
        assert!(preview.content.is_empty());
        assert_eq!(preview.preview, "synthetic.txt");
        let export = history.external_files_authorized(id, &permit, 1).unwrap();
        let native = dzc_platform::runtime::private_file_export(export).unwrap();
        let path = native.paths().unwrap().remove(0);
        let mut reader = std::fs::File::open(&path).unwrap();
        history
            .privacy()
            .set_session(PrivacySessionCommand::On { duration_ms: None }, 2)
            .unwrap();
        dzc_platform::runtime::revoke_private_exports().unwrap();
        assert!(std::fs::File::open(&path).is_err());
        use std::io::Read;
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"qvxnativesyntheticfile");
        drop(reader);
        assert!(!std::path::Path::new(&path).exists());
        assert!(native.paths().is_err());
        assert_eq!(std::fs::read(&source).unwrap(), b"qvxnativesyntheticfile");
        drop((native, conn, history));
        let _ = std::fs::remove_dir_all(directory);
    }
}
