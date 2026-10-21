use serde::{Deserialize, Serialize};
use tauri::Emitter;
use tauri::Manager;
use tauri::{AppHandle, State};

use crate::database::DbState;

#[cfg(target_os = "windows")]
const OCR_ENGINE_DISPLAY_NAME: &str = "Windows.Media.Ocr";
#[cfg(not(target_os = "windows"))]
const OCR_ENGINE_DISPLAY_NAME: &str = "Tesseract";

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
pub async fn recognize_clipboard_image(
    item_id: i64,
    app: AppHandle,
    expected_privacy_epoch: Option<u64>,
) -> Result<OcrResult, String> {
    use dzc_core::api::{PrivacyAccessIntent, SensitiveAccess};
    let history = app.state::<DbState>().history_service();
    let now = || chrono::Utc::now().timestamp_millis();
    let ticket = history
        .privacy()
        .capture_ticket(now())
        .map_err(|error| error.to_string())?;
    let permit = history
        .privacy()
        .authorize_read(
            PrivacyAccessIntent::SelectedCopy,
            if expected_privacy_epoch.is_some() {
                SensitiveAccess::ExplicitUserAction
            } else {
                SensitiveAccess::Denied
            },
            Some(expected_privacy_epoch.unwrap_or(ticket.epoch())),
            now(),
        )
        .map_err(|error| error.to_string())?;
    let dzc_core::ports::ClipboardPayload::Image { bytes, .. } = history
        .clipboard_payload_authorized(item_id, &permit, now())
        .map_err(|error| error.to_string())?
    else {
        return Err("OcrImageRequired".into());
    };
    let text = tokio::task::spawn_blocking(move || {
        crate::services::ocr::OcrService::new()
            .map_err(|_| "OcrEngineUnavailable".to_owned())?
            .recognize(&bytes)
            .map_err(|_| "OcrRecognitionFailed".to_owned())
    })
    .await
    .map_err(|_| "OcrRecognitionFailed")??;
    history
        .update_ocr_authorized(item_id, &text, "done", &ticket, now())
        .map_err(|error| error.to_string())?;
    history
        .privacy()
        .check_read(&permit, now())
        .map_err(|error| error.to_string())?;
    let result = OcrResult {
        item_id,
        text,
        confidence: None,
        status: "done".into(),
    };
    let _ = app.emit(
        "ocr:complete",
        OcrResult {
            item_id,
            text: String::new(),
            confidence: None,
            status: "done".into(),
        },
    );
    Ok(result)
}

#[tauri::command]
pub fn get_ocr_status(
    state: State<'_, DbState>,
    item_id: i64,
) -> Result<OcrStatusResponse, String> {
    use dzc_core::api::{PrivacyAccessIntent, SensitiveAccess};
    let now = || chrono::Utc::now().timestamp_millis();
    let history = state.history_service();
    let permit = history
        .privacy()
        .authorize_read(
            PrivacyAccessIntent::AutomationRead,
            SensitiveAccess::Denied,
            None,
            now(),
        )
        .map_err(|error| error.to_string())?;
    let entry = history
        .get_entry_authorized(item_id, &permit, now())
        .map_err(|error| error.to_string())?
        .ok_or("SensitiveAccessDenied")?;
    Ok(OcrStatusResponse {
        item_id,
        ocr_status: entry.ocr_status.unwrap_or_else(|| "pending".into()),
        ocr_text: entry.ocr_text,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrEngineInfo {
    pub available: bool,
    pub engine_name: String,
}

#[tauri::command]
pub fn check_ocr_engine_available() -> OcrEngineInfo {
    let available = crate::services::ocr::OcrService::new().is_ok();
    OcrEngineInfo {
        available,
        engine_name: OCR_ENGINE_DISPLAY_NAME.to_string(),
    }
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

    #[test]
    fn test_ocr_engine_info_serializes_both_fields() {
        let info = OcrEngineInfo {
            available: true,
            engine_name: "Tesseract".to_string(),
        };
        let json = serde_json::to_string(&info).expect("serialize");
        let parsed: OcrEngineInfo = serde_json::from_str(&json).expect("deserialize");
        assert!(parsed.available);
        assert_eq!(parsed.engine_name, "Tesseract");
    }

    #[test]
    fn test_ocr_engine_info_serializes_unavailable() {
        let info = OcrEngineInfo {
            available: false,
            engine_name: "Windows.Media.Ocr".to_string(),
        };
        let json = serde_json::to_string(&info).expect("serialize");
        assert!(
            json.contains("\"available\":false"),
            "available false must serialize as JSON false, got: {json}"
        );
        assert!(
            json.contains("\"engine_name\":\"Windows.Media.Ocr\""),
            "engine_name must serialize verbatim, got: {json}"
        );
    }

    #[test]
    fn test_ocr_engine_display_name_matches_platform() {
        #[cfg(target_os = "windows")]
        assert_eq!(OCR_ENGINE_DISPLAY_NAME, "Windows.Media.Ocr");
        #[cfg(not(target_os = "windows"))]
        assert_eq!(OCR_ENGINE_DISPLAY_NAME, "Tesseract");
    }
}
