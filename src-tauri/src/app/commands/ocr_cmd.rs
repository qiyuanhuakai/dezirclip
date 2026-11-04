use serde::{Deserialize, Serialize};
use tauri::Emitter;
use tauri::Manager;
use tauri::{AppHandle, State};

use crate::app::ipc_host::DesktopHost;
use dzc_host::wire::{Method, OcrEngineInfo};
#[cfg(test)]
use dzc_host::OCR_ENGINE_DISPLAY_NAME;

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
pub async fn recognize_clipboard_image(item_id: i64, app: AppHandle, expected_privacy_epoch: Option<u64>,
    expected_host_instance_id: Option<String>) -> Result<OcrResult, String> {
    let output = app.state::<DesktopHost>().manual_ocr(Method::DesktopOcrRecognize, item_id,
        expected_host_instance_id.as_deref(), expected_privacy_epoch).await?;
    let _ = app.emit("ocr:complete", OcrResult { item_id, text: String::new(), confidence: None, status: output.status.clone() });
    Ok(OcrResult { item_id, text: output.text.ok_or("OcrRecognitionFailed")?, confidence: None, status: output.status })
}

#[tauri::command]
pub async fn get_ocr_status(host: State<'_, DesktopHost>, item_id: i64) -> Result<OcrStatusResponse, String> {
    let row = host.manual_ocr(Method::DesktopOcrStatus, item_id, None, None).await?;
    Ok(OcrStatusResponse { item_id, ocr_status: row.status, ocr_text: row.text })
}

#[tauri::command]
pub fn check_ocr_engine_available() -> OcrEngineInfo {
    dzc_host::engine_info()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(all(target_os = "windows", not(feature = "portable")))]
    #[test]
    fn manual_ocr_command_preparation_accepts_private_images_and_rejects_stale_epoch() {
        use base64::Engine;
        use dzc_core::api::{HistoryService, PrivacySessionCommand};
        dzc_platform::install_default_key_provider();
        let history = HistoryService::open(std::path::Path::new(":memory:")).unwrap();
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(1, 1)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let content = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(png.get_ref())
        );
        for sensitive in [false, true] {
            let entry = serde_json::from_value(serde_json::json!({"id":0,"content_type":"image","content":content,"preview":"synthetic","source_app":"Synthetic OCR","timestamp":1,"is_pinned":false,"tags":if sensitive {vec!["sensitive"]} else {vec![]}})).unwrap();
            let id = history.save(&entry, None).unwrap();
            if sensitive {
                assert_eq!(
                    dzc_host::prepare_manual_ocr(&history, id, None, false).unwrap_err(),
                    "SensitiveAccessDenied"
                );
            } else {
                assert_eq!(
                    dzc_host::prepare_manual_ocr(&history, id, None, false).unwrap().2,
                    *png.get_ref()
                );
            }
            let epoch = history
                .privacy()
                .status(chrono::Utc::now().timestamp_millis())
                .unwrap()
                .privacy_epoch;
            assert_eq!(
                dzc_host::prepare_manual_ocr(&history, id, Some(epoch), true).unwrap().2,
                *png.get_ref()
            );
            history
                .privacy()
                .set_session(
                    PrivacySessionCommand::On { duration_ms: None },
                    chrono::Utc::now().timestamp_millis(),
                )
                .unwrap();
            assert_eq!(
                dzc_host::prepare_manual_ocr(&history, id, Some(epoch), true).unwrap_err(),
                "PrivacySessionActive"
            );
            history
                .privacy()
                .set_session(
                    PrivacySessionCommand::Off,
                    chrono::Utc::now().timestamp_millis(),
                )
                .unwrap();
            assert_eq!(
                dzc_host::prepare_manual_ocr(&history, id, Some(epoch), true).unwrap_err(),
                "PrivacyEpochChanged"
            );
        }
    }

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
