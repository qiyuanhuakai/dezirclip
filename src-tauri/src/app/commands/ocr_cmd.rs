use serde::{Deserialize, Serialize};
#[cfg(target_os = "windows")]
use tauri::Manager;
use tauri::{AppHandle, Emitter, State};

use crate::database::DbState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrResult {
    pub item_id: i64,
    pub text: String,
    pub confidence: Option<f32>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrStatusResponse {
    pub item_id: i64,
    pub ocr_status: String,
    pub ocr_text: Option<String>,
}

#[tauri::command]
#[cfg_attr(
    not(target_os = "windows"),
    allow(
        unused_variables,
        reason = "Linux returns early; params reserved for Windows path"
    )
)]
pub async fn recognize_clipboard_image(item_id: i64, app: AppHandle) -> Result<OcrResult, String> {
    #[cfg(target_os = "linux")]
    {
        return Err(
            "OCR is not supported on this platform (Linux OCR engine not available)".to_string(),
        );
    }

    #[cfg(target_os = "windows")]
    {
        let db_state = app.state::<DbState>();
        let conn = db_state
            .conn
            .lock()
            .map_err(|e| format!("DB lock failed: {e}"))?;

        let entry = db_state
            .repo
            .get_entry_by_id_with_conn(&conn, item_id)?
            .ok_or_else(|| format!("Clipboard entry {item_id} not found"))?;

        if entry.content_type != "image" {
            return Err(format!(
                "Entry {item_id} is not an image (content_type={})",
                entry.content_type
            ));
        }

        let png_bytes = crate::services::clipboard_ops::resolve_image_bytes(&entry.content)
            .ok_or_else(|| {
                format!(
                    "Failed to resolve image bytes for entry {item_id} (content_len={})",
                    entry.content.len()
                )
            })?;

        let text = {
            let service = crate::services::ocr::OcrService::new()
                .map_err(|e| format!("OCR engine init failed: {e}"))?;
            service
                .recognize(&png_bytes)
                .map_err(|e| format!("OCR recognition failed: {e}"))?
        };

        db_state
            .repo
            .update_ocr_text_with_conn(&conn, item_id, &text, "done")
            .map_err(|e| format!("Failed to persist OCR text: {e}"))?;

        drop(conn);

        let result = OcrResult {
            item_id,
            text: text.clone(),
            confidence: None,
            status: "done".to_string(),
        };

        let _ = app.emit("ocr:complete", &result);

        Ok(result)
    }
}

#[tauri::command]
pub fn get_ocr_status(
    state: State<'_, DbState>,
    item_id: i64,
) -> Result<OcrStatusResponse, String> {
    let conn = state
        .conn
        .lock()
        .map_err(|e| format!("DB lock failed: {e}"))?;
    let (ocr_status, ocr_text) = state
        .repo
        .get_ocr_status_with_conn(&conn, item_id)?
        .ok_or_else(|| format!("Clipboard entry {item_id} not found"))?;
    Ok(OcrStatusResponse {
        item_id,
        ocr_status,
        ocr_text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ocr_result_serialize_roundtrip() {
        let result = OcrResult {
            item_id: 1,
            text: "Hello World".to_string(),
            confidence: Some(0.95),
            status: "done".to_string(),
        };
        let json = serde_json::to_string(&result).expect("serialize");
        let parsed: OcrResult = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.item_id, 1);
        assert_eq!(parsed.text, "Hello World");
        assert_eq!(parsed.confidence, Some(0.95));
        assert_eq!(parsed.status, "done");
    }

    #[test]
    fn test_ocr_status_response_serialize_roundtrip() {
        let resp = OcrStatusResponse {
            item_id: 42,
            ocr_status: "done".to_string(),
            ocr_text: Some("extracted text".to_string()),
        };
        let json = serde_json::to_string(&resp).expect("serialize");
        let parsed: OcrStatusResponse = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.item_id, 42);
        assert_eq!(parsed.ocr_status, "done");
        assert_eq!(parsed.ocr_text, Some("extracted text".to_string()));
    }

    #[test]
    fn test_ocr_result_none_confidence_serializes_as_null() {
        let result = OcrResult {
            item_id: 2,
            text: "x".to_string(),
            confidence: None,
            status: "failed".to_string(),
        };
        let json = serde_json::to_string(&result).expect("serialize");
        assert!(
            json.contains("\"confidence\":null"),
            "confidence None must serialize as JSON null, got: {json}"
        );
        assert!(json.contains("\"status\":\"failed\""));
    }

    #[test]
    fn test_ocr_status_response_none_text_serializes_as_null() {
        let resp = OcrStatusResponse {
            item_id: 1,
            ocr_status: "pending".to_string(),
            ocr_text: None,
        };
        let json = serde_json::to_string(&resp).expect("serialize");
        assert!(
            json.contains("\"ocr_text\":null"),
            "ocr_text None must serialize as JSON null, got: {json}"
        );
    }
}
