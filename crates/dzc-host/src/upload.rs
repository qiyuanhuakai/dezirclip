//! Generated actions retain an opaque original Core ticket before any body is produced.
use crate::{Host, Session};
use dzc_core::{api::{CaptureTicket, PrivacyError}, capture::{CaptureData, CaptureFrame, CaptureReadMetadata, CaptureSource}};
use dzc_protocol::wire::*;
use base64::Engine;
use serde_json::Value;
use std::{sync::{Arc,Mutex},time::{Duration,Instant}};

struct Upload { id:String, owner:String, ticket:CaptureTicket, timestamp:i64, expires:Instant, bytes:Vec<u8> }
#[derive(Default)]
pub(crate) struct Uploads(Arc<Mutex<Vec<Upload>>>);
impl Uploads {
    pub(crate) fn clear(&self) -> Result<(),WireErrorCode> { self.0.lock().map_err(|_|WireErrorCode::Unavailable)?.clear(); Ok(()) }
}
impl Host {
    pub(crate) fn upload(&self, session:&Session, method:Method, parameters:Value) -> Result<Value,WireErrorCode> {
        session.require(Permission::DesktopHistory)?;
        if session.role()!=ClientRole::Desktop {return Err(WireErrorCode::PermissionDenied);}
        let now=crate::router::now_ms()?;
        let mut uploads=self.uploads.0.lock().map_err(|_|WireErrorCode::Unavailable)?;
        let epoch=self.snapshot()?.privacy_epoch;
        uploads.retain(|upload|upload.expires>Instant::now() && upload.ticket.epoch()==epoch);
        if method==Method::DesktopCaptureBegin {
            let request:CaptureBeginParams=serde_json::from_value(parameters).map_err(|_|WireErrorCode::InvalidRequest)?;
            let state=self.snapshot()?;
            if request.service_instance_id!=state.service_instance_id || request.expected_epoch!=state.privacy_epoch {return Err(WireErrorCode::PrivacyBlocked);}
            if uploads.len()>=4 {return Err(WireErrorCode::Busy);}
            let ticket=match self.history.privacy().capture_ticket(now) {
                Ok(ticket)=>ticket,
                Err(PrivacyError::PrivacySessionActive)=>return serde_json::to_value(CaptureBeginResult{capture_id:None,state}).map_err(|_|WireErrorCode::Internal),
                Err(_)=>return Err(WireErrorCode::PrivacyBlocked),
            };
            let id=uuid::Uuid::new_v4().simple().to_string();
            if ticket.epoch()!=request.expected_epoch {return Err(WireErrorCode::PrivacyBlocked);}
            uploads.push(Upload{id:id.clone(),owner:session.id().into(),ticket,timestamp:now,expires:Instant::now()+Duration::from_secs(5),bytes:Vec::new()});
            let storage=self.uploads.0.clone();let expired_id=id.clone();
            if std::thread::Builder::new().name("dzc-upload-expiry".into()).spawn(move|| {
                std::thread::sleep(Duration::from_secs(5));
                if let Ok(mut uploads)=storage.lock() {uploads.retain(|upload|upload.id!=expired_id);}
            }).is_err() {uploads.retain(|upload|upload.id!=id);return Err(WireErrorCode::Unavailable);}
            return serde_json::to_value(CaptureBeginResult{capture_id:Some(id),state}).map_err(|_|WireErrorCode::Internal);
        }
        if method==Method::DesktopCaptureWrite {
            let request:CaptureWriteParams=serde_json::from_value(parameters).map_err(|_|WireErrorCode::InvalidRequest)?;
            let total=uploads.iter().map(|upload|upload.bytes.len()).sum::<usize>();
            let upload=uploads.iter_mut().find(|upload|upload.id==request.capture_id && upload.owner==session.id()).ok_or(WireErrorCode::NotFound)?;
            self.history.privacy().check_capture(&upload.ticket,now).map_err(|_|WireErrorCode::PrivacyBlocked)?;
            if request.offset as usize!=upload.bytes.len() {return Err(WireErrorCode::InvalidRequest);}
            let bytes=base64::engine::general_purpose::STANDARD.decode(request.data_base64).map_err(|_|WireErrorCode::InvalidRequest)?;
            if bytes.len()>192*1024 || total.saturating_add(bytes.len())>32*1024*1024 {return Err(WireErrorCode::FrameTooLarge);}
            upload.bytes.extend(bytes);
            return serde_json::to_value(OperationResult{performed:true}).map_err(|_|WireErrorCode::Internal);
        }
        let request:CaptureFinishParams=serde_json::from_value(parameters).map_err(|_|WireErrorCode::InvalidRequest)?;
        let index=uploads.iter().position(|upload|upload.id==request.capture_id && upload.owner==session.id()).ok_or(WireErrorCode::NotFound)?;
        let upload=uploads.remove(index);
        drop(uploads);
        self.history.privacy().check_capture(&upload.ticket,now).map_err(|_|WireErrorCode::PrivacyBlocked)?;
        let value:Value=serde_json::from_slice(&upload.bytes).map_err(|_|WireErrorCode::InvalidRequest)?;
        dzc_protocol::wire::validate("GeneratedBody",&value).map_err(|_|WireErrorCode::InvalidRequest)?;
        let body:GeneratedBody=serde_json::from_value(value).map_err(|_|WireErrorCode::InvalidRequest)?;
        let data=match body.kind.as_str() {
            "text"=>CaptureData::Text(body.text),
            "html"=>CaptureData::RichText{text:body.text,html:body.html.ok_or(WireErrorCode::InvalidRequest)?},
            "image"=>CaptureData::Image{data_url:body.text},
            _=>return Err(WireErrorCode::InvalidRequest),
        };
        let frame=CaptureFrame{data,metadata:CaptureReadMetadata{privacy_ticket:Some(upload.ticket),inherited_protection:request.inherited_protection,..Default::default()}};
        let lifecycle=self.lifecycle.lock().map_err(|_|WireErrorCode::Unavailable)?.clone().ok_or(WireErrorCode::Unavailable)?;
        let performed=lifecycle.capture_generated(frame,CaptureSource{app_name:request.source_app,process_path:None},upload.timestamp)?;
        serde_json::to_value(OperationResult{performed}).map_err(|_|WireErrorCode::Internal)
    }
}
