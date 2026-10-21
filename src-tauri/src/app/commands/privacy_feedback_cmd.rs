//! Desktop policy requests always execute in the current capture owner's Host.
use crate::app::ipc_host::DesktopHost;
use dzc_core::api::{PrivacyDecisionReport,PrivacyFeedbackAction,PrivacyRuleSummary,RuntimeSnapshot};
use tauri::{AppHandle,Emitter,Manager};

#[tauri::command]
pub async fn privacy_feedback(app:AppHandle,id:i64,expected_epoch:u64,action:PrivacyFeedbackAction,expected_host_instance_id:Option<String>)->Result<RuntimeSnapshot,String> {
    let result=app.state::<DesktopHost>().feedback(id,action,expected_epoch,expected_host_instance_id.as_deref()).await?;
    let _=app.emit("clipboard-changed",());Ok(result)
}
#[tauri::command]
pub async fn get_privacy_rules(app:AppHandle)->Result<Vec<PrivacyRuleSummary>,String> {
    app.state::<DesktopHost>().policy_rules().await
}
#[tauri::command]
pub async fn remove_privacy_rule(app:AppHandle,rule_id:String,expected_epoch:u64)->Result<RuntimeSnapshot,String> {
    let result=app.state::<DesktopHost>().remove_rule(rule_id,expected_epoch).await?;
    let _=app.emit("clipboard-changed",());Ok(result)
}
#[tauri::command]
pub async fn get_privacy_decision(app:AppHandle,id:i64,expected_epoch:u64,expected_host_instance_id:Option<String>)->Result<PrivacyDecisionReport,String> {
    app.state::<DesktopHost>().policy_decision(id,expected_epoch,expected_host_instance_id.as_deref()).await
}
