use crate::app_state::{AppDataDir, SessionHistory};
use crate::database::DbState;
use crate::domain::models::ClipboardEntry;
use crate::error::{AppError, AppResult};
use crate::infrastructure::repository::clipboard_repo::ClipboardRepository;
use crate::infrastructure::repository::tag_repo::TagRepository;
use crate::services::clipboard::truncate_html_for_preview;
use tauri::State;

const UI_CONTENT_LIMIT: usize = 2000;
const UI_PREVIEW_LIMIT: usize = 500;
const TAG_CONTENT_LIMIT: usize = 50000;
const MAX_HISTORY_LIMIT: i32 = 150;
const MIN_HISTORY_LIMIT: i32 = 1;

fn is_text_like_content_type(content_type: &str) -> bool {
    matches!(content_type, "text" | "code" | "url" | "rich_text")
}

fn truncate_with_suffix(input: &str, limit: usize, suffix: &str) -> String {
    let mut chars = input.chars();
    if chars.by_ref().nth(limit).is_some() {
        let head: String = input.chars().take(limit).collect();
        format!("{head}{suffix}")
    } else {
        input.to_string()
    }
}

fn normalize_entries_for_ui(entries: &mut [ClipboardEntry]) {
    for item in entries {
        if is_text_like_content_type(&item.content_type) {
            item.content =
                truncate_with_suffix(&item.content, UI_CONTENT_LIMIT, "... [Truncated for speed]");
            item.preview =
                truncate_with_suffix(&item.content, UI_PREVIEW_LIMIT.saturating_sub(3), "...");
        }

        if let Some(ref html) = item.html_content {
            if html.chars().count() > 5000 {
                item.html_content = truncate_html_for_preview(html);
            }
        }
    }
}

fn normalize_limit(limit: i32) -> i32 {
    limit.clamp(MIN_HISTORY_LIMIT, MAX_HISTORY_LIMIT)
}

fn captured_entry_summary(
    repository: &dyn ClipboardRepository,
    session: &SessionHistory,
    id: i64,
) -> AppResult<ClipboardEntry> {
    let entry = if id < 0 {
        session
            .0
            .lock()
            .map_err(|_| AppError::Internal("CapturedEntryReadFailed".into()))?
            .iter()
            .find(|entry| entry.id == id)
            .cloned()
    } else if id > 0 {
        repository
            .get_entry_by_id(id)
            .map_err(|_| AppError::Internal("CapturedEntryReadFailed".into()))?
    } else {
        None
    };
    let mut entry = entry.ok_or_else(|| AppError::Validation("CapturedEntryUnavailable".into()))?;
    normalize_entries_for_ui(std::slice::from_mut(&mut entry));
    Ok(crate::services::clipboard::summarize_captured_entry(entry))
}

/// Hydrate a committed capture on demand; capture notifications contain only its ID.
#[tauri::command]
pub async fn get_captured_entry_summary(
    state: State<'_, DbState>,
    session: State<'_, SessionHistory>,
    id: i64,
) -> AppResult<ClipboardEntry> {
    captured_entry_summary(&state.repo, &session, id)
}

#[tauri::command]
pub async fn get_clipboard_history(
    state: State<'_, DbState>,
    session: State<'_, SessionHistory>,
    limit: i32,
    offset: i32,
    content_type: Option<String>,
) -> AppResult<Vec<ClipboardEntry>> {
    let limit = normalize_limit(limit);
    // 1. Get history from repository
    let mut history = state
        .repo
        .get_history(limit, offset, content_type.as_deref())?;

    // 2. Add session history items (non-persisted) ONLY on the first page
    if offset == 0 {
        let session_items = session.inner().0.lock().unwrap();
        for item in session_items.iter().rev() {
            if let Some(ct) = content_type.as_deref() {
                if item.content_type != ct {
                    continue;
                }
            }
            // Avoid duplicates: if item is already in DB, it will have id > 0
            if !history.iter().any(|h| h.id == item.id && item.id != 0) {
                history.push(item.clone());
            }
        }
    }

    // 3. Apply stable sorting: Pinned -> Pinned Order -> Timestamp -> ID
    // This MUST match the repository's logic to maintain pagination stability
    history.sort_by(|a, b| {
        b.is_pinned
            .cmp(&a.is_pinned)
            .then_with(|| b.pinned_order.cmp(&a.pinned_order))
            .then_with(|| b.timestamp.cmp(&a.timestamp))
            .then_with(|| b.id.cmp(&a.id))
    });

    // 4. Truncate to limit
    if history.len() > limit as usize {
        history.truncate(limit as usize);
    }

    normalize_entries_for_ui(&mut history);

    Ok(history)
}

