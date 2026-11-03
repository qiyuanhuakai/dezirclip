use crate::app::ipc_host::DesktopHost;
use dzc_host::wire::{BackupImportParams, BackupImportResult as ImportSummaryResponse, BackupPreview,
    BackupPreviewParams, BackupPreviewRow, Method, RuntimeState};
use tauri::State;

#[tauri::command]
pub async fn import_from_file(host: State<'_, DesktopHost>, path: String, mode: String, passphrase: Option<String>) -> Result<ImportSummaryResponse, String> {
    let state: RuntimeState = host.call(Method::Health, serde_json::json!({}), "RuntimeState").await?;
    host.backup(Method::DesktopBackupImport, BackupImportParams { service_instance_id: state.service_instance_id,
        expected_epoch: state.privacy_epoch, path, mode, passphrase }, "BackupImportResult").await
}

#[tauri::command]
pub async fn preview_backup_file(host: State<'_, DesktopHost>, path: String) -> Result<Vec<BackupPreviewRow>, String> {
    let state: RuntimeState = host.call(Method::Health, serde_json::json!({}), "RuntimeState").await?;
    let result: BackupPreview = host.backup(Method::DesktopBackupPreview, BackupPreviewParams {
        service_instance_id: state.service_instance_id, expected_epoch: state.privacy_epoch, path
    }, "BackupPreview").await?;
    Ok(result.entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::backup::{
        entries_from_json, export_to_json, import_from_encrypted, BackupError, ExportEntry, ImportMode, ImportSummary,
    };

    fn make_entry(id: i64) -> ExportEntry {
        ExportEntry {
            id,
            content_type: "text/plain".to_string(),
            content: format!("hello-{id}"),
            preview: Some(format!("preview-{id}")),
            html_content: None,
            source_app: Some("TestApp".to_string()),
            source_app_path: None,
            created_at: 1_000_000 + id,
            updated_at: 1_000_000 + id,
            use_count: 0,
            is_pinned: false,
            pinned_order: 0,
            tags: vec![],
            ocr_text: None,
            kinds: vec![],
        }
    }

    #[test]
    fn test_import_summary_serializes() {
        let summary = ImportSummary {
            imported: 5,
            skipped: 1,
            mode: "merge".to_string(),
        };
        let response = ImportSummaryResponse {
            imported: summary.imported as u64,
            skipped: summary.skipped as u64,
            mode: summary.mode.clone(),
        };
        assert_eq!(response.imported, 5);
        assert_eq!(response.skipped, 1);
        assert_eq!(response.mode, "merge");
    }

    #[test]
    fn test_import_from_file_json_roundtrip() {
        let tmp = std::env::temp_dir().join("dezirclip-import-json-roundtrip.json");
        let entries = vec![make_entry(1), make_entry(2)];
        let json = export_to_json(entries.clone()).unwrap();
        std::fs::write(&tmp, json).unwrap();

        let data = std::fs::read(&tmp).unwrap();
        let _ = std::fs::remove_file(&tmp);
        let json_str = std::str::from_utf8(&data).unwrap();
        let parsed = entries_from_json(json_str).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].id, 1);
        assert_eq!(parsed[1].content, "hello-2");
    }

    #[test]
    fn test_import_from_file_replace_clears() {
        let tmp = std::env::temp_dir().join("dezirclip-import-replace.json");
        let entries = vec![make_entry(1), make_entry(2)];
        let json = export_to_json(entries).unwrap();
        std::fs::write(&tmp, json).unwrap();

        let data = std::fs::read(&tmp).unwrap();
        let _ = std::fs::remove_file(&tmp);
        let json_str = std::str::from_utf8(&data).unwrap();
        let parsed = entries_from_json(json_str).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].id, 1);
        assert_eq!(parsed[1].id, 2);
    }

    #[test]
    fn test_import_from_file_merge_preserves() {
        let tmp = std::env::temp_dir().join("dezirclip-import-merge.json");
        let entries = vec![make_entry(10), make_entry(20)];
        let json = export_to_json(entries).unwrap();
        std::fs::write(&tmp, json).unwrap();

        let data = std::fs::read(&tmp).unwrap();
        let _ = std::fs::remove_file(&tmp);
        let json_str = std::str::from_utf8(&data).unwrap();
        let parsed = entries_from_json(json_str).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].id, 10);
        assert_eq!(parsed[1].id, 20);
    }

    #[test]
    fn test_import_from_file_encrypted_wrong_passphrase() {
        let entries = vec![make_entry(1)];
        let blob =
            crate::services::backup::export_to_encrypted(entries, "correct password").unwrap();
        let result = import_from_encrypted(&blob, "wrong password 456");
        assert!(matches!(result, Err(BackupError::WrongPassphrase)));
    }
}
