//! Generated actions retain an opaque original Core ticket before any body is produced.
use crate::{Host, Session};
use dzc_core::{api::{CaptureTicket, PrivacyError, PrivacyReadPermit, PrivacyAccessIntent, SensitiveAccess}, capture::{CaptureData, CaptureFrame, CaptureReadMetadata, CaptureSource}};
use dzc_protocol::wire::*;
use base64::Engine;
use serde_json::Value;
use std::{sync::{Arc,Mutex},time::{Duration,Instant}};

struct Upload { id:String, owner:String, ticket:Option<CaptureTicket>, copy_permit:Option<PrivacyReadPermit>, epoch:u64, timestamp:i64, expires:Instant, bytes:Vec<u8> }
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
        uploads.retain(|upload|upload.expires>Instant::now() && upload.epoch==epoch);
        if method==Method::DesktopCaptureBegin {
            let request:CaptureBeginParams=serde_json::from_value(parameters).map_err(|_|WireErrorCode::InvalidRequest)?;
            let state=self.snapshot()?;
            if request.service_instance_id!=state.service_instance_id || request.expected_epoch!=state.privacy_epoch {return Err(WireErrorCode::PrivacyBlocked);}
            if uploads.len()>=4 {return Err(WireErrorCode::Busy);}
            let copying=request.copy_to_clipboard.unwrap_or(false);
            if copying && !self.actions.can_copy() {return Err(WireErrorCode::UnsupportedCapability);}
            let copy_permit=if copying {Some(self.history.privacy().authorize_read(PrivacyAccessIntent::SelectedCopy,SensitiveAccess::Denied,Some(request.expected_epoch),now).map_err(crate::router::privacy_error)?)}else{None};
            let ticket=match self.history.privacy().capture_ticket(now) {
                Ok(ticket)=>Some(ticket),
                Err(PrivacyError::PrivacySessionActive) if copying=>None,
                Err(PrivacyError::PrivacySessionActive)=>return serde_json::to_value(CaptureBeginResult{capture_id:None,state}).map_err(|_|WireErrorCode::Internal),
                Err(_)=>return Err(WireErrorCode::PrivacyBlocked),
            };
            let id=uuid::Uuid::new_v4().simple().to_string();
            if ticket.as_ref().is_some_and(|ticket|ticket.epoch()!=request.expected_epoch) {return Err(WireErrorCode::PrivacyBlocked);}
            uploads.push(Upload{id:id.clone(),owner:session.id().into(),ticket,copy_permit,epoch:request.expected_epoch,timestamp:now,expires:Instant::now()+Duration::from_secs(5),bytes:Vec::new()});
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
            self.check_upload(upload,now)?;
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
        self.check_upload(&upload,now)?;
        let value:Value=serde_json::from_slice(&upload.bytes).map_err(|_|WireErrorCode::InvalidRequest)?;
        dzc_protocol::wire::validate("GeneratedBody",&value).map_err(|_|WireErrorCode::InvalidRequest)?;
        let body:GeneratedBody=serde_json::from_value(value).map_err(|_|WireErrorCode::InvalidRequest)?;
        if let Some(permit)=upload.copy_permit {
            if request.inherited_protection {return Err(WireErrorCode::InvalidRequest);}
            if request.paste==Some(true) && !self.actions.can_paste() {return Err(WireErrorCode::UnsupportedCapability);}
            // This entry contains only the caller's new payload. It is never
            // resolved as a history/session ID or allowed to name a local file.
            if body.kind=="image" && !body.text.starts_with("data:image/") {return Err(WireErrorCode::InvalidRequest);}
            let entry=dzc_core::api::ClipboardEntry {id:-1,content_type:body.kind.clone(),content:body.text.clone(),html_content:body.html.clone(),
                source_app:request.source_app.clone(),source_app_path:None,timestamp:upload.timestamp,preview:String::new(),is_pinned:false,tags:vec![],
                use_count:0,is_external:false,pinned_order:0,file_preview_exists:true,content_kinds:vec![],ocr_text:None,ocr_status:None};
            let payload=self.history.session_clipboard_payload_authorized(&entry,&permit,now).map_err(crate::router::privacy_error)?;
            if self.capture_owner.load(std::sync::atomic::Ordering::Acquire) {
                if let Some(ticket)=upload.ticket {
                    let frame=CaptureFrame{data:generated_data(body)?,metadata:CaptureReadMetadata{privacy_ticket:Some(ticket),..Default::default()}};
                    self.capture_generated(frame,CaptureSource{app_name:request.source_app,process_path:None},upload.timestamp)?;
                }
            }
            self.history.privacy().check_read(&permit,crate::router::now_ms()?).map_err(crate::router::privacy_error)?;
            self.copy_payload(payload)?;
            if request.paste==Some(true) {
                self.history.privacy().check_read(&permit,crate::router::now_ms()?).map_err(crate::router::privacy_error)?;
                self.actions.paste()?;
            }
            return serde_json::to_value(OperationResult{performed:true}).map_err(|_|WireErrorCode::Internal);
        }
        if request.paste==Some(true) {return Err(WireErrorCode::InvalidRequest);}
        let data=generated_data(body)?;
        let ticket=upload.ticket.ok_or(WireErrorCode::PrivacyBlocked)?;
        let frame=CaptureFrame{data,metadata:CaptureReadMetadata{privacy_ticket:Some(ticket),inherited_protection:request.inherited_protection,..Default::default()}};
        let lifecycle=self.lifecycle.lock().map_err(|_|WireErrorCode::Unavailable)?.clone().ok_or(WireErrorCode::Unavailable)?;
        let performed=lifecycle.capture_generated(frame,CaptureSource{app_name:request.source_app,process_path:None},upload.timestamp)?;
        serde_json::to_value(OperationResult{performed}).map_err(|_|WireErrorCode::Internal)
    }
    fn check_upload(&self,upload:&Upload,now:i64)->Result<(),WireErrorCode> {
        if let Some(ticket)=&upload.ticket {self.history.privacy().check_capture(ticket,now).map_err(|_|WireErrorCode::PrivacyBlocked)?;}
        if let Some(permit)=&upload.copy_permit {self.history.privacy().check_read(permit,now).map_err(crate::router::privacy_error)?;}
        Ok(())
    }
}
fn generated_data(body:GeneratedBody)->Result<CaptureData,WireErrorCode> {
        let data=match body.kind.as_str() {
            "text"=>CaptureData::Text(body.text),
            "html"=>CaptureData::RichText{text:body.text,html:body.html.ok_or(WireErrorCode::InvalidRequest)?},
            "image"=>CaptureData::Image{data_url:body.text},
            _=>return Err(WireErrorCode::InvalidRequest),
        };
        Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HostActions,HostOptions};
    use dzc_core::ports::ClipboardPayload;
    #[derive(Default)]
    struct Actions {copied:Mutex<Vec<String>>,pastes:std::sync::atomic::AtomicUsize}
    impl HostActions for Actions {
        fn platform_capabilities(&self)->Value {serde_json::json!({"synthetic":true})}
        fn can_copy(&self)->bool {true}
        fn copy(&self,payload:ClipboardPayload)->Result<(),WireErrorCode> {
            let label=match payload {
                ClipboardPayload::Text(text)=>format!("text:{}",text.len()),
                ClipboardPayload::Html{html,text}=>format!("html:{}:{}",html.len(),text.len()),
                ClipboardPayload::Image{bytes,mime}=>{assert!(!bytes.is_empty());assert_eq!(mime,"image/png");"image".into()},
                _=>return Err(WireErrorCode::InvalidRequest),
            };
            self.copied.lock().unwrap().push(label);Ok(())
        }
        fn can_paste(&self)->bool {true}
        fn paste(&self)->Result<(),WireErrorCode> {self.pastes.fetch_add(1,std::sync::atomic::Ordering::SeqCst);Ok(())}
        fn stop_helpers(&self)->Result<(),WireErrorCode> {Ok(())}
    }
    fn session(host:&Host,role:ClientRole,grant:bool)->Session {
        let token=grant.then(||host.grants().issue_control(role).unwrap().token().to_owned());
        host.hello(HelloParams{protocol_version:dzc_protocol::wire::VERSION,client_version:"synthetic".into(),client_role:role,
            capabilities:vec![],required_capabilities:vec![],grant:token}).unwrap().0
    }
    fn call(host:&Host,session:&Session,method:Method,params:Value)->Response {
        host.dispatch(session,Request{request_id:"generated-copy".into(),method,params})
    }
    fn begin(host:&Host,session:&Session,copy:bool)->Response {
        let state=host.snapshot().unwrap();
        call(host,session,Method::DesktopCaptureBegin,serde_json::json!({"service_instance_id":state.service_instance_id,"expected_epoch":state.privacy_epoch,"copy_to_clipboard":copy}))
    }
    fn upload(host:&Host,session:&Session,body:Value)->String {
        let id=begin(host,session,true).result.unwrap()["capture_id"].as_str().unwrap().to_owned();
        for (index,chunk) in serde_json::to_vec(&body).unwrap().chunks(192*1024).enumerate() {
            assert!(call(host,session,Method::DesktopCaptureWrite,serde_json::json!({"capture_id":id,"offset":index*192*1024,
                "data_base64":base64::engine::general_purpose::STANDARD.encode(chunk)})).error.is_none());
        }
        id
    }
    fn finish(host:&Host,session:&Session,id:&str)->Response {
        call(host,session,Method::DesktopCaptureFinish,serde_json::json!({"capture_id":id,"source_app":"Synthetic Emoji","inherited_protection":false,"paste":true}))
    }
    #[test]
    fn generated_copy_uses_desktop_owner_for_emoji_image_and_large_unpersisted_content() {
        let original=crate::router::tests::synthetic_host();
        let actions=Arc::new(Actions::default());
        let host=Host::new(HostOptions{mode:HostMode::Service,history:original.history.clone(),directory:original.directory.clone(),actions:actions.clone()}).unwrap();
        for role in [ClientRole::Desktop,ClientRole::Mcp,ClientRole::Surface,ClientRole::Cli] {
            assert_eq!(begin(&host,&session(&host,role,false),true).error.unwrap().code,WireErrorCode::PermissionDenied);
        }
        assert_eq!(begin(&host,&session(&host,ClientRole::Cli,true),true).error.unwrap().code,WireErrorCode::PermissionDenied);
        let desktop=session(&host,ClientRole::Desktop,true);
        let other=session(&host,ClientRole::Desktop,true);
        let bodies=[serde_json::json!({"kind":"text","text":"🎸"}),
            serde_json::json!({"kind":"image","text":"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVQI12P4z8AAAAMBAQAY3Y2wAAAAAElFTkSuQmCC"}),
            serde_json::json!({"kind":"html","text":"plain","html":"<b>plain</b>"}),
            serde_json::json!({"kind":"text","text":"x".repeat(1024*1024+1)})];
        for body in bodies {
            let kind=body["kind"].as_str().unwrap().to_owned();
            let id=upload(&host,&desktop,body);
            assert_eq!(finish(&host,&other,&id).error.unwrap().code,WireErrorCode::NotFound);
            let response=finish(&host,&desktop,&id);
            assert!(response.error.is_none(),"{kind}: {:?}",response.error);
            assert_eq!(response.result.unwrap()["performed"],true);
        }
        host.history.privacy().set_session(dzc_core::api::PrivacySessionCommand::On{duration_ms:None},crate::router::now_ms().unwrap()).unwrap();
        assert!(begin(&host,&desktop,false).result.unwrap().get("capture_id").is_none());
        let id=upload(&host,&desktop,serde_json::json!({"kind":"text","text":"active private session emoji"}));
        assert_eq!(finish(&host,&desktop,&id).result.unwrap()["performed"],true);
        assert_eq!(actions.copied.lock().unwrap().len(),5);
        assert_eq!(actions.pastes.load(std::sync::atomic::Ordering::SeqCst),5);
        assert!(host.history.query(&dzc_core::api::HistoryQuery{limit:20,..Default::default()}).unwrap().is_empty());
        let id=upload(&host,&desktop,serde_json::json!({"kind":"image","text":"/synthetic/forbidden/local-file.png"}));
        assert_eq!(finish(&host,&desktop,&id).error.unwrap().code,WireErrorCode::InvalidRequest);
        let id=upload(&host,&desktop,serde_json::json!({"kind":"text","text":"stale generated upload"}));
        host.history.privacy().set_session(dzc_core::api::PrivacySessionCommand::Off,crate::router::now_ms().unwrap()).unwrap();
        assert!(finish(&host,&desktop,&id).error.is_some());
        assert_eq!(actions.copied.lock().unwrap().len(),5);
    }
}
