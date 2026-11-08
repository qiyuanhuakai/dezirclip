use crate::error::{AppError, AppResult};
use tauri::{AppHandle, Emitter, Manager};

#[tauri::command]
pub async fn toggle_clipboard_pin(
    app_handle:AppHandle,id:i64,is_pinned:bool,expected_host_instance_id:Option<String>,
    expected_privacy_epoch:Option<u64>,confirm_persistence:Option<bool>,allow_sensitive:Option<bool>,
)->AppResult<()> {
    if (confirm_persistence==Some(true)||allow_sensitive==Some(true)) && expected_privacy_epoch.is_none() {return Err(AppError::Validation("PrivacyEpochRequired".into()));}
    let bridge=app_handle.state::<crate::app::ipc_host::DesktopHost>();
    let mut request=bridge.mutation_request(id,"pin",expected_privacy_epoch).map_err(AppError::Validation)?;
    request.pinned=Some(is_pinned);request.confirm_persistence=confirm_persistence;request.reveal=allow_sensitive.unwrap_or(false);
    bridge.mutation(request,expected_host_instance_id.as_deref()).await.map_err(AppError::Validation)?;
    let _=app_handle.emit("clipboard-changed",());Ok(())
}

#[tauri::command]
pub async fn update_tags(
    app_handle:AppHandle,id:i64,tags:Vec<String>,expected_host_instance_id:Option<String>,
    expected_privacy_epoch:Option<u64>,confirm_persistence:Option<bool>,allow_sensitive:Option<bool>,
)->AppResult<i64> {
    if (confirm_persistence==Some(true)||allow_sensitive==Some(true)) && expected_privacy_epoch.is_none() {return Err(AppError::Validation("PrivacyEpochRequired".into()));}
    let bridge=app_handle.state::<crate::app::ipc_host::DesktopHost>();
    let mut request=bridge.mutation_request(id,"tags",expected_privacy_epoch).map_err(AppError::Validation)?;
    request.tags=Some(tags);request.confirm_persistence=confirm_persistence;request.reveal=allow_sensitive.unwrap_or(false);
    let id=bridge.mutation(request,expected_host_instance_id.as_deref()).await.map_err(AppError::Validation)?.ok_or_else(||AppError::Validation("ClipboardHostUnavailable".into()))?;
    let _=app_handle.emit("clipboard-changed",());Ok(id)
}

#[tauri::command]
pub async fn add_manual_item(
    app_handle: AppHandle,
    content: String,
    content_type: String,
    tags: Vec<String>,
    expected_privacy_epoch: Option<u64>,
    confirm_persistence: Option<bool>,
) -> AppResult<i64> {
    let host = app_handle.state::<crate::app::ipc_host::DesktopHost>();
    let new_id = host.add_manual(content, content_type, tags, expected_privacy_epoch, confirm_persistence.unwrap_or(false)).await.map_err(AppError::Validation)?;
    let _ = app_handle.emit("clipboard-changed", ());
    Ok(new_id)
}

#[tauri::command]
pub async fn update_item_content(
    app_handle:AppHandle,id:i64,new_content:String,expected_host_instance_id:Option<String>,
    expected_privacy_epoch:Option<u64>,confirm_persistence:Option<bool>,allow_sensitive:Option<bool>,
)->AppResult<()> {
    if (confirm_persistence==Some(true)||allow_sensitive==Some(true)) && expected_privacy_epoch.is_none() {return Err(AppError::Validation("PrivacyEpochRequired".into()));}
    let bridge=app_handle.state::<crate::app::ipc_host::DesktopHost>();
    let mut request=bridge.mutation_request(id,"edit",expected_privacy_epoch).map_err(AppError::Validation)?;
    request.content=Some(new_content);request.confirm_persistence=confirm_persistence;request.reveal=allow_sensitive.unwrap_or(false);
    bridge.mutation(request,expected_host_instance_id.as_deref()).await.map_err(AppError::Validation)?;
    let _=app_handle.emit("clipboard-changed",());Ok(())
}
