use crate::app_state::{AppDataDir, SessionHistory};
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
pub fn toggle_clipboard_pin(
    app_handle: AppHandle,
    state: State<'_, DbState>,
    session: State<'_, SessionHistory>,
    app_data_dir: State<'_, AppDataDir>,
    id: i64,
    is_pinned: bool,
) -> AppResult<()> {
    let mut real_id = id;
    let mut entry_to_save = None;

    {
        let mut session_items = session.inner().0.lock().unwrap();
        if let Some(item) = session_items.iter_mut().find(|i| i.id == id) {
            if id < 0 && is_pinned {
                let mut pending = item.clone();
                pending.is_pinned = true;
                entry_to_save = Some(pending);
            } else {
                item.is_pinned = is_pinned;
            }
        }
    }

    let conn = state.conn.lock().unwrap();

    if let Some(entry) = entry_to_save {
        let data_dir = app_data_dir.0.lock().unwrap().clone();
        let new_id = state.repo.save_with_conn(&conn, &entry, Some(&data_dir)).map_err(AppError::from)?;
        {
            real_id = new_id;
            if let Ok(deleted_ids) = state.repo.enforce_limit_with_conn(&conn, Some(&data_dir)) {
                for deleted_id in deleted_ids {
                    let _ = app_handle.emit("clipboard-removed", deleted_id);
                }
            }
            {
                let mut session_items = session.inner().0.lock().unwrap();
                if let Some(item) = session_items.iter_mut().find(|i| i.id == id) {
                    item.id = new_id;
                    item.is_pinned = is_pinned;
                }
            }
        }
    }

    if real_id > 0 {
        state
            .repo
            .toggle_pin_with_conn(&conn, real_id, is_pinned)
            .map_err(AppError::from)?;
    }
    Ok(())
}

#[tauri::command]
pub fn update_tags(
    state: State<'_, DbState>,
    session: State<'_, SessionHistory>,
    app_data_dir: State<'_, AppDataDir>,
    id: i64,
    tags: Vec<String>,
) -> AppResult<i64> {
    if id < 0 {
        let mut session_items = session.inner().0.lock().unwrap();
        if let Some(index) = session_items.iter().position(|item| item.id == id) {
            let mut item = session_items[index].clone();
            item.tags = tags.clone();

            let data_dir = app_data_dir.0.lock().unwrap().clone();
            let new_id = state.history_service().save(&item, Some(&data_dir))?;

            session_items[index].id = new_id;
            session_items[index].tags = tags;
            return Ok(new_id);
        }
        return Err(AppError::Validation("Item not found".to_string()));
    }

    state.history_service().update_entry_tags(id, tags).map_err(AppError::from)?;
    Ok(id)
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
    app_handle: AppHandle,
    state: State<'_, DbState>,
    session: State<'_, SessionHistory>,
    id: i64,
    new_content: String,
) -> AppResult<()> {
    let preview = truncate_chars_with_suffix(&new_content, 500, "...");

    if id >= 0 {
        state
            .history_service()
            .update_entry_content(id, &new_content, &preview)
            .map_err(AppError::from)?;
    }
    let mut session_items = session.inner().0.lock().unwrap();
    if let Some(item) = session_items.iter_mut().find(|i| i.id == id) {
        item.content = new_content;
        item.preview = preview;
    } else if id < 0 {
        return Err(AppError::Validation("Item not found".into()));
    }
    drop(session_items);
    let _ = app_handle.emit("clipboard-changed", ());
    Ok(())
}
