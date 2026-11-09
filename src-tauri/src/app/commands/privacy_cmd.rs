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

/// Local content actions request one record; only explicit reveal includes protected content.
#[tauri::command]
pub async fn get_privacy_entry(
    app: AppHandle,
    id: i64,
    expected_epoch: u64,
    expected_host_instance_id:Option<String>,
    reveal:Option<bool>,
) -> Result<HostedEntry, String> {
    let bridge=app.state::<DesktopHost>();
    let instance=bridge.instance()?;
    if expected_host_instance_id.as_ref().is_some_and(|expected| expected!=&instance) {return Err("ClipboardHostInstanceChanged".into());}
    let reveal=reveal.unwrap_or(true);
    if let Some(entry)=bridge.entry(id,expected_epoch,reveal,None,expected_host_instance_id.as_deref()).await? {
        if bridge.instance()?!=instance {return Err("ClipboardHostInstanceChanged".into());}
        return Ok(entry);
    }
    if id<0 {bridge.check_instance(id,expected_host_instance_id.as_deref())?;}
    let history = app.state::<DbState>().history_service();
    let permit = history
        .privacy()
        .authorize_read(
            PrivacyAccessIntent::InteractiveRead,
            if reveal {SensitiveAccess::ExplicitUserAction}else{SensitiveAccess::Denied},
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
    if bridge.instance()?!=instance {return Err("ClipboardHostInstanceChanged".into());}
    Ok(HostedEntry{entry,host_instance_id:Some(instance)})
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
