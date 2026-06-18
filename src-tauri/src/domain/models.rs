use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ClipboardEntry {
    pub id: i64,
    pub content_type: String, // 'text', 'image', 'code', 'file', 'video'
    pub content: String,
    #[serde(default)]
    pub html_content: Option<String>,
    pub source_app: String,
    #[serde(default)]
    pub source_app_path: Option<String>,
    pub timestamp: i64,
    pub preview: String,
    pub is_pinned: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub use_count: i32,
    #[serde(default)]
    pub is_external: bool, // New field to track if content is a file path
    #[serde(default)]
    pub pinned_order: i64,  // For manual sorting of pinned items
    #[serde(default = "default_true")]
    pub file_preview_exists: bool, // Transient field: does the file exist on disk?
    /// Classification result populated by `services::classification::classify()`.
    /// Stored as a JSON array string (e.g., '["text","code"]') in `clipboard_history.content_kinds`
    /// (added in v13). Empty for pre-v13 data; new entries get the value computed at write time.
    #[serde(default)]
    pub content_kinds: Vec<String>,
    /// OCR-extracted text populated by `services::ocr::windows::extract_text()`.
    /// Stored in `clipboard_history.ocr_text` (added in v14). None for pre-v14 data
    /// or non-image entries.
    #[serde(default)]
    pub ocr_text: Option<String>,
    /// OCR processing status: "pending" | "processing" | "done" | "failed" | "unsupported".
    /// Stored in `clipboard_history.ocr_status` (added in v14).
    #[serde(default)]
    pub ocr_status: Option<String>,
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clipboard_entry_serialize_roundtrip() {
        let entry = ClipboardEntry {
            id: 1,
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
            ocr_text: Some("ocr result".to_string()),
            ocr_status: Some("done".to_string()),
        };
        let json = serde_json::to_string(&entry).expect("serialize");
        let parsed: ClipboardEntry = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.id, 1);
        assert_eq!(parsed.content_kinds, vec!["text".to_string()]);
        assert_eq!(parsed.ocr_text, Some("ocr result".to_string()));
        assert_eq!(parsed.ocr_status, Some("done".to_string()));
    }

    #[test]
    fn test_clipboard_entry_missing_new_fields_uses_defaults() {
        let json = r#"{"id":1,"content_type":"text","content":"hello","source_app":"T","timestamp":1,"preview":"h","is_pinned":false}"#;
        let parsed: ClipboardEntry = serde_json::from_str(json).expect("deserialize");
        assert_eq!(parsed.content_kinds, Vec::<String>::new());
        assert_eq!(parsed.ocr_text, None);
        assert_eq!(parsed.ocr_status, None);
    }
}
