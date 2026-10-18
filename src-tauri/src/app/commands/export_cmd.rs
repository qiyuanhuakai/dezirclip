use crate::database::DbState;
use crate::domain::models::ClipboardEntry;
use crate::infrastructure::repository::cli_query::CliQuery;
use crate::services::backup::{export_to_encrypted, export_to_json, ExportEntry};
use tauri::{AppHandle, State};

#[derive(Debug, serde::Serialize)]
pub struct ExportSummary {
    pub count: usize,
    pub format: String,
    pub encrypted: bool,
    pub path: String,
}

#[tauri::command]
pub fn export_to_file(
    _app: AppHandle,
    state: State<'_, DbState>,
    path: String,
    format: String,
    passphrase: Option<String>,
) -> Result<ExportSummary, String> {
    state
        .repo
        .ensure_export_destination(std::path::Path::new(&path))?;
    match format.as_str() {
        "json" => export_json(state, path, format),
        "encrypted" => export_encrypted(state, path, format, passphrase),
        _ => Err("format must be 'json' or 'encrypted'".to_string()),
    }
}

fn export_json(
    state: State<'_, DbState>,
    path: String,
    format: String,
) -> Result<ExportSummary, String> {
    let entries = state
        .repo
        .query_cli_for_export(&CliQuery {
            limit: i32::MAX,
            ..Default::default()
        })
        .map_err(|e| e.to_string())?;
    export_entries(entries, path, format, None)
}

fn export_encrypted(
    state: State<'_, DbState>,
    path: String,
    format: String,
    passphrase: Option<String>,
) -> Result<ExportSummary, String> {
    let entries = state
        .repo
        .query_cli_for_export(&CliQuery {
            limit: i32::MAX,
            ..Default::default()
        })
        .map_err(|e| e.to_string())?;
    export_entries(entries, path, format, passphrase)
}

/// Pure helper: serialize `entries` and write to `path`.
pub(crate) fn export_entries(
    entries: Vec<ClipboardEntry>,
    path: String,
    format: String,
    passphrase: Option<String>,
) -> Result<ExportSummary, String> {
    let count = entries.len();
    let export_entries: Vec<ExportEntry> = entries
        .iter()
        .map(crate::services::backup::prepare_export_entry)
        .collect::<Result<_, _>>()?;
    match format.as_str() {
        "json" => {
            let json = export_to_json(export_entries).map_err(|e| e.to_string())?;
            crate::infrastructure::attachments::write_backup(
                std::path::Path::new(&path),
                json.as_bytes(),
            )?;
            Ok(ExportSummary {
                count,
                format,
                encrypted: false,
                path,
            })
        }
        "encrypted" => {
            let passphrase = passphrase
                .ok_or_else(|| "passphrase is required for encrypted format".to_string())?;
            if passphrase.is_empty() {
                return Err("passphrase must not be empty".to_string());
            }
            let blob =
                export_to_encrypted(export_entries, &passphrase).map_err(|e| e.to_string())?;
            crate::infrastructure::attachments::write_backup(std::path::Path::new(&path), &blob)?;
            Ok(ExportSummary {
                count,
                format,
                encrypted: true,
                path,
            })
        }
        _ => Err("format must be 'json' or 'encrypted'".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::repository::clipboard_repo::ClipboardRepository;
    use crate::infrastructure::repository::clipboard_repo::SqliteClipboardRepository;
    use std::sync::{Arc, Mutex};

    fn setup_test_repo() -> SqliteClipboardRepository {
        let mut conn = rusqlite::Connection::open_in_memory().expect("open in-memory db");
        crate::infrastructure::repository::migrations::run_migrations(&mut conn)
            .expect("run migrations");
        SqliteClipboardRepository::new(Arc::new(Mutex::new(conn)))
    }

    fn seed_entry(repo: &SqliteClipboardRepository) {
        let entry = ClipboardEntry {
            id: 0,
            content_type: "text".to_string(),
            content: "hello".to_string(),
            html_content: None,
            source_app: "TestApp".to_string(),
            source_app_path: None,
            timestamp: 1700000000,
            preview: "hello".to_string(),
            is_pinned: false,
            tags: vec![],
            use_count: 0,
            is_external: false,
            pinned_order: 0,
            file_preview_exists: true,
            content_kinds: vec!["text".to_string()],
            ocr_text: None,
            ocr_status: None,
        };
        repo.save(&entry, None).expect("seed entry");
    }

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
        let tmp_path =
            std::env::temp_dir().join(format!("dezirclip-export-cmd-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&tmp_path);
        let repo = setup_test_repo();
        seed_entry(&repo);

        let entries = repo
            .query_cli(&CliQuery {
                limit: i32::MAX,
                ..Default::default()
            })
            .expect("load history");
        let result = export_entries(
            entries,
            tmp_path.to_string_lossy().to_string(),
            "json".to_string(),
            None,
        );
        assert!(
            result.is_ok(),
            "json export should succeed: {:?}",
            result.err()
        );
        assert!(tmp_path.exists(), "file should exist");
        let content = std::fs::read_to_string(&tmp_path).expect("read file");
        let parsed: serde_json::Value = serde_json::from_str(&content).expect("valid json");
        assert!(
            parsed.get("version").is_some(),
            "export file should have version"
        );
        assert!(
            parsed.get("entries").is_some(),
            "export file should have entries"
        );
        let _ = std::fs::remove_file(tmp_path);
    }

    #[test]
    fn test_export_to_file_encrypted_requires_passphrase() {
        let repo = setup_test_repo();
        seed_entry(&repo);
        let entries = repo
            .query_cli(&CliQuery {
                limit: i32::MAX,
                ..Default::default()
            })
            .expect("load history");

        let result = export_entries(
            entries,
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
        let result = export_entries(
            Vec::new(),
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
        let exported = crate::services::backup::prepare_export_entry(&entry).unwrap();
        assert_eq!(exported.id, 1);
        assert_eq!(exported.created_at, 1700000000);
        assert_eq!(exported.updated_at, 1700000000);
        assert_eq!(exported.pinned_order, 1);
        assert_eq!(exported.kinds, vec!["text".to_string()]);
    }
}
