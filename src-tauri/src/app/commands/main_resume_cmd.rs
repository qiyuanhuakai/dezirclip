use tauri::{Manager, WebviewWindow};
use crate::app::ipc_host::DesktopHost;
use dzc_host::wire::{Method, ServiceStatus};

#[tauri::command]
pub fn get_main_window_resume_request(window: WebviewWindow) -> Result<Option<u64>, String> {
    if window.label() != "main" { return Err("MainWindowRequired".into()); }
    Ok(crate::app::idle_destroyer::pending_main_window_resume())
}

#[tauri::command]
pub async fn complete_main_window_resume(window: WebviewWindow, request_id: u64, expected_host_instance_id: String, expected_epoch: u64) -> Result<bool, String> {
    if window.label() != "main" { return Err("MainWindowRequired".into()); }
    let bridge = window.state::<DesktopHost>();
    bridge.check_queue_state(&expected_host_instance_id, expected_epoch)?;
    let status: ServiceStatus = bridge.call(Method::ServiceStatus, serde_json::json!({}), "ServiceStatus").await?;
    if status.state.service_instance_id != expected_host_instance_id || status.privacy.privacy_epoch != expected_epoch {
        return Err("ClipboardHostInstanceChanged".into());
    }
    bridge.check_queue_state(&expected_host_instance_id, expected_epoch)?;
    let ready=crate::app::idle_destroyer::mark_main_window_resume_ready(request_id);
    if ready {crate::app::surface_supervisor::notify_main_shown(window.app_handle());}
    Ok(ready)
}
