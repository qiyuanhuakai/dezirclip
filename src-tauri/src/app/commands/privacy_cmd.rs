//! Trusted desktop requests use the same persistent state and opaque permissions as the Host.
use crate::app_state::SessionHistory;
use crate::database::DbState;
use crate::domain::models::ClipboardEntry;
use dzc_core::api::{PrivacyAccessIntent, PrivacySessionCommand, RuntimeSnapshot, SensitiveAccess};
use tauri::{AppHandle, Emitter, Manager};

fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn publish(app: &AppHandle, snapshot: RuntimeSnapshot) -> RuntimeSnapshot {
    let _ = app.emit("privacy-state-changed", &snapshot);
    snapshot
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
    Ok(publish(&app, snapshot))
}

#[tauri::command]
pub fn toggle_privacy_session(app: AppHandle) -> Result<RuntimeSnapshot, String> {
    let history = app.state::<DbState>().history_service();
    let snapshot = history
        .privacy()
        .toggle(now())
        .map_err(|error| error.to_string())?;
    Ok(publish(&app, snapshot))
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
            .authorize_session_entry(&entry, &permit, now())
            .map_err(|error| error.to_string())?
    } else {
        history
            .get_entry_authorized(id, &permit, now())
            .map_err(|error| error.to_string())?
            .ok_or("ClipboardRecordUnavailable")?
    };
    history
        .privacy()
        .check_read(&permit, now())
        .map_err(|error| error.to_string())?;
    Ok(entry)
}
