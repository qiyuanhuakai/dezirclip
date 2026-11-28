//! Body-bearing Desktop actions require the private Desktop control grant;
//! Surface/MCP read permissions can never invoke them.
use crate::{Host,Session};
use dzc_core::{api::{ClipboardEntry, PrivacyAccessIntent, PrivacyReadPermit, SensitiveAccess},capture::{CaptureData,CaptureFrame,CaptureReadMetadata,CaptureSource},ports::ClipboardPayload};
use dzc_protocol::wire::*;
use serde_json::Value;
use base64::Engine;
use sha2::{Digest, Sha256};

enum DerivedText<'a> {
    Transform(&'a str),
    QrSvg { content_sha256: &'a str },
    Ocr,
}

struct SelectedCopy {
    id: i64,
    permit: PrivacyReadPermit,
    session_entry: Option<ClipboardEntry>,
}

/// A prepared body for the embedded desktop's native clipboard formats.
pub struct EmbeddedClipboardSelection {
    pub entry: Option<ClipboardEntry>,
    pub payload: ClipboardPayload,
    pub action: EmbeddedClipboardAction,
}

/// Binds subsequent input and metadata updates to the original authorization.
pub struct EmbeddedClipboardAction {
    instance: String,
    permit: PrivacyReadPermit,
    pasted_entry: Option<i64>,
}

pub enum PasteHistoryChange {
    Unchanged,
    Updated(i64),
    Removed(i64),
}

fn denied(error:impl ToString)->WireErrorCode {crate::router::privacy_error(error)}
fn encoded<T:serde::Serialize>(value:T)->Result<Value,WireErrorCode> {serde_json::to_value(value).map_err(|_|WireErrorCode::Internal)}
impl Host {
    fn authorize_clipboard_selection(
        &self, id: i64, epoch: Option<u64>, reveal: bool, now: i64,
    ) -> Result<SelectedCopy, WireErrorCode> {
        let sensitive = if reveal { SensitiveAccess::ExplicitUserAction } else { SensitiveAccess::Denied };
        let permit = self.history.privacy().authorize_selected_copy(id, Some(self.instance_id()), sensitive, epoch, now)
            .map_err(denied)?;
        let session_entry = if id < 0 {
            Some(self.session.lock().map_err(|_| WireErrorCode::Unavailable)?.iter()
                .find(|entry| entry.id == id).cloned().ok_or(WireErrorCode::NotFound)?)
        } else {
            None
        };
        Ok(SelectedCopy { id, permit, session_entry })
    }

    fn selected_clipboard_payload(
        &self, selection: &SelectedCopy, derived_text: Option<String>, now: i64,
    ) -> Result<ClipboardPayload, WireErrorCode> {
        let permit = &selection.permit;
        match (&selection.session_entry, derived_text) {
            (Some(entry), Some(text)) => self.history.session_derived_text_payload_for_host_authorized(
                entry, self.instance_id(), text, permit, now,
            ),
            (None, Some(text)) => self.history.derived_text_payload_authorized(selection.id, text, permit, now),
            (Some(entry), None) => self.history.session_clipboard_payload_for_host_authorized(
                entry, self.instance_id(), permit, now,
            ),
            (None, None) => self.history.clipboard_payload_authorized(selection.id, permit, now),
        }.map_err(denied)
    }

    /// The embedded desktop still owns its native format and focus adapters.
    pub fn prepare_embedded_clipboard_selection(
        &self, id: i64, expected_instance: Option<&str>, expected_epoch: Option<u64>,
        derived_text: Option<String>,
    ) -> Result<EmbeddedClipboardSelection, WireErrorCode> {
        if self.mode != HostMode::Embedded || self.is_stopping() {
            return Err(WireErrorCode::Unavailable);
        }
        if id < 0 && expected_instance != Some(self.instance_id()) {
            return Err(WireErrorCode::PrivacyBlocked);
        }
        let _request = self.requests.lock().map_err(|_| WireErrorCode::Unavailable)?;
        let now = crate::router::now_ms()?;
        let selection = self.authorize_clipboard_selection(id, expected_epoch, expected_epoch.is_some(), now)?;
        let derived = derived_text.is_some();
        let entry = if derived {
            None
        } else if let Some(entry) = &selection.session_entry {
            Some(self.history.authorize_session_entry_for_host(entry, self.instance_id(), &selection.permit, now)
                .map_err(denied)?)
        } else {
            Some(self.history.get_entry_authorized(id, &selection.permit, now)
                .map_err(denied)?.ok_or(WireErrorCode::SensitiveAccessDenied)?)
        };
        let payload = self.selected_clipboard_payload(&selection, derived_text, now)?;
        Ok(EmbeddedClipboardSelection {
            entry,
            payload,
            action: EmbeddedClipboardAction {
                instance: self.instance_id().into(),
                permit: selection.permit,
                pasted_entry: (!derived).then_some(id),
            },
        })
    }

    pub fn check_embedded_clipboard_action(&self, action: &EmbeddedClipboardAction) -> Result<(), WireErrorCode> {
        if self.mode != HostMode::Embedded || self.is_stopping() {
            return Err(WireErrorCode::Unavailable);
        }
        if action.instance != self.instance_id() {
            return Err(WireErrorCode::PrivacyBlocked);
        }
        self.history.privacy().check_read(&action.permit, crate::router::now_ms()?).map_err(denied)
    }

    /// Record only a successful native paste. Derived text has no source entry
    /// to delete or promote in the embedded desktop's paste flow.
    pub fn complete_embedded_paste(
        &self, action: EmbeddedClipboardAction, delete_after_use: bool, move_to_top: Option<bool>,
    ) -> Result<PasteHistoryChange, WireErrorCode> {
        let _request = self.requests.lock().map_err(|_| WireErrorCode::Unavailable)?;
        self.check_embedded_clipboard_action(&action)?;
        match action.pasted_entry {
            Some(id) => self.complete_clipboard_use(id, true, delete_after_use, move_to_top),
            None => Ok(PasteHistoryChange::Unchanged),
        }
    }

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
        self.desktop_copy_impl(session, request, #[cfg(any(target_os = "linux", target_os = "windows"))] None, None)
    }

    pub(crate) fn desktop_transform_copy(&self, session: &Session, parameters: Value) -> Result<Value, WireErrorCode> {
        let request: DesktopTransformCopyParams = serde_json::from_value(parameters).map_err(|_| WireErrorCode::InvalidRequest)?;
        let copy = DesktopCopyParams {
            id: request.id, service_instance_id: request.service_instance_id, expected_epoch: request.expected_epoch,
            reveal: request.reveal, paste: false, with_format: None, derived_text: None, image_as_base64: None,
            delete_after_use: None, move_to_top: None, expected_paste_target: None,
        };
        self.desktop_copy_impl(session, copy, #[cfg(any(target_os = "linux", target_os = "windows"))] None, Some(DerivedText::Transform(&request.kind)))
    }

    pub(crate) fn desktop_qr_preview(&self, session: &Session, parameters: Value) -> Result<Value, WireErrorCode> {
        let request: DesktopEntryParams = serde_json::from_value(parameters).map_err(|_| WireErrorCode::InvalidRequest)?;
        self.desktop_permission(session, &request.service_instance_id)?;
        if request.id == 0 || request.thumbnail_dimension.is_some() || request.include_video_frame.is_some() || request.omit_ocr_text.is_some() {
            return Err(WireErrorCode::InvalidRequest);
        }
        let now = crate::router::now_ms()?;
        let access = if request.reveal { SensitiveAccess::ExplicitUserAction } else { SensitiveAccess::Denied };
        let permit = self.history.privacy().authorize_read(PrivacyAccessIntent::InteractiveRead, access, Some(request.expected_epoch), now).map_err(denied)?;
        let row = self.entry_authorized(request.id, &permit, now)?;
        if matches!(row.content_type.as_str(), "image" | "video" | "file" | "files") {
            return Err(WireErrorCode::InvalidRequest);
        }
        let content = qr_content(&row.content)?;
        let png = dzc_core::qr::generate_qr_png(content, 256).map_err(|_| WireErrorCode::InvalidRequest)?;
        let mut chars = content.chars();
        let mut summary: String = chars.by_ref().take(60).collect();
        if chars.next().is_some() {
            summary.push('…');
        }
        let preview = DesktopQrPreview {
            image: format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png)),
            summary,
            content_sha256: qr_content_sha256(content),
        };
        self.history.privacy().check_read(&permit, crate::router::now_ms()?).map_err(denied)?;
        if !self.grants.still_valid(session) {
            return Err(WireErrorCode::PermissionDenied);
        }
        self.authorized_snapshot(session, &permit, &preview)
    }

    pub(crate) fn desktop_qr_copy(&self, session: &Session, parameters: Value) -> Result<Value, WireErrorCode> {
        let request: DesktopQrCopyParams = serde_json::from_value(parameters).map_err(|_| WireErrorCode::InvalidRequest)?;
        let copy = DesktopCopyParams {
            id: request.id, service_instance_id: request.service_instance_id, expected_epoch: request.expected_epoch,
            reveal: request.reveal, paste: false, with_format: None, derived_text: None, image_as_base64: None,
            delete_after_use: None, move_to_top: None, expected_paste_target: None,
        };
        let operation = DerivedText::QrSvg { content_sha256: &request.content_sha256 };
        self.desktop_copy_impl(session, copy, #[cfg(any(target_os = "linux", target_os = "windows"))] None, Some(operation))
    }

    pub(crate) fn desktop_ocr_copy(&self, session: &Session, parameters: Value) -> Result<Value, WireErrorCode> {
        let request: ManualOcrParams = serde_json::from_value(parameters).map_err(|_| WireErrorCode::InvalidRequest)?;
        if request.max_text_bytes.is_some() {
            return Err(WireErrorCode::InvalidRequest);
        }
        let copy = DesktopCopyParams {
            id: request.id, service_instance_id: request.service_instance_id, expected_epoch: request.expected_epoch,
            reveal: request.reveal, paste: false, with_format: None, derived_text: None, image_as_base64: None,
            delete_after_use: None, move_to_top: None, expected_paste_target: None,
        };
        self.desktop_copy_impl(session, copy, #[cfg(any(target_os = "linux", target_os = "windows"))] None, Some(DerivedText::Ocr))
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    pub(crate) fn remember_window_target(&self, session: &Session, params: Value) -> Result<Value, WireErrorCode> {
        self.desktop_permission(session, self.instance_id())?;
        session.require(Permission::ClipboardPaste)?;
        let mut slot = session.window_target.lock().map_err(|_| WireErrorCode::Unavailable)?;
        *slot = None;
        let peer = session.peer.as_ref().ok_or(WireErrorCode::PermissionDenied)?;
        let request: WindowTargetParams = serde_json::from_value(params).map_err(|_| WireErrorCode::InvalidRequest)?;
        // Only presenting the main window may recover the last external target.
        // Global paste shortcuts must reject our own foreground window upfront.
        let target = if request.allow_previous == Some(true) {
            dzc_platform::runtime::RememberedFocus::capture_window(peer)
        } else {
            dzc_platform::runtime::RememberedFocus::capture(peer)
        };
        *slot = Some(target.map_err(|_| WireErrorCode::Unavailable)?);
        encoded(OperationResult { performed: true })
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
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

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    fn take_window_target(&self, session: &Session) -> Result<dzc_platform::runtime::QueueInputTarget, WireErrorCode> {
        let destination = session.window_target.lock().map_err(|_| WireErrorCode::Unavailable)?
            .take().ok_or(WireErrorCode::NotFound)?;
        let target = destination.restore().map_err(|_| WireErrorCode::Unavailable)?;
        dzc_platform::runtime::QueueInputTarget::prepare(target).map_err(|_| WireErrorCode::Unavailable)
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
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

    #[cfg(any(target_os = "linux", target_os = "windows"))]
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

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    pub(crate) fn desktop_copy_bound(&self, session: &Session, request: DesktopCopyParams, bound: Option<(dzc_platform::runtime::QueueInputTarget, RuntimeState)>) -> Result<Value,WireErrorCode> {
        self.desktop_copy_impl(session, request, bound, None)
    }

    fn desktop_copy_impl(
        &self,
        session: &Session,
        request: DesktopCopyParams,
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        bound: Option<(dzc_platform::runtime::QueueInputTarget, RuntimeState)>,
        derived: Option<DerivedText<'_>>,
    ) -> Result<Value, WireErrorCode> {
        self.desktop_permission(session,&request.service_instance_id)?;
        if !self.actions.can_copy() || request.paste && !self.actions.can_paste() {return Err(WireErrorCode::UnsupportedCapability);}
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        let use_bound_target = false;
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        let use_bound_target = bound.is_some();
        let paste_target = if use_bound_target { None } else { self.prepare_desktop_paste(request.paste, request.expected_paste_target.as_ref())? };
        let now=crate::router::now_ms()?;
        let generated=request.derived_text.is_some() || request.image_as_base64.unwrap_or(false) || derived.is_some();
        // A Service started without capture still handles explicit copy actions.
        // An unexpectedly unavailable capture owner still fails closed.
        let capture_generated = generated && !self.capture_disabled_by_request()?;
        let ticket=if capture_generated {match self.history.privacy().capture_ticket(now) {
            Ok(ticket)=>Some(ticket),Err(dzc_core::api::PrivacyError::PrivacySessionActive)=>None,Err(error)=>return Err(denied(error))}}
            else {None};
        let selection = self.authorize_clipboard_selection(request.id, Some(request.expected_epoch), request.reveal, now)?;
        let permit = &selection.permit;
        let ocr = matches!(derived, Some(DerivedText::Ocr));
        let mut payload = if ocr {
            self.history.ocr_text_payload_authorized(request.id, permit, now).map_err(denied)?
        } else {
            self.selected_clipboard_payload(&selection, request.derived_text, now)?
        };
        if request.image_as_base64.unwrap_or(false) {payload=image_text(payload)?;}
        if let Some(operation) = derived.filter(|_| !ocr) {
            payload = derive_text_payload(payload, &operation)?;
        }
        if request.with_format==Some(false) {payload=plain(payload);}
        if let Some(ticket)=ticket {
            let text=payload_text(&payload).ok_or(WireErrorCode::InvalidRequest)?;
            let frame=CaptureFrame {data:CaptureData::Text(text.into()),metadata:CaptureReadMetadata{privacy_ticket:Some(ticket),inherited_protection:matches!(payload,ClipboardPayload::Private{..}),..Default::default()}};
            self.capture_generated(frame,CaptureSource{app_name:"DezirClip Derived".into(),process_path:None},now)?;
        }
        self.history.privacy().check_read(permit,crate::router::now_ms()?).map_err(denied)?;
        #[cfg(any(target_os = "linux", target_os = "windows"))]
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
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        let selected_state = bound.as_ref().map(|(_,state)| state.clone());
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        let check_selection = || -> Result<(), WireErrorCode> {
            if let Some(state) = &selected_state {
                if !self.grants.still_valid(session) { return Err(WireErrorCode::PermissionDenied); }
                if !crate::same_surface_state(state, &self.snapshot()?) { return Err(WireErrorCode::HistoryRevisionChanged); }
            }
            Ok(())
        };
        #[cfg(any(target_os = "linux", target_os = "windows"))]
        check_selection()?;
        self.copy_payload(payload)?;
        if request.paste {
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            check_selection()?;
            self.history.privacy().check_read(permit,crate::router::now_ms()?).map_err(denied)?;
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            let sent = if let Some(((target, _), (method, kind, text))) = bound.zip(input) {
                target.send(&method, text.as_deref(), kind, || !self.is_stopping() && check_selection().is_ok()
                    && crate::router::now_ms().is_ok_and(|now| self.history.privacy().check_read(permit, now).is_ok()))
                    .map_err(|_| WireErrorCode::Unavailable)?;
                true
            } else { false };
            #[cfg(not(any(target_os = "linux", target_os = "windows")))]
            let sent = false;
            if !sent {
                if let Some(target) = paste_target { target.send()?; } else { self.actions.paste()?; }
            }
            #[cfg(any(target_os = "linux", target_os = "windows"))]
            self.play_feedback(dzc_platform::sound::Cue::Paste);
        }
        self.complete_clipboard_use(request.id, request.paste, request.delete_after_use == Some(true), request.move_to_top)?;
        encoded(OperationResult{performed:true})
    }

    fn complete_clipboard_use(
        &self, id: i64, pasted: bool, delete_after_use: bool, move_to_top: Option<bool>,
    ) -> Result<PasteHistoryChange, WireErrorCode> {
        if delete_after_use {
            if id < 0 {
                let mut rows = self.session.lock().map_err(|_| WireErrorCode::Unavailable)?;
                if !rows.iter().any(|row| row.id == id) {
                    return Ok(PasteHistoryChange::Unchanged);
                }
                rows.retain(|row| row.id != id);
            } else {
                self.history.delete(id, Some(&self.directory)).map_err(|_| WireErrorCode::Unavailable)?;
            }
            self.publish(EventKind::EntryDeleted, EventData { entry_id: Some(id), status: None })?;
            Ok(PasteHistoryChange::Removed(id))
        } else if pasted && id > 0 {
            self.record_desktop_paste(id, move_to_top)?;
            Ok(PasteHistoryChange::Updated(id))
        } else {
            Ok(PasteHistoryChange::Unchanged)
        }
    }
    pub(super) fn record_desktop_paste(&self, id: i64, move_to_top: Option<bool>) -> Result<(), WireErrorCode> {
        self.history.record_paste_use(id, crate::router::now_ms()?, move_to_top)
            .map_err(|_| WireErrorCode::Unavailable)?;
        self.publish_usage_changed(id)
    }
    pub(super) fn publish_usage_changed(&self, id: i64) -> Result<(), WireErrorCode> {
        self.publish(EventKind::EntryUpdated, EventData { entry_id: Some(id), status: Some("usage_changed".into()) })
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
        if request.expected_entry_sha256.is_some() && request.action != "edit" {return Err(WireErrorCode::InvalidRequest);}
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
            self.publish_usage_changed(request.id)?;
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
        let status = if request.action == "pin" {
            request.pinned.map(|pinned| if pinned { "pin_set" } else { "pin_cleared" }.into())
        } else {
            None
        };
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
                    self.history.update_session_manual_authorized(&mut rows[index],&content,&preview,intent,&permit,now,request.expected_entry_sha256.as_deref()).map_err(denied)?;
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
            self.publish(EventKind::EntryUpdated, EventData { entry_id: Some(result_id), status })?;
            return Ok(DesktopMutationResult{id:result_id,performed:true});
        }
        match request.action.as_str() {
            "pin"=>self.history.update_entry_metadata_authorized(request.id,Some(request.pinned.ok_or(WireErrorCode::InvalidRequest)?),None,&permit,now).map_err(denied)?,
            "tags"=>self.history.update_entry_metadata_authorized(request.id,None,Some(request.tags.ok_or(WireErrorCode::InvalidRequest)?),&permit,now).map_err(denied)?,
            "edit"=>{let content=request.content.ok_or(WireErrorCode::InvalidRequest)?;let preview:String=content.chars().take(500).collect();
                self.history.update_manual_checked(request.id,&content,&preview,intent,&permit,now,request.expected_entry_sha256.as_deref()).map_err(denied)?;}
            _=>return Err(WireErrorCode::InvalidRequest),
        }
        self.publish(EventKind::EntryUpdated, EventData { entry_id: Some(request.id), status })?;
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

fn derive_text_payload(payload: ClipboardPayload, operation: &DerivedText<'_>) -> Result<ClipboardPayload, WireErrorCode> {
    match payload {
        ClipboardPayload::Text(text) | ClipboardPayload::Html { text, .. } => {
            let result = match operation {
                DerivedText::Transform(kind) => transform_text(&text, kind)?,
                DerivedText::QrSvg { content_sha256 } => {
                    let content = qr_content(&text)?;
                    if qr_content_sha256(content) != *content_sha256 {
                        return Err(WireErrorCode::HistoryRevisionChanged);
                    }
                    dzc_core::qr::generate_qr_svg(content).map_err(|_| WireErrorCode::InvalidRequest)?
                }
                DerivedText::Ocr => return Err(WireErrorCode::InvalidRequest),
            };
            Ok(ClipboardPayload::Text(result))
        }
        ClipboardPayload::Private { payload, authorization } => Ok(ClipboardPayload::Private {
            payload: Box::new(derive_text_payload(*payload, operation)?),
            authorization,
        }),
        _ => Err(WireErrorCode::InvalidRequest),
    }
}

fn transform_text(text: &str, kind: &str) -> Result<String, WireErrorCode> {
    if text.chars().take(250_001).count() > 250_000 {
        return Err(WireErrorCode::FrameTooLarge);
    }
    let result = dzc_core::transforms::transform_text(text, kind).map_err(|_| WireErrorCode::InvalidRequest)?;
    if result.chars().take(524_289).count() > 524_288 {
        return Err(WireErrorCode::FrameTooLarge);
    }
    Ok(result)
}

fn qr_content(content: &str) -> Result<&str, WireErrorCode> {
    // 7,089 ASCII digits is the maximum payload of any standard QR symbol.
    if content.is_empty() || content.len() > 7_089 {
        return Err(WireErrorCode::InvalidRequest);
    }
    Ok(content)
}

fn qr_content_sha256(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

#[cfg(test)]
mod qr_copy_tests {
    use super::*;

    #[test]
    fn qr_svg_copy_requires_the_text_shown_in_the_preview() {
        let source = "https://example.test/你好";
        let sha = qr_content_sha256(source);
        let operation = DerivedText::QrSvg { content_sha256: &sha };
        let html = ClipboardPayload::Html { text: source.into(), html: format!("<p>{source}</p>") };
        let generated = derive_text_payload(html, &operation).unwrap();
        assert_eq!(payload_text(&generated), Some(dzc_core::qr::generate_qr_svg(source).unwrap().as_str()));
        let changed = ClipboardPayload::Text(format!("{source}?updated"));
        assert!(matches!(derive_text_payload(changed, &operation), Err(WireErrorCode::HistoryRevisionChanged)));
    }
}

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
        fn revoke_private_exports(&self) -> Result<(), WireErrorCode> { Ok(()) }
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
    fn embedded_paste_tracks_the_original_selection_until_native_input_finishes() {
        let original = crate::fixtures::synthetic_host();
        let open = || Host::new(crate::HostOptions {
            mode: HostMode::Embedded,
            history: original.history.clone(),
            directory: original.directory.clone(),
            actions: Arc::new(Actions::default()),
        }).unwrap();
        let host = open();
        let mut entry: ClipboardEntry = serde_json::from_value(serde_json::json!({
            "id": 0, "content_type": "text", "content": "selected content", "preview": "selected content",
            "source_app": "fixture", "timestamp": 3, "is_pinned": false
        })).unwrap();
        let id = host.history.save(&entry, None).unwrap();
        let clipboard = host.prepare_embedded_clipboard_selection(id, None, None, None).unwrap();
        assert_eq!(clipboard.entry.unwrap().content, "selected content");
        assert!(matches!(clipboard.payload, ClipboardPayload::Text(text) if text == "selected content"));
        assert_eq!(host.history.get_entry_by_id(id).unwrap().unwrap().use_count, 0);
        assert_eq!(open().check_embedded_clipboard_action(&clipboard.action), Err(WireErrorCode::PrivacyBlocked));
        assert!(matches!(host.complete_embedded_paste(clipboard.action, false, Some(false)),
            Ok(PasteHistoryChange::Updated(updated)) if updated == id));
        let stored = host.history.get_entry_by_id(id).unwrap().unwrap();
        assert_eq!((stored.timestamp, stored.use_count), (3, 1));

        let derived = host.prepare_embedded_clipboard_selection(id, None, None, Some("replacement".into())).unwrap();
        assert!(derived.entry.is_none());
        assert!(matches!(host.complete_embedded_paste(derived.action, true, None),
            Ok(PasteHistoryChange::Unchanged)));
        assert!(host.history.get_entry_by_id(id).unwrap().is_some());

        entry.id = -7;
        host.session.lock().unwrap().push_front(entry);
        assert!(matches!(host.prepare_embedded_clipboard_selection(-7, None, None, None),
            Err(WireErrorCode::PrivacyBlocked)));
        let ephemeral = host.prepare_embedded_clipboard_selection(-7, Some(host.instance_id()), None, None).unwrap();
        assert!(matches!(host.complete_embedded_paste(ephemeral.action, true, None),
            Ok(PasteHistoryChange::Removed(-7))));
        assert!(host.session.lock().unwrap().is_empty());

        let stale = host.prepare_embedded_clipboard_selection(id, None, None, None).unwrap();
        host.history.privacy().toggle(crate::router::now_ms().unwrap()).unwrap();
        assert_eq!(host.check_embedded_clipboard_action(&stale.action), Err(WireErrorCode::PrivacyEpochChanged));
        assert!(matches!(host.complete_embedded_paste(stale.action, true, None), Err(WireErrorCode::PrivacyEpochChanged)));
        assert!(host.history.get_entry_by_id(id).unwrap().is_some());
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
            fn revoke_private_exports(&self) -> Result<(), WireErrorCode> { Ok(()) }
        }
        for mode in [HostMode::Embedded,HostMode::Service] {
            for delete in [false,true] {
                let original=crate::fixtures::synthetic_host();
                let host=Host::new(crate::HostOptions{mode,history:original.history.clone(),directory:original.directory.clone(),actions:Arc::new(FailingPaste)}).unwrap();
                let entry=serde_json::from_value(serde_json::json!({"id":0,"content_type":"text","content":"synthetic rejected paste","preview":"synthetic","source_app":"Synthetic","timestamp":1,"is_pinned":false})).unwrap();
                let id=host.history.save(&entry,None).unwrap();
                let query=dzc_core::api::HistoryQuery{id:Some(id),metadata:true,limit:1,..Default::default()};
                let before=host.history.query(&query).unwrap();
                let owner=session(&host,ClientRole::Desktop);let state=host.snapshot().unwrap();
                let cursor=state.event_sequence;
                let failed=call(&host,&owner,Method::DesktopClipboardCopy,serde_json::json!({"id":id,"service_instance_id":state.service_instance_id,"expected_epoch":state.privacy_epoch,"reveal":false,"paste":true,"expected_paste_target":crate::fixtures::windows_target(),"delete_after_use":delete,"move_to_top":true}));
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
                assert_eq!(*target,crate::fixtures::windows_target());
                Ok(Box::new(Fixed{current:self.current.clone(),sent:self.sent.clone()}))
            }
            fn stop_helpers(&self)->Result<(),WireErrorCode> {Ok(())}
            fn revoke_private_exports(&self) -> Result<(), WireErrorCode> { Ok(()) }
        }
        for (missing, change, preference, requested) in [
            (true, false, true, None),
            (false, true, true, None),
            (false, false, false, None),
            (false, false, true, Some(false)),
            (false, false, true, None),
        ] {
            let original=crate::fixtures::synthetic_host();
            let actions=Arc::new(Bound{current:Arc::new(AtomicBool::new(true)),sent:Arc::new(AtomicUsize::new(0)),copies:AtomicUsize::new(0),change});
            let host=Host::new(crate::HostOptions{mode:HostMode::Service,history:original.history.clone(),directory:original.directory.clone(),actions:actions.clone()}).unwrap();
            let entry=serde_json::from_value(serde_json::json!({"id":0,"content_type":"text","content":"synthetic bound target","preview":"synthetic","source_app":"Synthetic","timestamp":1,"is_pinned":false})).unwrap();
            let id=host.history.save(&entry,None).unwrap();
            let owner=session(&host,ClientRole::Desktop);let state=host.snapshot().unwrap();
            host.history.update_queue_setting(
                "app.move_to_top_after_paste",
                if preference { "true" } else { "false" },
                state.privacy_epoch,
            ).unwrap();
            let mut params=serde_json::json!({"id":id,"service_instance_id":state.service_instance_id,"expected_epoch":state.privacy_epoch,"reveal":false,"paste":true,"delete_after_use":false});
            if let Some(move_to_top) = requested {
                params["move_to_top"] = move_to_top.into();
            }
            if !missing {params["expected_paste_target"]=serde_json::to_value(crate::fixtures::windows_target()).unwrap();}
            let response=call(&host,&owner,Method::DesktopClipboardCopy,params);
            assert_eq!(response.error.is_none(),!missing && !change);
            assert_eq!(actions.copies.load(Ordering::SeqCst),if !missing {1}else{0});
            assert_eq!(actions.sent.load(Ordering::SeqCst),if !missing && !change {1}else{0});
            let rows=host.history.query(&dzc_core::api::HistoryQuery{id:Some(id),metadata:true,limit:1,..Default::default()}).unwrap();
            assert_eq!(rows.len(),1);
            assert_eq!(rows[0].use_count,if !missing && !change {1}else{0});
            assert_eq!(rows[0].timestamp > 1, !missing && !change && requested.unwrap_or(preference));
        }
    }
    #[test]
    fn desktop_paste_notifies_observers_and_promotes_only_when_requested() {
        let host = crate::fixtures::synthetic_host();
        let entry = serde_json::from_value(serde_json::json!({
            "id": 0, "content_type": "text", "content": "first synthetic entry",
            "preview": "example", "source_app": "Synthetic", "timestamp": 1, "is_pinned": false
        })).unwrap();
        let id = host.history.save(&entry, None).unwrap();
        let mut newer = entry;
        newer.content = "newer synthetic entry".into();
        newer.timestamp = 4;
        let newer_id = host.history.save(&newer, None).unwrap();
        let first_id = || {
            host.history.query(&dzc_core::api::HistoryQuery { metadata: true, limit: 1, ..Default::default() })
                .unwrap()[0].id
        };
        let owner = session(&host, ClientRole::Desktop);
        let cursor = host.snapshot().unwrap().event_sequence;
        let query = dzc_core::api::HistoryQuery { id: Some(id), metadata: true, limit: 1, ..Default::default() };
        let epoch = host.snapshot().unwrap().privacy_epoch;
        host.history.update_queue_setting("app.move_to_top_after_paste", "false", epoch).unwrap();
        let before = host.snapshot().unwrap().history_revision;
        host.record_desktop_paste(id, None).unwrap();
        assert_eq!(host.snapshot().unwrap().history_revision, before + 1);
        let after = &host.history.query(&query).unwrap()[0];
        assert_eq!((after.timestamp, after.use_count), (1, 1));
        assert_eq!(first_id(), newer_id);
        // The protocol also accepts an explicit policy for individual actions.
        host.record_desktop_paste(id, Some(true)).unwrap();
        let after = &host.history.query(&query).unwrap()[0];
        assert!(after.timestamp > 1);
        assert_eq!(after.use_count, 2);
        assert_eq!(first_id(), id);
        host.history.update_queue_setting("app.move_to_top_after_paste", "true", epoch).unwrap();
        host.history.touch_entry(id, 2).unwrap();
        host.record_desktop_paste(id, Some(false)).unwrap();
        let after = &host.history.query(&query).unwrap()[0];
        assert_eq!((after.timestamp, after.use_count), (2, 3));
        host.record_desktop_paste(id, None).unwrap();
        let after = &host.history.query(&query).unwrap()[0];
        assert!(after.timestamp > 2);
        assert_eq!(after.use_count, 4);
        host.history.toggle_pin(id, true).unwrap();
        host.history.touch_entry(id, 3).unwrap();
        host.record_desktop_paste(id, None).unwrap();
        let after = &host.history.query(&query).unwrap()[0];
        assert_eq!((after.timestamp, after.use_count), (3, 5));
        let events: EventsResult = serde_json::from_value(call(&host, &owner, Method::EventsSubscribe,
            serde_json::json!({"after_sequence": cursor, "wait_ms": 0})).result.unwrap()).unwrap();
        let updates: Vec<_> = events.events.iter().filter(|event| event.event == EventKind::EntryUpdated).collect();
        assert_eq!(updates.len(), 5);
        assert!(updates.iter().all(|event| event.data.entry_id == Some(id) &&
            event.data.status.as_deref() == Some("usage_changed")));
    }

    #[test]
    fn host_copy_only_does_not_paste_delete_or_count_and_completion_only_updates_metadata() {
        let original=crate::fixtures::synthetic_host();
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
        let host=crate::fixtures::synthetic_host();let owner=session(&host,ClientRole::Desktop);let state=host.snapshot().unwrap();
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
