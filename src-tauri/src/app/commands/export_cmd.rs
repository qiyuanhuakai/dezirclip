use tauri::AppHandle;
use crate::database::DbState;
use crate::infrastructure::repository::clipboard_repo::ClipboardRepository;
use crate::services::backup::{export_to_encrypted, export_to_json, ExportEntry};
use crate::domain::models::ClipboardEntry;

#[derive(Debug, serde::Serialize)]
pub struct ExportSummary {
    pub count: usize,
    pub format: String,
    pub encrypted: bool,
    pub path: String,
}

#[tauri::command]
pub fn export_to_file(
    app: AppHandle,
    path: String,
    format: String,
    passphrase: Option<String>,
) -> Result<ExportSummary, String> {
    match format.as_str() {
        "json" => export_json(app, path, format),
        "encrypted" => export_encrypted(app, path, format, passphrase),
        _ => Err("format must be 'json' or 'encrypted'".to_string()),
    }
}

fn export_json(app: AppHandle, path: String, format: String) -> Result<ExportSummary, String> {
    let entries = load_entries(&app)?;
    let export_entries: Vec<ExportEntry> = entries.into_iter().map(into_export_entry).collect();
    let json = export_to_json(export_entries).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(ExportSummary {
        count: 0,
        format,
        encrypted: false,
        path,
    })
}

fn export_encrypted(
    app: AppHandle,
    path: String,
    format: String,
    passphrase: Option<String>,
) -> Result<ExportSummary, String> {
    let passphrase = passphrase.ok_or_else(|| "passphrase is required for encrypted format".to_string())?;
    if passphrase.is_empty() {
        return Err("passphrase must not be empty".to_string());
    }
    let entries = load_entries(&app)?;
    let export_entries: Vec<ExportEntry> = entries.into_iter().map(into_export_entry).collect();
    let blob = export_to_encrypted(export_entries, &passphrase).map_err(|e| e.to_string())?;
    std::fs::write(&path, blob).map_err(|e| e.to_string())?;
    Ok(ExportSummary {
        count: 0,
        format,
        encrypted: true,
        path,
    })
}

fn load_entries(app: &AppHandle) -> Result<Vec<ClipboardEntry>, String> {
    let db = app.state::<DbState>();
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let repo = ClipboardRepository::new(&*conn);
    repo.get_history(i32::MAX, 0, None).map_err(|e| e.to_string())
}

fn into_export_entry(entry: ClipboardEntry) -> ExportEntry {
    ExportEntry {
        id: entry.id,
        content_type: entry.content_type,
        content: entry.content,
        preview: Some(entry.preview),
        html_content: entry.html_content,
        source_app: Some(entry.source_app),
        source_app_path: entry.source_app_path,
        created_at: entry.timestamp,
        updated_at: entry.timestamp,
        use_count: entry.use_count,
        is_pinned: entry.is_pinned,
        pinned_order: entry.pinned_order as i32,
        tags: entry.tags,
        ocr_text: entry.ocr_text,
        kinds: entry.content_kinds,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_export_summary_serializes() {
        let summary = ExportSummary {
            count: 3,
            format: "json".to_string(),
            encrypted: false,
            path: "/tmp/test.json".to_string(),
        };
        let json = serde_json::to_string(&summary).expect("serialize");
        assert!(json.contains("\"count\":3"));
        assert!(json.contains("\"format\":\"json\""));
        assert!(json.contains("\"encrypted\":false"));
        assert!(json.contains("\"path\":\"/tmp/test.json\""));
    }

    #[test]
    fn test_export_to_file_json_writes_valid_file() {
        let tmp_path = "/tmp/test_export_cmd.json";
        let _ = std::fs::remove_file(tmp_path);
        let result = export_to_file(
            fake_app_handle(),
            tmp_path.to_string(),
            "json".to_string(),
            None,
        );
        assert!(result.is_ok(), "json export should succeed: {:?}", result.err());
        assert!(std::path::Path::new(tmp_path).exists(), "file should exist");
        let content = std::fs::read_to_string(tmp_path).expect("read file");
        let parsed: serde_json::Value = serde_json::from_str(&content).expect("valid json");
        assert!(parsed.get("version").is_some(), "export file should have version");
        assert!(parsed.get("entries").is_some(), "export file should have entries");
        let _ = std::fs::remove_file(tmp_path);
    }

    #[test]
    fn test_export_to_file_encrypted_requires_passphrase() {
        let result = export_to_file(
            fake_app_handle(),
            "/tmp/test.enc".to_string(),
            "encrypted".to_string(),
            None,
        );
        assert!(result.is_err(), "missing passphrase should error");
        let err_msg = result.unwrap_err();
        assert!(
            err_msg.contains("passphrase is required"),
            "error should mention passphrase requirement, got: {err_msg}"
        );
    }

    #[test]
    fn test_export_to_file_invalid_format() {
        let result = export_to_file(
            fake_app_handle(),
            "/tmp/test.csv".to_string(),
            "csv".to_string(),
            None,
        );
        assert!(result.is_err(), "invalid format should error");
        let err_msg = result.unwrap_err();
        assert!(
            err_msg.contains("format must be 'json' or 'encrypted'"),
            "error should mention valid formats, got: {err_msg}"
        );
    }

    #[test]
    fn test_into_export_entry_maps_fields() {
        let entry = ClipboardEntry {
            id: 1,
            content_type: "text".to_string(),
            content: "hello".to_string(),
            html_content: Some("<b>hi</b>".to_string()),
            source_app: "App".to_string(),
            source_app_path: Some("/app".to_string()),
            timestamp: 1700000000,
            preview: "hello".to_string(),
            is_pinned: true,
            tags: vec!["tag1".to_string()],
            use_count: 5,
            is_external: false,
            pinned_order: 1,
            file_preview_exists: true,
            content_kinds: vec!["text".to_string()],
            ocr_text: Some("ocr".to_string()),
            ocr_status: Some("done".to_string()),
        };
        let exported = into_export_entry(entry);
        assert_eq!(exported.id, 1);
        assert_eq!(exported.created_at, 1700000000);
        assert_eq!(exported.updated_at, 1700000000);
        assert_eq!(exported.pinned_order, 1);
        assert_eq!(exported.kinds, vec!["text".to_string()]);
    }

    /// Build a minimal AppHandle backed by an in-memory SQLite DbState.
    fn fake_app_handle() -> AppHandle {
        use tauri::{App, Manager, Runtime};

        let app = App::new(
            tauri::generate_context!(),
            tauri::Builder::default().setup(|_app| Ok(())),
        )
        .expect("create tauri app");

        let db_state = crate::database::DbState::new_in_memory().expect("in-memory db");
        app.manage(db_state);

        app.app_handle()
    }
}
