use crate::database::DbState;
use crate::services::backup::{
    decrypt_to_json, entries_from_json, import_from_json, ExportEntry, ImportMode,
};
use tauri::{AppHandle, Manager};

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

    let (entries, summary) = if looks_encrypted(&data) {
        let passphrase =
            passphrase.ok_or_else(|| "passphrase is required for encrypted files".to_string())?;
        if passphrase.is_empty() {
            return Err("passphrase must not be empty".to_string());
        }
        let json = decrypt_to_json(&data, &passphrase).map_err(|e| e.to_string())?;
        let entries = entries_from_json(&json).map_err(|e| e.to_string())?;
        let summary = import_from_json(&json, import_mode).map_err(|e| e.to_string())?;
        (entries, summary)
    } else {
        let json =
            std::str::from_utf8(&data).map_err(|e| format!("file is not valid UTF-8: {e}"))?;
        let entries = entries_from_json(json).map_err(|e| e.to_string())?;
        let summary = import_from_json(json, import_mode).map_err(|e| e.to_string())?;
        (entries, summary)
    };

    apply_import(app, import_mode, &entries)?;

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

fn apply_import(app: AppHandle, mode: ImportMode, entries: &[ExportEntry]) -> Result<(), String> {
    let db = app.state::<DbState>();
    crate::services::backup::apply_entries(&db.repo, entries, mode)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::backup::{
        export_to_json, import_from_encrypted, BackupError, ExportEntry, ImportSummary,
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
            imported: summary.imported,
            skipped: summary.skipped,
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