#[tauri::command]
pub async fn search_clipboard_history(
    state: State<'_, DbState>,
    session: State<'_, SessionHistory>,
    search_term: String,
    limit: i32,
) -> AppResult<Vec<ClipboardEntry>> {
    let limit = normalize_limit(limit);
    let mut history = state.repo.search(&search_term, limit)?;

    let term = search_term.to_lowercase();
    let session_items = session.inner().0.lock().unwrap();
    for item in session_items.iter().rev() {
        let matches = item.content.to_lowercase().contains(&term)
            || item.source_app.to_lowercase().contains(&term)
            || item.tags.iter().any(|t| t.to_lowercase().contains(&term));

        if matches {
            if !history.iter().any(|h| h.id == item.id && item.id != 0) {
                history.push(item.clone());
            }
        }
    }

    history.sort_by(|a, b| b.timestamp.cmp(&a.timestamp).then_with(|| b.id.cmp(&a.id)));
    if history.len() > limit as usize {
        history.truncate(limit as usize);
    }

    normalize_entries_for_ui(&mut history);

    Ok(history)
}

#[tauri::command]
pub fn delete_clipboard_entry(
    state: State<'_, DbState>,
    session: State<'_, SessionHistory>,
    app_data: State<'_, AppDataDir>,
    id: i64,
) -> AppResult<()> {
    {
        let mut session_items = session.inner().0.lock().unwrap();
        session_items.retain(|item| item.id != id);
    }

    if id > 0 {
        let data_dir = app_data.0.lock().unwrap();
        state.repo.delete(id, Some(&data_dir))?;
    }
    Ok(())
}

#[tauri::command]
pub fn clear_clipboard_history(
    state: State<'_, DbState>,
    session: State<'_, SessionHistory>,
    app_data: State<'_, AppDataDir>,
) -> AppResult<()> {
    {
        let mut session_items = session.inner().0.lock().unwrap();
        session_items.retain(|item| item.is_pinned || !item.tags.is_empty());
    }
    let data_dir = app_data.0.lock().unwrap();
    state.repo.clear(Some(&data_dir)).map_err(AppError::from)
}

#[tauri::command]
pub async fn get_tag_items(
    state: State<'_, DbState>,
    tag: String,
) -> AppResult<Vec<ClipboardEntry>> {
    let mut history = state
        .tag_repo
        .get_entries_by_tag(&tag)
        .map_err(AppError::from)?;

    for item in &mut history {
        if is_text_like_content_type(&item.content_type) {
            item.content =
                truncate_with_suffix(&item.content, TAG_CONTENT_LIMIT, "... [Content Truncated]");
        }
    }

    Ok(history)
}

#[tauri::command]
pub async fn get_tag_items_page(
    state: State<'_, DbState>,
    tag: String,
    query: Option<String>,
    sort_by: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
) -> AppResult<crate::infrastructure::repository::tag_page::TagItemsPage> {
    let repository =
        crate::infrastructure::repository::tag_repo::SqliteTagRepository::new(state.conn.clone());
    tauri::async_runtime::spawn_blocking(move || {
        repository.get_page(
            &tag,
            query.as_deref().unwrap_or(""),
            sort_by.as_deref().unwrap_or("time"),
            limit.unwrap_or(60),
            offset.unwrap_or(0),
        )
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))?
    .map_err(AppError::from)
}

#[tauri::command]
pub async fn get_tag_item_detail(state: State<'_, DbState>, id: i64) -> AppResult<ClipboardEntry> {
    let repository =
        crate::infrastructure::repository::clipboard_repo::SqliteClipboardRepository::new(
            state.conn.clone(),
        );
    tauri::async_runtime::spawn_blocking(move || repository.get_entry_by_id(id))
        .await
        .map_err(|error| AppError::Internal(error.to_string()))?
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::Validation("Entry not found".into()))
}

