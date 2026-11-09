use crate::CaptureWork;
use dzc_core::{api::HistoryService,capture::StoredCapture};
use std::sync::Arc;

use dzc_core::api::{CaptureTicket, PrivacyAccessIntent, PrivacyReadPermit, SensitiveAccess};
use dzc_protocol::wire::{EventData, EventKind, ManualOcrParams, ManualOcrText, Method, OcrEngineInfo, WireErrorCode};
use crate::{Host, router::{now_ms, privacy_error}};

#[cfg(target_os = "windows")]
pub const OCR_ENGINE_DISPLAY_NAME: &str = "Windows.Media.Ocr";
#[cfg(target_os = "linux")]
pub const OCR_ENGINE_DISPLAY_NAME: &str = "Tesseract";

pub fn engine_info() -> OcrEngineInfo {
    OcrEngineInfo { available: dzc_platform::ocr::OcrService::new().is_ok(), engine_name: OCR_ENGINE_DISPLAY_NAME.into() }
}

pub fn prepare_manual_ocr(history: &HistoryService, id: i64, epoch: Option<u64>, reveal: bool)
    -> Result<(CaptureTicket, PrivacyReadPermit, Vec<u8>), String> {
    let ticket = history.privacy().capture_ticket(chrono_ms()).map_err(|error| error.to_string())?;
    let access = if reveal && epoch.is_some() { SensitiveAccess::ExplicitUserAction } else { SensitiveAccess::Denied };
    let permit = history.privacy().authorize_selected_copy(id, None, access, Some(epoch.unwrap_or(ticket.epoch())), chrono_ms())
        .map_err(|error| error.to_string())?;
    let bytes = history.image_bytes_authorized(id, &permit, chrono_ms()).map_err(|error| error.to_string())?
        .ok_or("OcrImageRequired")?;
    Ok((ticket, permit, bytes))
}

impl Host {
    pub fn manual_ocr(&self, method: Method, p: ManualOcrParams) -> Result<(PrivacyReadPermit, ManualOcrText), WireErrorCode> {
        if p.id <= 0 || p.service_instance_id != self.instance_id() { return Err(WireErrorCode::InvalidRequest); }
        match method {
            Method::DesktopOcrRecognize => {
                let _operation = self.manual_ocr_operation.try_lock().map_err(|_| WireErrorCode::Busy)?;
                if self.is_stopping() { return Err(WireErrorCode::Busy); }
                let (ticket, permit, bytes) = prepare_manual_ocr(&self.history, p.id, Some(p.expected_epoch), p.reveal)
                    .map_err(|error| if error == "OcrImageRequired" { WireErrorCode::InvalidRequest } else { privacy_error(error) })?;
                let text = dzc_platform::ocr::OcrService::new().map_err(|_| WireErrorCode::UnsupportedCapability)?
                    .recognize(&bytes).map_err(|_| WireErrorCode::Unavailable)?;
                drop(bytes);
                self.history.privacy().check_read(&permit, now_ms()?).map_err(privacy_error)?;
                let changed = self.history.update_ocr_authorized(p.id, &text, "done", &ticket, now_ms()?).map_err(privacy_error)?;
                if changed == 0 { return Err(WireErrorCode::NotFound); }
                self.publish(EventKind::EntryUpdated, EventData { entry_id: Some(p.id), status: Some("done".into()) })?;
                self.history.privacy().check_read(&permit, now_ms()?).map_err(privacy_error)?;
                Ok((permit, ManualOcrText { status: "done".into(), text: Some(text) }))
            }
            Method::DesktopOcrStatus => {
                let permit = self.history.privacy().authorize_read(PrivacyAccessIntent::AutomationRead, SensitiveAccess::Denied,
                    Some(p.expected_epoch), now_ms()?).map_err(privacy_error)?;
                let row = self.history.get_entry_authorized(p.id, &permit, now_ms()?).map_err(privacy_error)?.ok_or(WireErrorCode::SensitiveAccessDenied)?;
                self.history.privacy().check_read(&permit, now_ms()?).map_err(privacy_error)?;
                Ok((permit, ManualOcrText { status: row.ocr_status.unwrap_or_else(|| "pending".into()), text: row.ocr_text }))
            }
            _ => Err(WireErrorCode::InvalidRequest),
        }
    }
}

/// Both frontends use the original capture ticket and the one native OCR
/// implementation. The work token remains in the actual recognition thread.
pub fn schedule_capture_ocr(history:HistoryService,capture:&StoredCapture,work:CaptureWork,
    changed:Arc<dyn Fn(i64,&'static str)+Send+Sync>) -> Result<(),()> {
    let (Some(id),Some(ticket))=(capture.ocr_target,capture.privacy_ticket) else {return Ok(());};
    std::thread::Builder::new().name("dzc-capture-ocr".into()).spawn(move|| {
        let _work=work;
        let now=||chrono_ms();
        let bytes=match history.image_bytes_for_capture_authorized(id,&ticket,now()) {Ok(Some(bytes))=>bytes,_=>return};
        if !history.update_ocr_authorized(id,"","processing",&ticket,now()).is_ok_and(|count|count>0) {return;}
        changed(id,"processing");
        let result=dzc_platform::ocr::OcrService::new().and_then(|service|service.recognize(&bytes));
        let (text,status)=match result {
            Ok(text)=>(text,"done"),
            Err(dzc_platform::ocr::OcrError::NoOcrEngine|dzc_platform::ocr::OcrError::NoLanguages)=>(String::new(),"unsupported"),
            Err(_)=>(String::new(),"failed"),
        };
        if history.update_ocr_authorized(id,&text,status,&ticket,now()).is_ok_and(|count|count>0) {changed(id,status);}
    }).map(|_|()).map_err(|_|())
}
fn chrono_ms()->i64 {std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok().and_then(|value|i64::try_from(value.as_millis()).ok()).unwrap_or(i64::MAX)}
