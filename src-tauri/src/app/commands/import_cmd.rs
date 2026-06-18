use tauri::AppHandle;
use crate::database::DbState;
use crate::services::backup::{import_from_encrypted, import_from_json, ImportMode, ImportSummary};

const ENCRYPTED_HEADER_LEN: usize = 12 + 16;

#[derive(Debug, serde::Serialize)]
pub struct ImportSummaryResponse {
    pub imported: usize,
    pub skipped: usize,
    pub mode: String,
}

#[tauri::command]
pub fn import_from_file(
    app: AppHandle,
    path: String,
    mode: String,
    passphrase: Option<String>,
) -> Result<ImportSummaryResponse, String> {
    let import_mode = match mode.as_str() {
        "merge" => ImportMode::Merge,
        "replace" => ImportMode::Replace,
        other => return Err(format!("mode must be 'merge' or 'replace', got '{other}'")),
    };

    let data = std::fs::read(&path).map_err(|e| format!("failed to read file: {e}"))?;
    if data.is_empty() {
        return Err("import file is empty".to_string());
    }

    let summary = if looks_encrypted(&data) {
        let passphrase = passphrase.ok_or_else(|| "passphrase is required for encrypted files".to_string())?;
        if passphrase.is_empty() {
            return Err("passphrase must not be empty".to_string());
        }
        import_from_encrypted(&data, &passphrase).map_err(|e| e.to_string())?
    } else {
        let json = std::str::from_utf8(&data).map_err(|e| format!("file is not valid UTF-8: {e}"))?;
        import_from_json(json, import_mode).map_err(|e| e.to_string())?
    };

    apply_import(app, import_mode, &summary)?;

    Ok(ImportSummaryResponse {
        imported: summary.imported,
        skipped: summary.skipped,
        mode: summary.mode,
    })
}

fn looks_encrypted(data: &[u8]) -> bool {
    if data.len() < ENCRYPTED_HEADER_LEN {
        return false;
    }
    if let Ok(json) = std::str::from_utf8(data) {
        if json.trim_start().starts_with('{') {
            return false;
        }
    }
    true
}

fn apply_import(app: AppHandle, mode: ImportMode, summary: &ImportSummary) -> Result<(), String> {
    let db = app.state::<DbState>();
    let conn = db.conn.lock().map_err(|e| e.to_string())?;

    if mode == ImportMode::Replace {
        conn.execute("DELETE FROM clipboard_history", [])
            .map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM entry_tags", [])
            .map_err(|e| e.to_string())?;
    }

    if summary.imported == 0 {
        return Ok(());
    }

    for entry in &summary.entries {
        let existing = conn
            .query_row(
                "SELECT id FROM clipboard_history WHERE id = ?",
                [entry.id],
                |row| row.get::<_, i64>(0),
            )
            .ok();

        let tags = serde_json::to_string(&entry.tags).unwrap_or_else(|_| "[]".to_string());

        if existing.is_some() {
            conn.execute(
                "UPDATE clipboard_history SET \
                 content_type = ?1, content = ?2, html_content = ?3, source_app = ?4, \
                 timestamp = ?5, preview = ?6, content_hash = ?7, tags = ?8, \
                 is_external = ?9, source_app_path = ?10, pinned_order = ?11 \
                 WHERE id = ?12",
                rusqlite::params![
                    entry.content_type,
                    entry.content,
                    entry.html_content,
                    entry.source_app,
                    entry.updated_at,
                    entry.preview,
                    entry.id,
                    tags,
                    0,
                    entry.source_app_path,
                    entry.pinned_order,
                    entry.id,
                ],
            )
            .map_err(|e| e.to_string())?;
        } else {
            conn.execute(
                "INSERT INTO clipboard_history \
                 (id, content_type, content, html_content, source_app, timestamp, preview, \
                  is_pinned, content_hash, tags, is_external, pinned_order, source_app_path, \
                  ocr_text, ocr_status) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, NULL, 'pending')",
                rusqlite::params![
                    entry.id,
                    entry.content_type,
                    entry.content,
                    entry.html_content,
                    entry.source_app,
                    entry.created_at,
                    entry.preview,
                    if entry.is_pinned { 1 } else { 0 },
                    entry.id,
                    tags,
                    0,
                    entry.pinned_order,
                    entry.source_app_path,
                ],
            )
            .map_err(|e| e.to_string())?;
        }
    }

    Ok(())
}
