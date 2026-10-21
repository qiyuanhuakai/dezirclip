use crate::CaptureWork;
use dzc_core::{api::HistoryService,capture::StoredCapture};
use std::sync::Arc;

/// Both frontends use the original capture ticket and the one native OCR
/// implementation. The work token remains in the actual recognition thread.
pub fn schedule_capture_ocr(history:HistoryService,capture:&StoredCapture,work:CaptureWork,
    changed:Arc<dyn Fn(i64,&'static str)+Send+Sync>) -> Result<(),()> {
    let (Some(id),Some(ticket))=(capture.ocr_target,capture.privacy_ticket) else {return Ok(());};
    std::thread::Builder::new().name("dzc-capture-ocr".into()).spawn(move|| {
        let _work=work;
        let now=||chrono_ms();
        let bytes=match history.image_bytes_for_capture_authorized(id,&ticket,now()) {Ok(Some(bytes))=>bytes,_=>return};
        if history.update_ocr_authorized(id,"","processing",&ticket,now()).is_err() {return;}
        changed(id,"processing");
        let result=dzc_platform::ocr::OcrService::new().and_then(|service|service.recognize(&bytes));
        let (text,status)=match result {
            Ok(text)=>(text,"done"),
            Err(dzc_platform::ocr::OcrError::NoOcrEngine|dzc_platform::ocr::OcrError::NoLanguages)=>(String::new(),"unsupported"),
            Err(_)=>(String::new(),"failed"),
        };
        if history.update_ocr_authorized(id,&text,status,&ticket,now()).is_ok() {changed(id,status);}
    }).map(|_|()).map_err(|_|())
}
fn chrono_ms()->i64 {std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok().and_then(|value|i64::try_from(value.as_millis()).ok()).unwrap_or(i64::MAX)}
