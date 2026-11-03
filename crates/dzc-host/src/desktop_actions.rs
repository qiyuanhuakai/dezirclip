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
    pub(crate) fn prepare_desktop_paste(&self, paste: bool, target: Option<&WindowsPasteTarget>)
        -> Result<Option<Box<dyn crate::PreparedPaste>>, WireErrorCode> {
        if !paste {
            if target.is_some() { return Err(WireErrorCode::InvalidRequest); }
            return Ok(None);
        }
        if cfg!(target_os="windows") || target.is_some() || self.actions.supports_windows_bound_paste() {
            let target = target.ok_or(WireErrorCode::UnsupportedCapability)?;
            if !self.actions.supports_windows_bound_paste() { return Err(WireErrorCode::UnsupportedCapability); }
            return self.actions.prepare_windows_paste(target).map(Some);
        }
        Ok(None)
    }
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
        self.desktop_copy_bound(session, request, None)
    }

    pub(crate) fn remember_window_target(&self, session: &Session) -> Result<Value, WireErrorCode> {
        self.desktop_permission(session, self.instance_id())?;
        session.require(Permission::ClipboardPaste)?;
        let mut slot = session.window_target.lock().map_err(|_| WireErrorCode::Unavailable)?;
        *slot = None;
        let peer = session.peer.as_ref().ok_or(WireErrorCode::PermissionDenied)?;
        *slot = Some(dzc_platform::runtime::RememberedFocus::capture(peer).map_err(|_| WireErrorCode::Unavailable)?);
        encoded(OperationResult { performed: true })
    }

    pub(crate) fn paste_from_window(&self, session: &Session, request: DesktopCopyParams) -> Result<Value, WireErrorCode> {
        self.check_library_state(session, &request.service_instance_id, request.expected_epoch)?;
        session.require(Permission::ClipboardPaste)?;
        if !request.paste || request.expected_paste_target.is_some() || request.id == 0 {
            return Err(WireErrorCode::InvalidRequest);
        }
        let state = self.snapshot()?;
        if state.privacy_epoch != request.expected_epoch { return Err(WireErrorCode::PrivacyEpochChanged); }
        let input = self.take_window_target(session)?;
        if !crate::same_surface_state(&state, &self.snapshot()?) { return Err(WireErrorCode::HistoryRevisionChanged); }
        self.desktop_copy_bound(session, request, Some((input, state)))
    }

    fn take_window_target(&self, session: &Session) -> Result<dzc_platform::runtime::QueueInputTarget, WireErrorCode> {
        let destination = session.window_target.lock().map_err(|_| WireErrorCode::Unavailable)?
            .take().ok_or(WireErrorCode::NotFound)?;
        let target = destination.restore().map_err(|_| WireErrorCode::Unavailable)?;
        dzc_platform::runtime::QueueInputTarget::prepare(target).map_err(|_| WireErrorCode::Unavailable)
    }

    pub(crate) fn send_window_emoji(&self, session: &Session, request: WindowEmojiParams) -> Result<Value, WireErrorCode> {
        self.check_library_state(session, &request.service_instance_id, request.expected_epoch)?;
        if !self.actions.can_copy() || (request.paste && !self.actions.can_paste()) {
            return Err(WireErrorCode::UnsupportedCapability);
        }
        if request.paste { session.require(Permission::ClipboardPaste)?; }
        let now = crate::router::now_ms()?;
        let privacy = self.history.privacy();
        let permit = privacy.authorize_read(PrivacyAccessIntent::GeneratedCopy, SensitiveAccess::Denied, Some(request.expected_epoch), now).map_err(denied)?;
        let text = match request.kind.as_str() {
            "text" => request.content,
            "image" => dzc_platform::user_resource::read_managed_emoji(&self.directory, request.content.as_ref(), false)
                .map_err(|_| WireErrorCode::InvalidRequest)?,
            _ => return Err(WireErrorCode::InvalidRequest),
        };
        let ticket = match privacy.capture_ticket(now) {
            Ok(ticket) => Some(ticket),
            Err(dzc_core::api::PrivacyError::PrivacySessionActive) => None,
            Err(error) => return Err(denied(error)),
        };
        let input = if request.paste {
            let (_, _, method) = self.history.queue_preferences().map_err(|_| WireErrorCode::Unavailable)?;
            let plain = (request.kind == "text").then(|| text.clone());
            Some((self.take_window_target(session)?, method, plain))
        } else { None };
        let authorized = || !self.is_stopping() && self.grants.still_valid(session)
            && crate::router::now_ms().is_ok_and(|time| privacy.check_read(&permit, time).is_ok());
        if !authorized() { return Err(WireErrorCode::PrivacyBlocked); }
        self.commit_generated_copy(GeneratedBody { kind: request.kind, text, html: None }, "DezirClip", now, ticket, &permit)?;
        if let Some((target, method, plain)) = input {
            let kind = if plain.is_some() { "text" } else { "binary" };
            target.send(&method, plain.as_deref(), kind, authorized).map_err(|_| WireErrorCode::Unavailable)?;
            self.play_feedback(dzc_platform::sound::Cue::Paste);
        }
        encoded(OperationResult { performed: true })
    }

    pub(crate) fn emoji_favorites(&self, session: &Session, request: EmojiFavoritesParams) -> Result<Value, WireErrorCode> {
        self.check_library_state(session, &request.service_instance_id, request.expected_epoch)?;
        if let Some(action) = request.mutation.as_deref() {
            use dzc_platform::user_resource;
            let path = std::path::Path::new(request.path.as_deref().ok_or(WireErrorCode::InvalidRequest)?);
            if !path.is_absolute() { return Err(WireErrorCode::InvalidRequest); }
            let image = match action {
                "import" => {
                    let source = user_resource::selected_background_path(&self.directory, path).map_err(|_| WireErrorCode::InvalidRequest)?;
                    Some(user_resource::read_image_bytes(&source).map_err(|_| WireErrorCode::InvalidRequest)?)
                }
                "remove" => None,
                _ => return Err(WireErrorCode::InvalidRequest),
            };
            let privacy = self.history.privacy();
            let prepared = if let Some(bytes) = image.as_ref() {
                privacy.with_writable_profile(request.expected_epoch, crate::router::now_ms()?, || ()).map_err(denied)?;
                Some(user_resource::prepare_emoji_favorite(&self.directory, bytes).map_err(|_| WireErrorCode::InvalidRequest)?)
            } else { None };
            let paths = privacy.with_writable_profile(request.expected_epoch, crate::router::now_ms()?, || {
                if !self.grants.still_valid(session) { return Err(WireErrorCode::PermissionDenied); }
                match prepared {
                    Some(image) => image.publish().map(|path| vec![path]),
                    None => user_resource::remove_emoji_favorite(&self.directory, path).map(|()| Vec::new()),
                }.map_err(|_| WireErrorCode::InvalidRequest)
            }).map_err(denied)??;
            return encoded(EmojiFavoritesResult { paths, preview: None });
        }
        let (paths, preview) = match request.path {
            Some(path) => (Vec::new(), Some(dzc_platform::user_resource::read_managed_emoji(&self.directory, path.as_ref(), true)
                .map_err(|_| WireErrorCode::InvalidRequest)?)),
            None => (dzc_platform::user_resource::list_emoji_favorites(&self.directory).map_err(|_| WireErrorCode::Unavailable)?, None),
        };
        self.check_library_state(session, &request.service_instance_id, request.expected_epoch)?;
        if !self.grants.still_valid(session) { return Err(WireErrorCode::PermissionDenied); }
        let result = EmojiFavoritesResult { paths, preview };
        if serde_json::to_vec(&result).map_err(|_| WireErrorCode::Internal)?.len() > MAX_FRAME_BYTES - 4096 {
            return Err(WireErrorCode::FrameTooLarge);
        }
        encoded(result)
    }

    pub(crate) fn desktop_copy_bound(&self, session: &Session, request: DesktopCopyParams, bound: Option<(dzc_platform::runtime::QueueInputTarget, RuntimeState)>) -> Result<Value,WireErrorCode> {
        self.desktop_permission(session,&request.service_instance_id)?;
        if !self.actions.can_copy() || request.paste && !self.actions.can_paste() {return Err(WireErrorCode::UnsupportedCapability);}
        let paste_target=if bound.is_none() { self.prepare_desktop_paste(request.paste,request.expected_paste_target.as_ref())? } else { None };
        let now=crate::router::now_ms()?;
        let generated=request.derived_text.is_some() || request.image_as_base64.unwrap_or(false);
        // A Service started without capture still handles explicit copy actions.
        // An unexpectedly unavailable capture owner still fails closed.
        let capture_generated = generated && !self.capture_disabled_by_request()?;
        let ticket=if capture_generated {match self.history.privacy().capture_ticket(now) {
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
        if let Some(ticket)=ticket {
            let text=payload_text(&payload).ok_or(WireErrorCode::InvalidRequest)?;
            let frame=CaptureFrame {data:CaptureData::Text(text.into()),metadata:CaptureReadMetadata{privacy_ticket:Some(ticket),inherited_protection:matches!(payload,ClipboardPayload::Private{..}),..Default::default()}};
            self.capture_generated(frame,CaptureSource{app_name:"DezirClip Derived".into(),process_path:None},now)?;
        }
        self.history.privacy().check_read(&permit,crate::router::now_ms()?).map_err(denied)?;
        let input = if bound.is_some() {
            let (_, _, method) = self.history.queue_preferences().map_err(|_|WireErrorCode::Unavailable)?;
            let mut data = &payload;
            while let ClipboardPayload::Private {payload,..} = data { data = payload; }
            let (kind, text) = match data {
                ClipboardPayload::Text(text) => ("text", Some(text.clone())),
                ClipboardPayload::Html {text,..} => ("rich_text", Some(text.clone())),
                _ => ("binary", None),
            };
            Some((method, kind, text))
        } else { None };
        let selected_state = bound.as_ref().map(|(_,state)| state.clone());
        let check_selection = || -> Result<(), WireErrorCode> {
            if let Some(state) = &selected_state {
                if !self.grants.still_valid(session) { return Err(WireErrorCode::PermissionDenied); }
                if !crate::same_surface_state(state, &self.snapshot()?) { return Err(WireErrorCode::HistoryRevisionChanged); }
            }
            Ok(())
        };
        check_selection()?;
        self.copy_payload(payload)?;
        if request.paste {
            check_selection()?;
            self.history.privacy().check_read(&permit,crate::router::now_ms()?).map_err(denied)?;
            if let Some(((target, _), (method, kind, text))) = bound.zip(input) {
                target.send(&method, text.as_deref(), kind, || !self.is_stopping() && check_selection().is_ok()
                    && crate::router::now_ms().is_ok_and(|now| self.history.privacy().check_read(&permit, now).is_ok()))
                    .map_err(|_| WireErrorCode::Unavailable)?;
            } else if let Some(target)=paste_target { target.send()?; } else { self.actions.paste()?; }
            self.play_feedback(dzc_platform::sound::Cue::Paste);
        }
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
    pub(super) fn mutate_inner(&self,request:DesktopMutationParams)->Result<DesktopMutationResult,WireErrorCode> {
        if self.is_stopping() {return Err(WireErrorCode::Busy);}
        if request.service_instance_id!=self.snapshot()?.service_instance_id {return Err(WireErrorCode::PrivacyBlocked);}
        let now=crate::router::now_ms()?;
        let completed_copy=request.action=="used";
        if completed_copy && (request.id<=0 || request.reveal || request.content.is_some() || request.tags.is_some() || request.pinned.is_some() || request.confirm_persistence.is_some()) {return Err(WireErrorCode::InvalidRequest);}
        let permit=if completed_copy {
            self.history.privacy().authorize_selected_copy(request.id,Some(&request.service_instance_id),SensitiveAccess::Denied,Some(request.expected_epoch),now)
        } else {
            self.history.privacy().authorize_read(PrivacyAccessIntent::InteractiveRead,if request.reveal {SensitiveAccess::ExplicitUserAction}else{SensitiveAccess::Denied},Some(request.expected_epoch),now)
        }.map_err(denied)?;
        if completed_copy {
            let rows=self.history.query(&dzc_core::api::HistoryQuery{id:Some(request.id),metadata:true,limit:1,..Default::default()}).map_err(|_|WireErrorCode::Unavailable)?;
            if rows.is_empty() {return Err(WireErrorCode::NotFound);}
            self.history.privacy().check_read(&permit,crate::router::now_ms()?).map_err(denied)?;
            self.history.increment_use_count(request.id).map_err(|_|WireErrorCode::Unavailable)?;
            self.publish(EventKind::EntryUpdated,EventData{entry_id:Some(request.id),status:None})?;
            return Ok(DesktopMutationResult{id:request.id,performed:true});
        }
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
pub(crate) fn plain(payload:ClipboardPayload)->ClipboardPayload {match payload {
    ClipboardPayload::Html{text,..}=>ClipboardPayload::Text(text),
    ClipboardPayload::Private{payload,authorization}=>ClipboardPayload::Private{payload:Box::new(plain(*payload)),authorization},payload=>payload}}
fn image_text(payload:ClipboardPayload)->Result<ClipboardPayload,WireErrorCode> {Ok(match payload {
    ClipboardPayload::Image{bytes,..}=>ClipboardPayload::Text(base64::engine::general_purpose::STANDARD.encode(bytes)),
    ClipboardPayload::Private{payload,authorization}=>ClipboardPayload::Private{payload:Box::new(image_text(*payload)?),authorization},
    _=>return Err(WireErrorCode::InvalidRequest),
})}
fn payload_text(payload:&ClipboardPayload)->Option<&str> {match payload {ClipboardPayload::Text(text)=>Some(text),ClipboardPayload::Private{payload,..}=>payload_text(payload),_=>None}}
#[cfg(test)]
mod queue_completion_tests {
    use super::*;
    use std::sync::{Arc,atomic::{AtomicUsize,Ordering}};
    #[derive(Default)]
    struct Actions {copies:AtomicUsize,pastes:AtomicUsize}
    impl crate::HostActions for Actions {
        fn platform_capabilities(&self)->Value {serde_json::json!({})}
        fn can_copy(&self)->bool {true}
        fn copy(&self,_:ClipboardPayload)->Result<(),WireErrorCode> {self.copies.fetch_add(1,Ordering::SeqCst);Ok(())}
        fn can_paste(&self)->bool {true}
        fn paste(&self)->Result<(),WireErrorCode> {self.pastes.fetch_add(1,Ordering::SeqCst);Ok(())}
        fn stop_helpers(&self)->Result<(),WireErrorCode> {Ok(())}
    }
    fn session(host:&Host,role:ClientRole)->Session {
        let grant=host.grants().issue_control(role).unwrap();
        host.hello(HelloParams{protocol_version:VERSION,client_version:"synthetic".into(),client_role:role,capabilities:vec![],required_capabilities:vec![],grant:Some(grant.token().into())}).unwrap().0
    }
    fn call(host:&Host,owner:&Session,method:Method,params:Value)->Response {
        host.dispatch(owner,Request{request_id:"queue-completion".into(),method,params})
    }
    fn used(id:i64,state:&RuntimeState)->Value {
        serde_json::json!({"id":id,"service_instance_id":state.service_instance_id,"expected_epoch":state.privacy_epoch,"reveal":false,"action":"used"})
    }
    #[test]
    fn rejected_native_paste_preserves_history_in_embedded_and_service_modes() {
        struct FailingPaste;
        impl crate::HostActions for FailingPaste {
            fn platform_capabilities(&self)->Value {serde_json::json!({})}
            fn can_copy(&self)->bool {true}
            fn copy(&self,_:ClipboardPayload)->Result<(),WireErrorCode> {Ok(())}
            fn can_paste(&self)->bool {true}
            fn paste(&self)->Result<(),WireErrorCode> {panic!("bound Desktop paste cannot use unbound fallback")}
            fn supports_windows_bound_paste(&self)->bool {true}
            fn prepare_windows_paste(&self,_:&WindowsPasteTarget)->Result<Box<dyn crate::PreparedPaste>,WireErrorCode> {
                struct Rejected;
                impl crate::PreparedPaste for Rejected {fn send(&self)->Result<(),WireErrorCode>{Err(WireErrorCode::Unavailable)}}
                Ok(Box::new(Rejected))
            }
            fn stop_helpers(&self)->Result<(),WireErrorCode> {Ok(())}
        }
        for mode in [HostMode::Embedded,HostMode::Service] {
            for delete in [false,true] {
                let original=crate::router::tests::synthetic_host();
                let host=Host::new(crate::HostOptions{mode,history:original.history.clone(),directory:original.directory.clone(),actions:Arc::new(FailingPaste)}).unwrap();
                let entry=serde_json::from_value(serde_json::json!({"id":0,"content_type":"text","content":"synthetic rejected paste","preview":"synthetic","source_app":"Synthetic","timestamp":1,"is_pinned":false})).unwrap();
                let id=host.history.save(&entry,None).unwrap();
                let query=dzc_core::api::HistoryQuery{id:Some(id),metadata:true,limit:1,..Default::default()};
                let before=host.history.query(&query).unwrap();
                let owner=session(&host,ClientRole::Desktop);let state=host.snapshot().unwrap();
                let cursor=state.event_sequence;
                let failed=call(&host,&owner,Method::DesktopClipboardCopy,serde_json::json!({"id":id,"service_instance_id":state.service_instance_id,"expected_epoch":state.privacy_epoch,"reveal":false,"paste":true,"expected_paste_target":crate::router::tests::windows_target(),"delete_after_use":delete,"move_to_top":true}));
                assert_eq!(failed.error.unwrap().code,WireErrorCode::Unavailable);
                let after=host.history.query(&query).unwrap();
                assert_eq!(after.len(),1);
                assert_eq!(after[0].use_count,before[0].use_count);
                assert_eq!(after[0].timestamp,before[0].timestamp);
                assert_eq!(after[0].content,before[0].content);
                let events:EventsResult=serde_json::from_value(call(&host,&owner,Method::EventsSubscribe,serde_json::json!({"after_sequence":cursor,"wait_ms":0})).result.unwrap()).unwrap();
                assert!(!events.events.iter().any(|event|matches!(event.event,EventKind::EntryDeleted|EventKind::EntryUpdated)));
            }
        }
    }
    #[test]
    fn bound_target_missing_or_changed_during_copy_never_pastes_or_consumes_history() {
        use std::sync::atomic::AtomicBool;
        struct Fixed { current:Arc<AtomicBool>,sent:Arc<AtomicUsize> }
        impl crate::PreparedPaste for Fixed {
            fn send(&self)->Result<(),WireErrorCode> {
                if !self.current.load(Ordering::SeqCst) {return Err(WireErrorCode::Unavailable);}
                self.sent.fetch_add(1,Ordering::SeqCst);Ok(())
            }
        }
        struct Bound { current:Arc<AtomicBool>,sent:Arc<AtomicUsize>,copies:AtomicUsize,change:bool }
        impl crate::HostActions for Bound {
            fn platform_capabilities(&self)->Value {serde_json::json!({})}
            fn can_copy(&self)->bool {true}
            fn copy(&self,_:ClipboardPayload)->Result<(),WireErrorCode> {
                self.copies.fetch_add(1,Ordering::SeqCst);
                if self.change {self.current.store(false,Ordering::SeqCst);} Ok(())
            }
            fn can_paste(&self)->bool {true}
            fn paste(&self)->Result<(),WireErrorCode> {panic!("no unbound fallback")}
            fn supports_windows_bound_paste(&self)->bool {true}
            fn prepare_windows_paste(&self,target:&WindowsPasteTarget)->Result<Box<dyn crate::PreparedPaste>,WireErrorCode> {
                assert_eq!(*target,crate::router::tests::windows_target());
                Ok(Box::new(Fixed{current:self.current.clone(),sent:self.sent.clone()}))
            }
            fn stop_helpers(&self)->Result<(),WireErrorCode> {Ok(())}
        }
        for (missing,change) in [(true,false),(false,true),(false,false)] {
            let original=crate::router::tests::synthetic_host();
            let actions=Arc::new(Bound{current:Arc::new(AtomicBool::new(true)),sent:Arc::new(AtomicUsize::new(0)),copies:AtomicUsize::new(0),change});
            let host=Host::new(crate::HostOptions{mode:HostMode::Service,history:original.history.clone(),directory:original.directory.clone(),actions:actions.clone()}).unwrap();
            let entry=serde_json::from_value(serde_json::json!({"id":0,"content_type":"text","content":"synthetic bound target","preview":"synthetic","source_app":"Synthetic","timestamp":1,"is_pinned":false})).unwrap();
            let id=host.history.save(&entry,None).unwrap();
            let owner=session(&host,ClientRole::Desktop);let state=host.snapshot().unwrap();
            let mut params=serde_json::json!({"id":id,"service_instance_id":state.service_instance_id,"expected_epoch":state.privacy_epoch,"reveal":false,"paste":true,"delete_after_use":false,"move_to_top":false});
            if !missing {params["expected_paste_target"]=serde_json::to_value(crate::router::tests::windows_target()).unwrap();}
            let response=call(&host,&owner,Method::DesktopClipboardCopy,params);
            assert_eq!(response.error.is_none(),!missing && !change);
            assert_eq!(actions.copies.load(Ordering::SeqCst),if !missing {1}else{0});
            assert_eq!(actions.sent.load(Ordering::SeqCst),if !missing && !change {1}else{0});
            let rows=host.history.query(&dzc_core::api::HistoryQuery{id:Some(id),metadata:true,limit:1,..Default::default()}).unwrap();
            assert_eq!(rows.len(),1);
            assert_eq!(rows[0].use_count,if !missing && !change {1}else{0});
        }
    }
    #[test]
    fn host_copy_only_does_not_paste_delete_or_count_and_completion_only_updates_metadata() {
        let original=crate::router::tests::synthetic_host();
        let actions=Arc::new(Actions::default());
        let host=Host::new(crate::HostOptions{mode:HostMode::Service,history:original.history.clone(),directory:original.directory.clone(),actions:actions.clone()}).unwrap();
        let entry=serde_json::from_value(serde_json::json!({"id":0,"content_type":"text","content":"synthetic queue","preview":"synthetic","source_app":"Synthetic","timestamp":1,"is_pinned":false})).unwrap();
        let id=host.history.save(&entry,None).unwrap();let owner=session(&host,ClientRole::Desktop);let state=host.snapshot().unwrap();
        let copy=call(&host,&owner,Method::DesktopClipboardCopy,serde_json::json!({"id":id,"service_instance_id":state.service_instance_id,"expected_epoch":state.privacy_epoch,"reveal":false,"paste":false,"delete_after_use":false,"move_to_top":false}));
        assert!(copy.error.is_none());assert_eq!(actions.copies.load(Ordering::SeqCst),1);assert_eq!(actions.pastes.load(Ordering::SeqCst),0);
        assert_eq!(host.history.query(&dzc_core::api::HistoryQuery{id:Some(id),metadata:true,limit:1,..Default::default()}).unwrap()[0].use_count,0);
        let completed=call(&host,&owner,Method::DesktopHistoryMutate,used(id,&state));
        assert!(completed.error.is_none());assert_eq!(completed.result.unwrap(),serde_json::json!({"id":id,"performed":true}));
        assert_eq!(host.history.query(&dzc_core::api::HistoryQuery{id:Some(id),metadata:true,limit:1,..Default::default()}).unwrap()[0].use_count,1);
        assert_eq!(actions.copies.load(Ordering::SeqCst),1);assert_eq!(actions.pastes.load(Ordering::SeqCst),0);
        let mut deletion=used(id,&state);deletion["action"]="delete".into();
        assert!(call(&host,&owner,Method::DesktopHistoryMutate,deletion).error.is_none());
        assert!(host.history.query(&dzc_core::api::HistoryQuery{id:Some(id),metadata:true,limit:1,..Default::default()}).unwrap().is_empty());
    }
    #[test]
    fn completed_use_requires_desktop_grant_original_instance_epoch_and_metadata_only_parameters() {
        let host=crate::router::tests::synthetic_host();let owner=session(&host,ClientRole::Desktop);let state=host.snapshot().unwrap();
        assert_eq!(call(&host,&session(&host,ClientRole::Cli),Method::DesktopHistoryMutate,used(1,&state)).error.unwrap().code,WireErrorCode::PermissionDenied);
        let capabilities=call(&host,&owner,Method::Capabilities,serde_json::json!({})).result.unwrap();
        assert_eq!(capabilities["platform"]["desktop_queue_completion"],serde_json::json!({"supported":true,"version":1}));
        let mut wrong=used(1,&state);wrong["service_instance_id"]="replacement".into();
        assert_eq!(call(&host,&owner,Method::DesktopHistoryMutate,wrong).error.unwrap().code,WireErrorCode::PrivacyBlocked);
        let mut body=used(1,&state);body["content"]="unexpected body".into();
        assert_eq!(call(&host,&owner,Method::DesktopHistoryMutate,body).error.unwrap().code,WireErrorCode::InvalidRequest);
        assert_eq!(call(&host,&owner,Method::DesktopHistoryMutate,used(999,&state)).error.unwrap().code,WireErrorCode::NotFound);
        host.history.privacy().toggle(crate::router::now_ms().unwrap()).unwrap();
        assert_eq!(call(&host,&owner,Method::DesktopHistoryMutate,used(1,&state)).error.unwrap().code,WireErrorCode::PrivacyEpochChanged);
    }
}
