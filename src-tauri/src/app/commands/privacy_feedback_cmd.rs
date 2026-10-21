//! Local desktop policy requests; remote session IDs require the owning Host's identity.
use crate::app_state::SessionHistory;
use crate::database::DbState;
use dzc_core::api::{
    CoreResult, PrivacyAccessIntent, PrivacyDecisionReport, PrivacyFeedbackAction,
    PrivacyReadPermit, PrivacyRuleSummary, RuntimeSnapshot, SensitiveAccess,
};
use tauri::{AppHandle, Emitter, Manager};

fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn permission(app: &AppHandle, expected_epoch: u64) -> Result<PrivacyReadPermit, String> {
    app.state::<DbState>()
        .history_service()
        .privacy()
        .authorize_read(
            PrivacyAccessIntent::InteractiveRead,
            SensitiveAccess::ExplicitUserAction,
            Some(expected_epoch),
            now(),
        )
        .map_err(|error| error.to_string())
}

// The service-enabled desktop adapter replaces this local publication with its Owner ACK.
async fn finish_mutation(
    app: &AppHandle,
    result: CoreResult<RuntimeSnapshot>,
) -> Result<RuntimeSnapshot, String> {
    // Cleanup can fail after a protected SQL commit; revoke even on an error response.
    dzc_platform::runtime::revoke_private_exports()
        .map_err(|_| "PrivateExportRevocationUnconfirmed".to_owned())?;
    let current = app
        .state::<DbState>()
        .history_service()
        .runtime_snapshot(now())
        .map_err(|error| error.to_string())?;
    let _ = app.emit("privacy-state-changed", &current);
    let _ = app.emit("clipboard-changed", ());
    result.map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn privacy_feedback(
    app: AppHandle,
    id: i64,
    expected_epoch: u64,
    action: PrivacyFeedbackAction,
    expected_host_instance_id: Option<String>,
) -> Result<RuntimeSnapshot, String> {
    if id < 0 {
        // An original Host instance ID must be checked by the owner, never inferred here.
        let _ = expected_host_instance_id;
        return Err("SessionFeedbackUnavailable".into());
    }
    if id == 0 {
        return Err("ClipboardRecordUnavailable".into());
    }
    let permit = permission(&app, expected_epoch)?;
    let result = app
        .state::<DbState>()
        .history_service()
        .privacy_policy()
        .feedback(id, action, &permit, now());
    finish_mutation(&app, result).await
}

#[tauri::command]
pub fn get_privacy_rules(app: AppHandle) -> Result<Vec<PrivacyRuleSummary>, String> {
    app.state::<DbState>()
        .history_service()
        .privacy_policy()
        .rules()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn remove_privacy_rule(
    app: AppHandle,
    rule_id: String,
    expected_epoch: u64,
) -> Result<RuntimeSnapshot, String> {
    let permit = permission(&app, expected_epoch)?;
    let result = {
        let state = app.state::<SessionHistory>();
        let mut session = state.0.lock().map_err(|_| "PrivacyStateUnavailable")?;
        app.state::<DbState>()
            .history_service()
            .privacy_policy()
            .remove_rule_with_session(&rule_id, session.make_contiguous(), &permit, now())
    };
    finish_mutation(&app, result).await
}

#[tauri::command]
pub fn get_privacy_decision(
    app: AppHandle,
    id: i64,
    expected_epoch: u64,
    expected_host_instance_id: Option<String>,
) -> Result<PrivacyDecisionReport, String> {
    if id < 0 {
        let _ = expected_host_instance_id;
        return Err("SessionFeedbackUnavailable".into());
    }
    let permit = permission(&app, expected_epoch)?;
    app.state::<DbState>()
        .history_service()
        .privacy_policy()
        .decision(id, &permit, now())
        .map_err(|error| error.to_string())
}
