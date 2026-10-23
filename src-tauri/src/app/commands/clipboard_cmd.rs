use crate::app_state::{AppDataDir};
use crate::database::{self, DbState};
use crate::error::{AppError, AppResult};
use tauri::{AppHandle, Emitter, Manager, State};

fn truncate_chars_with_suffix(text: &str, max_chars: usize, suffix: &str) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let cut = text
        .char_indices()
        .nth(max_chars)
        .map(|(idx, _)| idx)
        .unwrap_or(text.len());
    let mut out = String::with_capacity(cut + suffix.len());
    out.push_str(&text[..cut]);
    out.push_str(suffix);
    out
}

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
    state: State<'_, DbState>,
    content: String,
    content_type: String,
    tags: Vec<String>,
    expected_privacy_epoch: Option<u64>,
    confirm_persistence: Option<bool>,
) -> AppResult<i64> {
    let preview = truncate_chars_with_suffix(&content, 200, "...");

    let entry = database::ClipboardEntry {
        id: 0,
        content_type,
        content,
        html_content: None,
        source_app: "Manual".to_string(),
        source_app_path: None,
        timestamp: chrono::Utc::now().timestamp_millis(),
        preview,
        is_pinned: false,
        tags,
        use_count: 0,
        is_external: false,
        pinned_order: 0,
        file_preview_exists: true,
        content_kinds: Vec::new(),
        ocr_text: None,
        ocr_status: None,
    };

    let app_data_dir = app_handle.state::<AppDataDir>();
    let data_dir = app_data_dir.0.lock().unwrap().clone();
    let confirmed=confirm_persistence.unwrap_or(false);
    if confirmed && expected_privacy_epoch.is_none() {
        return Err(AppError::Validation("PrivacyEpochRequired".into()));
    }
    let history=state.history_service();
    let permit=history.privacy().authorize_read(
        dzc_core::api::PrivacyAccessIntent::InteractiveRead,
        dzc_core::api::SensitiveAccess::Denied,
        expected_privacy_epoch,chrono::Utc::now().timestamp_millis(),
    ).map_err(|error|AppError::Validation(error.to_string()))?;
    let intent=if confirmed {dzc_core::api::ManualPersistenceIntent::ConfirmedDuringPrivacySession}
        else{dzc_core::api::ManualPersistenceIntent::Ordinary};
    let new_id=history.save_manual_authorized(&entry,Some(&data_dir),intent,&permit,chrono::Utc::now().timestamp_millis())?;
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
