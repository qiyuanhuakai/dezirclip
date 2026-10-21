//! Trusted desktop requests use the same persistent state and opaque permissions as the Host.
use crate::app_state::SessionHistory;
use crate::database::DbState;
use crate::domain::models::ClipboardEntry;
use dzc_core::api::{PrivacyAccessIntent, RuntimeSnapshot, SensitiveAccess};
use crate::app::ipc_host::{DesktopHost,HostedEntry};
use dzc_host::wire::{Method,PrivacyState,PrivacySetParams,PrivacyHotkey};
use tauri::{AppHandle, Manager};

fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[tauri::command]
pub async fn get_clipboard_host_status(app:AppHandle)->Result<dzc_host::wire::ServiceStatus,String> {
    app.state::<DesktopHost>().call(Method::ServiceStatus,serde_json::json!({}),"ServiceStatus").await
}

#[tauri::command]
pub async fn get_privacy_status(app: AppHandle) -> Result<RuntimeSnapshot, String> {
    let bridge=app.state::<DesktopHost>();
    let privacy:PrivacyState=bridge.call(Method::PrivacyGet,serde_json::json!({}),"PrivacyState").await?;
    bridge.snapshot_after(privacy)
}

#[tauri::command]
pub async fn set_privacy_session(
    app: AppHandle,
    enabled: bool,
    duration_ms: Option<u64>,
) -> Result<RuntimeSnapshot, String> {
    if !enabled && duration_ms.is_some() {return Err("PrivacyDurationInvalid".into());}
    let bridge=app.state::<DesktopHost>();
    let privacy:PrivacyState=bridge.call(Method::PrivacySet,PrivacySetParams{active:enabled,duration_ms},"PrivacyState").await?;
    bridge.snapshot_after(privacy)
}

#[tauri::command]
pub async fn toggle_privacy_session(app: AppHandle) -> Result<RuntimeSnapshot, String> {
    let bridge=app.state::<DesktopHost>();
    let privacy:PrivacyState=bridge.call(Method::PrivacyToggle,serde_json::json!({}),"PrivacyState").await?;
    bridge.snapshot_after(privacy)
}

#[tauri::command]
pub async fn set_privacy_session_hotkey(app:AppHandle,hotkey:String)->Result<String,String> {
    let result:PrivacyHotkey=app.state::<DesktopHost>().call(Method::PrivacyHotkeySet,PrivacyHotkey{hotkey},"PrivacyHotkey").await?;
    Ok(result.hotkey)
}

/// Invoked only by the local GUI's explicit reveal action. IPC roles do not grant this intent.
#[tauri::command]
pub async fn get_privacy_entry(
    app: AppHandle,
    id: i64,
    expected_epoch: u64,
    expected_host_instance_id:Option<String>,
) -> Result<HostedEntry, String> {
    let bridge=app.state::<DesktopHost>();
    if let Some(entry)=bridge.entry(id,expected_epoch,true,None,expected_host_instance_id.as_deref()).await? {return Ok(entry);}
    if id<0 {bridge.check_instance(id,expected_host_instance_id.as_deref())?;}
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
    bridge.hosted(entry)
}

#[tauri::command]
pub async fn get_history_resource_preview(
    app: AppHandle,
    id: i64,
    expected_epoch: u64,
    max_dimension: Option<u32>,
    expected_host_instance_id:Option<String>,
) -> Result<HostedEntry, String> {
    let bridge=app.state::<DesktopHost>();
    let maximum = max_dimension.unwrap_or(384).clamp(64, 1024);
    if let Some(entry)=bridge.entry(id,expected_epoch,false,Some(maximum),expected_host_instance_id.as_deref()).await? {return Ok(entry);}
    if id<0 {bridge.check_instance(id,expected_host_instance_id.as_deref())?;}
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
    let entry=if id > 0 {
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
    }?;
    bridge.hosted(entry)
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
