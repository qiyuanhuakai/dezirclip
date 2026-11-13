use tauri::{AppHandle, Manager, WebviewWindow};
use dzc_host::wire::*;

#[tauri::command]
pub async fn copy_dropped_files(app: AppHandle, window: WebviewWindow, paths: Vec<String>, expected_host_instance_id: String, expected_privacy_epoch: u64) -> Result<(), String> {
    if window.label() != "main" { return Err("FileDropWindowUnavailable".into()); }
    if crate::infrastructure::file_drag::active() { return Ok(()); }
    let result: OperationResult = app.state::<crate::app::ipc_host::DesktopHost>().blocking_call(Method::DesktopFilesDrop,
        FileDropParams { paths, service_instance_id: expected_host_instance_id, expected_epoch: expected_privacy_epoch }, "OperationResult").await?;
    if !result.performed { return Err("FileDropUnconfirmed".into()); }
    Ok(())
}

#[tauri::command]
pub async fn start_file_drag(app: AppHandle, window: WebviewWindow, id: i64, expected_host_instance_id: String, expected_privacy_epoch: u64, allow_sensitive: bool) -> Result<(), String> {
    if window.label() != "main" || !window.is_visible().map_err(|e| e.to_string())? { return Err("FileDragWindowUnavailable".into()); }
    let result: FileDragResult = app.state::<crate::app::ipc_host::DesktopHost>().blocking_call(Method::DesktopFilesDrag,
        FileDragParams { id, service_instance_id: expected_host_instance_id, expected_epoch: expected_privacy_epoch, reveal: allow_sensitive }, "FileDragResult").await?;
    let (send, receive) = tokio::sync::oneshot::channel();
    let source = window.clone();
    window.run_on_main_thread(move || crate::infrastructure::file_drag::start(&source, result.paths,
        move |result| { let _ = send.send(result); })).map_err(|e| e.to_string())?;
    receive.await.map_err(|_| "FileDragWindowClosed".to_string())?
}
