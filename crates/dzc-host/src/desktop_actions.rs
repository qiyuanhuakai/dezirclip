//! Body-bearing Desktop actions require the private Desktop control grant;
//! Surface/MCP read permissions can never invoke them.
use crate::{Host,Session};
use dzc_core::{api::{PrivacyAccessIntent,SensitiveAccess},capture::{CaptureData,CaptureFrame,CaptureReadMetadata,CaptureSource},ports::ClipboardPayload};
use dzc_protocol::wire::*;
use serde_json::Value;
use base64::Engine;
fn denied(error:impl ToString)->WireErrorCode {crate::router::privacy_error(error)}
fn encoded<T:serde::Serialize>(value:T)->Result<Value,WireErrorCode> {serde_json::to_value(value).map_err(|_|WireErrorCode::Internal)}
impl Host {
    pub(crate) fn desktop_permission(&self,session:&Session,instance:&str)->Result<(),WireErrorCode> {
        session.require(Permission::DesktopHistory)?;
        if session.role()!=ClientRole::Desktop {return Err(WireErrorCode::PermissionDenied);}
        if instance!=self.snapshot()?.service_instance_id {return Err(WireErrorCode::PrivacyBlocked);}
        Ok(())
    }
    pub(crate) fn desktop_summary(&self,session:&Session,parameters:Value)->Result<Value,WireErrorCode> {
        let request:DesktopEntryParams=serde_json::from_value(parameters).map_err(|_|WireErrorCode::InvalidRequest)?;
        self.desktop_permission(session,&request.service_instance_id)?;
        let now=crate::router::now_ms()?;
        let permit=self.history.privacy().authorize_read(PrivacyAccessIntent::InteractiveRead,SensitiveAccess::Denied,Some(request.expected_epoch),now).map_err(denied)?;
        let mut rows=if request.id<0 {
            let session=self.session.lock().map_err(|_|WireErrorCode::Unavailable)?;
            let row=session.iter().find(|row|row.id==request.id).ok_or(WireErrorCode::NotFound)?;
            self.history.entries_metadata_for_gui(std::slice::from_ref(row),true,now).map_err(denied)?
        } else {
            let rows=self.history.query(&dzc_core::api::HistoryQuery{limit:1,id:Some(request.id),metadata:true,..Default::default()}).map_err(|_|WireErrorCode::Unavailable)?;
            self.history.entries_metadata_for_gui(&rows,true,now).map_err(denied)?
        };
        let row=rows.pop().ok_or(WireErrorCode::NotFound)?;
        self.history.privacy().check_read(&permit,crate::router::now_ms()?).map_err(denied)?;
        self.authorized_snapshot(session,&permit,&crate::desktop::entry(row,true))
    }
    pub(crate) fn desktop_copy(&self,session:&Session,parameters:Value)->Result<Value,WireErrorCode> {
        let request:DesktopCopyParams=serde_json::from_value(parameters).map_err(|_|WireErrorCode::InvalidRequest)?;
        self.desktop_permission(session,&request.service_instance_id)?;
        if !self.actions.can_copy() || request.paste && !self.actions.can_paste() {return Err(WireErrorCode::UnsupportedCapability);}
        let now=crate::router::now_ms()?;
        let generated=request.derived_text.is_some() || request.image_as_base64.unwrap_or(false);
        let ticket=if generated {match self.history.privacy().capture_ticket(now) {
            Ok(ticket)=>Some(ticket),Err(dzc_core::api::PrivacyError::PrivacySessionActive)=>None,Err(error)=>return Err(denied(error))}}
            else {None};
        let permit=self.history.privacy().authorize_selected_copy(request.id,Some(&request.service_instance_id),if request.reveal {SensitiveAccess::ExplicitUserAction}else{SensitiveAccess::Denied},Some(request.expected_epoch),now).map_err(denied)?;
        let session_entry=if request.id<0 {Some(self.session.lock().map_err(|_|WireErrorCode::Unavailable)?.iter().find(|row|row.id==request.id).cloned().ok_or(WireErrorCode::NotFound)?)}else{None};
        let mut payload=if let Some(text)=request.derived_text {
            if let Some(entry)=session_entry.as_ref() {self.history.session_derived_text_payload_for_host_authorized(entry,&request.service_instance_id,text,&permit,now)}
                else {self.history.derived_text_payload_authorized(request.id,text,&permit,now)}
        } else if let Some(entry)=session_entry.as_ref() {self.history.session_clipboard_payload_for_host_authorized(entry,&request.service_instance_id,&permit,now)}
            else {self.history.clipboard_payload_authorized(request.id,&permit,now)}.map_err(denied)?;
        if request.image_as_base64.unwrap_or(false) {payload=image_text(payload)?;}
        if request.with_format==Some(false) {payload=plain(payload);}
        if generated {
            if let Some(ticket)=ticket {
                let text=payload_text(&payload).ok_or(WireErrorCode::InvalidRequest)?;
                let frame=CaptureFrame {data:CaptureData::Text(text.into()),metadata:CaptureReadMetadata{privacy_ticket:Some(ticket),inherited_protection:matches!(payload,ClipboardPayload::Private{..}),..Default::default()}};
                self.capture_generated(frame,CaptureSource{app_name:"DezirClip Derived".into(),process_path:None},now)?;
            }
        }
        self.history.privacy().check_read(&permit,crate::router::now_ms()?).map_err(denied)?;
        self.copy_payload(payload)?;
        if request.paste {self.history.privacy().check_read(&permit,crate::router::now_ms()?).map_err(denied)?;self.actions.paste()?;}
        if request.delete_after_use==Some(true) {
            if request.id<0 {self.session.lock().map_err(|_|WireErrorCode::Unavailable)?.retain(|row|row.id!=request.id);}
            else {self.history.delete(request.id,Some(&self.directory)).map_err(|_|WireErrorCode::Unavailable)?;}
            self.publish(EventKind::EntryDeleted,EventData{entry_id:Some(request.id),status:None})?;
        } else if request.id>0 && request.paste {
            self.history.increment_use_count(request.id).map_err(|_|WireErrorCode::Unavailable)?;
            let entry=self.history.query(&dzc_core::api::HistoryQuery {id:Some(request.id),metadata:true,limit:1,..Default::default()}).map_err(|_|WireErrorCode::Unavailable)?;
            if request.move_to_top.unwrap_or(true) && entry.first().is_some_and(|entry|!entry.is_pinned) {self.history.touch_entry(request.id,crate::router::now_ms()?).map_err(|_|WireErrorCode::Unavailable)?;}
        }
        encoded(OperationResult{performed:true})
    }
    pub(crate) fn desktop_mutate(&self,session:&Session,parameters:Value)->Result<Value,WireErrorCode> {
        let request:DesktopMutationParams=serde_json::from_value(parameters).map_err(|_|WireErrorCode::InvalidRequest)?;
        self.desktop_permission(session,&request.service_instance_id)?;
        encoded(self.mutate_inner(request)?)
    }
    /// Trusted Embedded Desktop action. Body fields remain owned Rust values;
    /// remote callers additionally pass the router's role/grant checks.
    pub fn mutate_owned(&self,request:DesktopMutationParams)->Result<DesktopMutationResult,WireErrorCode> {
        let _request=self.requests.lock().map_err(|_|WireErrorCode::Unavailable)?;
        self.mutate_inner(request)
    }
    fn mutate_inner(&self,request:DesktopMutationParams)->Result<DesktopMutationResult,WireErrorCode> {
        if self.is_stopping() {return Err(WireErrorCode::Busy);}
        if request.service_instance_id!=self.snapshot()?.service_instance_id {return Err(WireErrorCode::PrivacyBlocked);}
        let now=crate::router::now_ms()?;
        let permit=self.history.privacy().authorize_read(PrivacyAccessIntent::InteractiveRead,if request.reveal{SensitiveAccess::ExplicitUserAction}else{SensitiveAccess::Denied},Some(request.expected_epoch),now).map_err(denied)?;
        if request.action=="delete" {
            if request.id<0 {
                let mut rows=self.session.lock().map_err(|_|WireErrorCode::Unavailable)?;
                if !rows.iter().any(|row|row.id==request.id) {return Err(WireErrorCode::NotFound);}
                self.history.privacy().check_read(&permit,crate::router::now_ms()?).map_err(denied)?;
                rows.retain(|row|row.id!=request.id);
            } else {
                let rows=self.history.query(&dzc_core::api::HistoryQuery{id:Some(request.id),metadata:true,limit:1,..Default::default()}).map_err(|_|WireErrorCode::Unavailable)?;
                if rows.is_empty() {return Err(WireErrorCode::NotFound);}
                self.history.privacy().check_read(&permit,crate::router::now_ms()?).map_err(denied)?;
                self.history.delete(request.id,Some(&self.directory)).map_err(|_|WireErrorCode::Unavailable)?;
            }
            self.publish(EventKind::EntryDeleted,EventData{entry_id:Some(request.id),status:None})?;
            return Ok(DesktopMutationResult{id:request.id,performed:true});
        }
        let intent=if request.confirm_persistence.unwrap_or(false){dzc_core::api::ManualPersistenceIntent::ConfirmedDuringPrivacySession}else{dzc_core::api::ManualPersistenceIntent::Ordinary};
        if request.id<0 {
            let mut rows=self.session.lock().map_err(|_|WireErrorCode::Unavailable)?;
            let index=rows.iter().position(|row|row.id==request.id).ok_or(WireErrorCode::NotFound)?;
            let mut result_id=request.id;
            match request.action.as_str() {
                "pin" if request.pinned==Some(false)=>{
                    self.history.privacy().check_read(&permit,crate::router::now_ms()?).map_err(denied)?;
                    rows[index].is_pinned=false;
                },
                "edit"=>{
                    let content=request.content.ok_or(WireErrorCode::InvalidRequest)?;
                    let preview:String=content.chars().take(500).collect();
                    self.history.update_session_manual_authorized(&mut rows[index],&content,&preview,intent,&permit,now).map_err(denied)?;
                }
                "pin"|"tags"=>{
                    let (pinned,tags)=if request.action=="pin" {(Some(request.pinned.ok_or(WireErrorCode::InvalidRequest)?),None)}
                        else {(None,Some(request.tags.ok_or(WireErrorCode::InvalidRequest)?))};
                    result_id=self.history.persist_session_metadata_authorized(&rows[index],pinned,tags,intent,&permit,now).map_err(denied)?;
                    rows.remove(index);
                }
                _=>return Err(WireErrorCode::InvalidRequest),
            }
            drop(rows);
            if result_id!=request.id {self.publish(EventKind::EntryDeleted,EventData{entry_id:Some(request.id),status:None})?;}
            self.publish(EventKind::EntryUpdated,EventData{entry_id:Some(result_id),status:None})?;
            return Ok(DesktopMutationResult{id:result_id,performed:true});
        }
        match request.action.as_str() {
            "pin"=>self.history.update_entry_metadata_authorized(request.id,Some(request.pinned.ok_or(WireErrorCode::InvalidRequest)?),None,&permit,now).map_err(denied)?,
            "tags"=>self.history.update_entry_metadata_authorized(request.id,None,Some(request.tags.ok_or(WireErrorCode::InvalidRequest)?),&permit,now).map_err(denied)?,
            "edit"=>{let content=request.content.ok_or(WireErrorCode::InvalidRequest)?;let preview:String=content.chars().take(500).collect();
                self.history.update_manual_authorized(request.id,&content,&preview,intent,&permit,now).map_err(denied)?;}
            _=>return Err(WireErrorCode::InvalidRequest),
        }
        self.publish(EventKind::EntryUpdated,EventData{entry_id:Some(request.id),status:None})?;
        Ok(DesktopMutationResult{id:request.id,performed:true})
    }
}
fn plain(payload:ClipboardPayload)->ClipboardPayload {match payload {
    ClipboardPayload::Html{text,..}=>ClipboardPayload::Text(text),
    ClipboardPayload::Private{payload,authorization}=>ClipboardPayload::Private{payload:Box::new(plain(*payload)),authorization},payload=>payload}}
fn image_text(payload:ClipboardPayload)->Result<ClipboardPayload,WireErrorCode> {Ok(match payload {
    ClipboardPayload::Image{bytes,..}=>ClipboardPayload::Text(base64::engine::general_purpose::STANDARD.encode(bytes)),
    ClipboardPayload::Private{payload,authorization}=>ClipboardPayload::Private{payload:Box::new(image_text(*payload)?),authorization},
    _=>return Err(WireErrorCode::InvalidRequest),
})}
fn payload_text(payload:&ClipboardPayload)->Option<&str> {match payload {ClipboardPayload::Text(text)=>Some(text),ClipboardPayload::Private{payload,..}=>payload_text(payload),_=>None}}
