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

fn prepare_ocr_image(
    history: &dzc_core::api::HistoryService,
    item_id: i64,
    expected_privacy_epoch: Option<u64>,
) -> Result<
    (
        dzc_core::api::CaptureTicket,
        dzc_core::api::PrivacyReadPermit,
        Vec<u8>,
    ),
    String,
> {
    use dzc_core::api::{PrivacyAccessIntent, SensitiveAccess};
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
    let bytes = history
        .image_bytes_authorized(item_id, &permit, now())
        .map_err(|error| error.to_string())?
        .ok_or("OcrImageRequired")?;
    Ok((ticket, permit, bytes))
}

#[tauri::command]
pub async fn recognize_clipboard_image(
    item_id: i64,
    app: AppHandle,
    expected_privacy_epoch: Option<u64>,
    expected_host_instance_id:Option<String>,
) -> Result<OcrResult, String> {
    let bridge=app.state::<crate::app::ipc_host::DesktopHost>();
    bridge.check_instance(item_id,expected_host_instance_id.as_deref())?;
    if item_id<0 {return Err("SessionOcrUnavailable".into());}
    if bridge.embedded()?.is_none() {return Err("RemoteOcrUnavailable".into());}
    let work=bridge.embedded()?.ok_or("ClipboardHostUnavailable")?.begin_work().map_err(|_|"CaptureWorkUnavailable")?;
    let history = app.state::<DbState>().history_service();
    let now = || chrono::Utc::now().timestamp_millis();
    let (ticket, permit, bytes) = prepare_ocr_image(&history, item_id, expected_privacy_epoch)?;
    let task_work=work.clone();
    let text = tokio::task::spawn_blocking(move || {
        let _work=task_work;
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
    drop(work);
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
                    prepare_ocr_image(&history, id, None).unwrap_err(),
                    "SensitiveAccessDenied"
                );
            } else {
                assert_eq!(
                    prepare_ocr_image(&history, id, None).unwrap().2,
                    *png.get_ref()
                );
            }
            let epoch = history
                .privacy()
                .status(chrono::Utc::now().timestamp_millis())
                .unwrap()
                .privacy_epoch;
            assert_eq!(
                prepare_ocr_image(&history, id, Some(epoch)).unwrap().2,
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
                prepare_ocr_image(&history, id, Some(epoch)).unwrap_err(),
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
                prepare_ocr_image(&history, id, Some(epoch)).unwrap_err(),
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
