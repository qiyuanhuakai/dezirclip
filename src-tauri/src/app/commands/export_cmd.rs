use crate::app::ipc_host::DesktopHost;
use dzc_host::wire::{BackupExportParams, BackupExportResult as ExportSummary, Method, RuntimeState};
use tauri::State;

#[tauri::command]
pub async fn export_to_file(host: State<'_, DesktopHost>, path: String, format: String, passphrase: Option<String>, expected_epoch: Option<u64>) -> Result<ExportSummary, String> {
    let state: RuntimeState = host.call(Method::Health, serde_json::json!({}), "RuntimeState").await?;
    host.blocking_call(Method::DesktopBackupExport, BackupExportParams { service_instance_id: state.service_instance_id,
        expected_epoch: expected_epoch.unwrap_or(state.privacy_epoch), path, format, passphrase }, "BackupExportResult").await
}

#[cfg(test)]
mod tests {
    use super::*;
    use dzc_core::api::{ClipboardEntry, HistoryService, SessionHistoryStore};
    use std::collections::VecDeque;
    use std::path::Path;
    use std::sync::Arc;

    async fn request_export(history: &HistoryService, path: &Path, format: &str) -> Result<ExportSummary, String> {
        let host = DesktopHost::synthetic_fixture(history.clone(), Arc::new(SessionHistoryStore::new(VecDeque::new())));
        let state: RuntimeState = host.call(Method::Health, serde_json::json!({}), "RuntimeState").await?;
        host.blocking_call(Method::DesktopBackupExport, BackupExportParams {
            service_instance_id: state.service_instance_id,
            expected_epoch: state.privacy_epoch,
            path: path.to_string_lossy().into_owned(),
            format: format.into(),
            passphrase: None,
        }, "BackupExportResult").await
    }

    fn seed_entry(history: &HistoryService) {
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
        history.save(&entry, None).expect("seed entry");
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

    #[tokio::test]
    async fn test_export_to_file_json_writes_valid_file() {
        let path = std::env::temp_dir().join(format!("dezirclip-export-{}.json", uuid::Uuid::new_v4()));
        let history = HistoryService::open_desktop(Path::new(":memory:")).unwrap();
        seed_entry(&history);

        let result = request_export(&history, &path, "json").await.unwrap();
        assert_eq!(result.count, 1);
        assert!(!result.encrypted);
        let entries = dzc_core::backup_format::entries_from_json(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(entries[0].content, "hello");
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn test_export_rejects_unknown_format_and_missing_passphrase() {
        let path = std::env::temp_dir().join(format!("dezirclip-export-{}.dzc", uuid::Uuid::new_v4()));
        let history = HistoryService::open_desktop(Path::new(":memory:")).unwrap();
        for format in ["csv", "encrypted"] {
            let err = request_export(&history, &path, format).await.unwrap_err();
            assert_eq!(err, "ClipboardHostUnavailable", "{format}");
        }
        assert!(!path.exists());
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
        let exported = dzc_core::backup_format::prepare_export_entry(&entry).unwrap();
        assert_eq!(exported.id, 1);
        assert_eq!(exported.created_at, 1700000000);
        assert_eq!(exported.updated_at, 1700000000);
        assert_eq!(exported.pinned_order, 1);
        assert_eq!(exported.kinds, vec!["text".to_string()]);
    }
}