#[tauri::command]
pub fn remove_tag_from_all(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    session: State<'_, SessionHistory>,
    tag_name: String,
) -> AppResult<()> {
    state
        .tag_repo
        .remove_preserving_entries(&tag_name)
        .map_err(AppError::from)?;
    if let Ok(mut history) = session.0.lock() {
        for item in history.iter_mut() {
            item.tags.retain(|tag| tag != &tag_name);
        }
    }
    use tauri::Emitter;
    let _ = app.emit("clipboard-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn get_all_tags_info(
    state: State<'_, DbState>,
) -> AppResult<std::collections::HashMap<String, i32>> {
    state.tag_repo.get_all_with_counts().map_err(AppError::from)
}

#[tauri::command]
pub fn rename_tag_globally(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    session: State<'_, SessionHistory>,
    old_name: String,
    new_name: String,
) -> AppResult<()> {
    let new_name = new_name.trim().to_string();
    state
        .tag_repo
        .rename(&old_name, &new_name)
        .map_err(AppError::from)?;
    {
        let mut session_items = session.inner().0.lock().unwrap();
        for item in session_items.iter_mut() {
            for tag in item.tags.iter_mut() {
                if *tag == old_name {
                    *tag = new_name.clone();
                }
            }
            item.tags.sort();
            item.tags.dedup();
        }
    }

    use tauri::Emitter;
    let _ = app.emit("clipboard-changed", ());
    Ok(())
}

fn delete_sensitive_tag_contents(
    app: &tauri::AppHandle,
    state: &DbState,
    session: &SessionHistory,
    app_data: &AppDataDir,
    tag_name: &str,
) -> AppResult<()> {
    let data_dir = app_data
        .0
        .lock()
        .map_err(|error| AppError::Internal(error.to_string()))?
        .clone();
    let report = state
        .tag_repo
        .delete_globally(tag_name, Some(&data_dir))
        .map_err(AppError::from)?;
    if let Ok(mut history) = session.0.lock() {
        history.retain(|item| {
            !report.deleted_ids.contains(&item.id) && !item.tags.iter().any(|tag| tag == tag_name)
        });
    }
    state.repo.invalidate_caches();
    use tauri::Emitter;
    let _ = app.emit("clipboard-changed", ());
    if !report.cleanup_errors.is_empty() {
        return Err(AppError::IO(format!(
            "history and tag were deleted, but attachment cleanup failed: {}",
            report.cleanup_errors.join("; ")
        )));
    }
    Ok(())
}

#[tauri::command]
pub fn delete_sensitive_tag_with_contents(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    session: State<'_, SessionHistory>,
    app_data: State<'_, AppDataDir>,
    tag_name: String,
) -> AppResult<()> {
    delete_sensitive_tag_contents(&app, &state, &session, &app_data, &tag_name)
}

// Preserve the old command name without retaining an unguarded destructive path.
#[tauri::command]
pub fn delete_tag_from_all(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    session: State<'_, SessionHistory>,
    app_data: State<'_, AppDataDir>,
    tag_name: String,
) -> AppResult<()> {
    delete_sensitive_tag_contents(&app, &state, &session, &app_data, &tag_name)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagEntriesDeleted {
    deleted_count: usize,
}

#[tauri::command]
pub async fn delete_tag_entries(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    session: State<'_, SessionHistory>,
    app_data: State<'_, AppDataDir>,
    tag_name: String,
    selection: crate::infrastructure::repository::tag_batch::TagEntrySelection,
) -> AppResult<TagEntriesDeleted> {
    let data_dir = app_data
        .0
        .lock()
        .map_err(|error| AppError::Internal(error.to_string()))?
        .clone();
    let repository =
        crate::infrastructure::repository::tag_repo::SqliteTagRepository::new(state.conn.clone());
    let report = tauri::async_runtime::spawn_blocking(move || {
        repository.delete_entries_for_tag(&tag_name, selection, Some(&data_dir))
    })
    .await
    .map_err(|error| AppError::Internal(error.to_string()))?
    .map_err(AppError::from)?;
    if let Ok(mut history) = session.0.lock() {
        history.retain(|item| !report.deleted_ids.contains(&item.id));
    }
    state.repo.invalidate_caches();
    use tauri::Emitter;
    let _ = app.emit("clipboard-changed", ());
    if !report.cleanup_errors.is_empty() {
        return Err(AppError::IO(format!(
            "history entries were deleted, but attachment cleanup failed: {}",
            report.cleanup_errors.join("; ")
        )));
    }
    Ok(TagEntriesDeleted {
        deleted_count: report.deleted_ids.len(),
    })
}

#[tauri::command]
pub fn create_new_tag(state: State<'_, DbState>, tag_name: String) -> AppResult<()> {
    let tag_name = tag_name.trim();
    if tag_name.is_empty()
        || tag_name.chars().count() > 80
        || tag_name.chars().any(char::is_control)
    {
        return Err(AppError::Validation(
            "tag names must contain 1..80 printable characters".into(),
        ));
    }
    state.tag_repo.create(tag_name).map_err(AppError::from)
}

#[tauri::command]
pub async fn get_clipboard_content(
    state: State<'_, DbState>,
    session: State<'_, SessionHistory>,
    id: i64,
) -> AppResult<String> {
    {
        let session_items = session.inner().0.lock().unwrap();
        if let Some(item) = session_items.iter().find(|i| i.id == id) {
            return Ok(item.content.clone());
        }
    }

    state
        .repo
        .get_entry_content(id)
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::Validation("Entry not found".to_string()))
}

#[tauri::command]
pub fn update_pinned_order(state: State<'_, DbState>, orders: Vec<(i64, i64)>) -> AppResult<()> {
    state
        .repo
        .update_pinned_order(orders)
        .map_err(AppError::from)
}

#[tauri::command]
pub async fn get_db_count(state: State<'_, DbState>) -> AppResult<i64> {
    state.repo.get_count().map_err(AppError::from)
}

#[cfg(test)]
mod capture_summary_tests {
    use super::*;
    use crate::infrastructure::repository::clipboard_repo::SqliteClipboardRepository;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    fn entry(id: i64) -> ClipboardEntry {
        ClipboardEntry {
            id,
            content_type: "rich_text".into(),
            content: "body ".repeat(1000),
            html_content: Some("<p>body</p>".repeat(1000)),
            source_app: "Editor".into(),
            source_app_path: None,
            timestamp: 1,
            preview: "body ".repeat(1000),
            is_pinned: true,
            tags: vec!["work".into()],
            use_count: 2,
            is_external: false,
            pinned_order: 3,
            file_preview_exists: true,
            content_kinds: Vec::new(),
            ocr_text: None,
            ocr_status: None,
        }
    }

    fn repository() -> SqliteClipboardRepository {
        SqliteClipboardRepository::new(Arc::new(Mutex::new(
            crate::database::init_db(":memory:").unwrap(),
        )))
    }

    #[test]
    fn session_summary_is_bounded_preserves_metadata_and_does_not_mutate_source() {
        let repository = repository();
        let session = SessionHistory(Mutex::new(VecDeque::from([entry(-1)])));
        let summary = captured_entry_summary(&repository, &session, -1).unwrap();
        assert_eq!(summary.id, -1);
        assert!(summary.content.chars().count() < 2100);
        assert!(summary.html_content.unwrap().chars().count() < 5100);
        assert!(summary.preview.chars().count() <= 500);
        assert!(summary.is_pinned);
        assert_eq!(summary.tags, vec!["work"]);
        assert_eq!(summary.pinned_order, 3);
        assert_eq!(session.0.lock().unwrap()[0].content.len(), 5000);
        for id in [-2, 0, 1] {
            assert!(captured_entry_summary(&repository, &session, id)
                .unwrap_err()
                .to_string()
                .ends_with("CapturedEntryUnavailable"));
        }
    }

    #[test]
    fn persistent_image_summary_uses_the_committed_attachment_path() {
        let directory = std::env::temp_dir().join(format!(
            "dezirclip-capture-summary-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&directory).unwrap();
        let connection = Arc::new(Mutex::new(crate::database::init_db(":memory:").unwrap()));
        let repository = SqliteClipboardRepository::new(connection.clone());
        let mut image = entry(0);
        image.content_type = "image".into();
        image.html_content = None;
        image.content = "data:image/png;base64,b3RoZXI=".into();
        let id = repository
            .save_with_conn(&connection.lock().unwrap(), &image, Some(&directory))
            .unwrap();
        let session = SessionHistory(Mutex::new(VecDeque::new()));
        let summary = captured_entry_summary(&repository, &session, id).unwrap();
        assert_eq!(summary.id, id);
        assert!(summary.is_external);
        assert!(!summary.content.starts_with("data:"));
        assert!(std::path::Path::new(&summary.content).starts_with(directory.join("attachments")));
        assert_eq!(std::fs::read(&summary.content).unwrap(), b"other");
        assert!(image.content.starts_with("data:"));
        drop(repository);
        drop(connection);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn summary_read_failures_do_not_expose_storage_errors_or_payloads() {
        let connection = Arc::new(Mutex::new(crate::database::init_db(":memory:").unwrap()));
        let repository = SqliteClipboardRepository::new(connection.clone());
        connection
            .lock()
            .unwrap()
            .execute_batch("DROP TABLE clipboard_history")
            .unwrap();
        let session = SessionHistory(Mutex::new(VecDeque::new()));
        let error = captured_entry_summary(&repository, &session, 1)
            .unwrap_err()
            .to_string();
        assert!(error.ends_with("CapturedEntryReadFailed"));
        assert!(!error.contains("clipboard_history"));
    }
}
