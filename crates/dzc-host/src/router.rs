use crate::{authorization::{GrantAuthority, Session}, events::Events};
use dzc_core::api::{self, ClipboardEntry, HistoryService, PrivacyAccessIntent, PrivacySessionCommand, SensitiveAccess};
use dzc_protocol::wire::{self, *};
use serde::Serialize;
use serde_json::Value;
use std::{collections::VecDeque, path::PathBuf, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}, time::{Duration, SystemTime, UNIX_EPOCH}};

pub trait HostActions: Send + Sync {
    fn platform_capabilities(&self) -> Value;
    fn can_copy(&self) -> bool;
    fn copy(&self, payload: dzc_core::ports::ClipboardPayload) -> Result<(), WireErrorCode>;
    fn can_paste(&self) -> bool;
    fn paste(&self) -> Result<(), WireErrorCode>;
    fn can_activate(&self) -> bool { false }
    fn activate(&self, _entry_id: Option<i64>) -> Result<(), WireErrorCode> { Err(WireErrorCode::Unavailable) }
    fn dismiss(&self) -> Result<(), WireErrorCode> { Err(WireErrorCode::Unavailable) }
    fn stop_helpers(&self) -> Result<(), WireErrorCode>;
    fn revoke_private_exports(&self) -> Result<(), WireErrorCode> {
        dzc_platform::runtime::revoke_private_exports().map_err(|_| WireErrorCode::Busy)
    }
    fn privacy_changed(&self, _snapshot: &api::RuntimeSnapshot) {}
}

pub trait HostLifecycle: Send + Sync {
    fn stop_capture(&self) -> Result<(), WireErrorCode>;
    fn release_ownership(&self) -> Result<(), WireErrorCode> { Ok(()) }
    fn capture_lease(&self) -> Option<Arc<dzc_platform::ipc::CaptureLease>> { None }
    fn capture_generated(&self, _frame:dzc_core::capture::CaptureFrame, _source:dzc_core::capture::CaptureSource, _timestamp:i64) -> Result<bool,WireErrorCode> {Err(WireErrorCode::Unavailable)}
    fn capture_active(&self) -> bool { false }
    fn begin_work(&self)->Result<crate::CaptureWork,WireErrorCode> {Err(WireErrorCode::Unavailable)}
}

pub struct HostOptions {
    pub mode: HostMode,
    pub history: HistoryService,
    pub directory: PathBuf,
    pub actions: Arc<dyn HostActions>,
}

pub struct Host {
    pub(crate) history: HistoryService,
    pub(crate) directory: PathBuf,
    pub(crate) session: Arc<Mutex<VecDeque<ClipboardEntry>>>,
    pub(crate) grants: GrantAuthority,
    pub(crate) actions: Arc<dyn HostActions>,
    mode: HostMode,
    instance: String,
    events: Events,
    pub(crate) stop_requested: AtomicBool,
    pub(crate) capture_owner: AtomicBool,
    pub(crate) lifecycle: Mutex<Option<Arc<dyn HostLifecycle>>>,
    pub(crate) requests: Mutex<()>,
    stopped: AtomicBool,
    pub(crate) snapshots: crate::desktop::Snapshots,
    observed_epoch: Mutex<u64>,
    hotkey: Mutex<Option<crate::hotkey::Hotkey>>,
    pub(crate) uploads: crate::upload::Uploads,
    copy_echo:Mutex<Option<(dzc_platform::capture::ClipboardEcho,i64)>>,
    capture_diagnostic:Mutex<Option<String>>,
}

pub fn now_ms() -> Result<i64, WireErrorCode> {
    i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| WireErrorCode::Unavailable)?.as_millis()).map_err(|_| WireErrorCode::Unavailable)
}
fn unavailable<T>(_: T) -> WireErrorCode { WireErrorCode::Unavailable }
pub(crate) fn privacy_error(error:impl ToString) -> WireErrorCode {
    match error.to_string().as_str() {
        "PrivacyStateUnavailable"=>WireErrorCode::PrivacyStateUnavailable,"PrivacyEpochChanged"=>WireErrorCode::PrivacyEpochChanged,
        "PrivacySessionActive"=>WireErrorCode::PrivacySessionActive,"SensitiveAccessDenied"=>WireErrorCode::SensitiveAccessDenied,
        "ManualPersistenceConfirmationRequired"=>WireErrorCode::ManualPersistenceConfirmationRequired,"PrivacyDurationInvalid"=>WireErrorCode::PrivacyDurationInvalid,
        _=>WireErrorCode::PrivacyBlocked,
    }
}
fn result<T: Serialize>(schema: &str, value: T) -> Result<Value, WireErrorCode> {
    let value = serde_json::to_value(value).map_err(|_| WireErrorCode::Internal)?;
    wire::validate(schema, &value).map_err(|_| WireErrorCode::Internal)?;
    Ok(value)
}
fn params<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, WireErrorCode> {
    serde_json::from_value(value).map_err(|_| WireErrorCode::InvalidRequest)
}

impl Host {
    pub fn new(options: HostOptions) -> Result<Arc<Self>, WireErrorCode> {
        Self::with_session(options, Arc::new(Mutex::new(VecDeque::new())))
    }
    pub fn with_session(options: HostOptions, session: Arc<Mutex<VecDeque<ClipboardEntry>>>) -> Result<Arc<Self>, WireErrorCode> {
        let epoch = options.history.runtime_snapshot(now_ms()?).map_err(unavailable)?.privacy_epoch;
        let host = Arc::new(Self { history: options.history, directory: options.directory, session, mode: options.mode, actions: options.actions,
            grants: GrantAuthority::default(), instance: uuid::Uuid::new_v4().to_string(), events: Events::new(),
            stop_requested: AtomicBool::new(false), capture_owner: AtomicBool::new(false), lifecycle: Mutex::new(None), requests: Mutex::new(()), stopped: AtomicBool::new(false),
            snapshots: Default::default(), observed_epoch: Mutex::new(epoch), hotkey: Mutex::new(None), uploads:Default::default(),copy_echo:Mutex::new(None),capture_diagnostic:Mutex::new(None) });
        *host.hotkey.lock().map_err(unavailable)? = Some(crate::hotkey::Hotkey::new(Arc::downgrade(&host)));
        Ok(host)
    }
    pub fn attach_lifecycle(&self, lifecycle: Arc<dyn HostLifecycle>) -> Result<(), WireErrorCode> {
        let mut current = self.lifecycle.lock().map_err(unavailable)?;
        if current.is_some() || self.is_stopping() { return Err(WireErrorCode::Busy); }
        *current = Some(lifecycle);
        Ok(())
    }
    pub fn grants(&self) -> &GrantAuthority { &self.grants }
    #[cfg(test)]
    pub(crate) fn waiting_subscriptions(&self) -> usize { self.events.waiters() }
    pub fn is_stopping(&self) -> bool { self.stop_requested.load(Ordering::Acquire) }
    pub fn capture_stopped(&self) -> bool { self.stopped.load(Ordering::Acquire) }
    pub fn set_capture_diagnostic(&self,reason:&str)->Result<(),WireErrorCode> {
        if !matches!(reason,"native_listener_unavailable"|"capture_ownership_unavailable"|"disabled_by_request") {return Err(WireErrorCode::InvalidRequest);}
        *self.capture_diagnostic.lock().map_err(unavailable)?=Some(reason.into());Ok(())
    }
    fn capture_active(&self) -> bool {
        self.capture_owner.load(Ordering::Acquire) && self.lifecycle.lock().ok().and_then(|slot|slot.as_ref().map(|lifecycle|lifecycle.capture_active())).unwrap_or(false)
    }
    /// Trusted in-process lifecycle entry; the IPC router separately checks its grant.
    pub fn shutdown(&self) -> Result<(), WireErrorCode> {
        let _serial = self.requests.lock().map_err(unavailable)?;
        self.stop()
    }
    fn stop(&self) -> Result<(), WireErrorCode> {
        if self.stopped.load(Ordering::Acquire) {return Ok(());}
        self.stop_requested.store(true, Ordering::Release);
        self.events.interrupt();
        if let Some(lifecycle) = self.lifecycle.lock().map_err(unavailable)?.as_ref() { lifecycle.stop_capture()?; }
        if let Some(hotkey) = self.hotkey.lock().map_err(unavailable)?.as_mut() { hotkey.stop()?; }
        self.actions.revoke_private_exports()?;
        self.snapshots.clear()?;
        self.uploads.clear()?;
        self.actions.stop_helpers()?;
        if let Some(lifecycle) = self.lifecycle.lock().map_err(unavailable)?.as_ref() { lifecycle.release_ownership()?; }
        self.stopped.store(true, Ordering::Release);
        Ok(())
    }
    pub fn snapshot(&self) -> Result<RuntimeState, WireErrorCode> {
        let snapshot = self.history.runtime_snapshot(now_ms()?).map_err(unavailable)?;
        let state = RuntimeState { service_instance_id: self.instance.clone(), history_revision: snapshot.history_revision,
            privacy_epoch: snapshot.privacy_epoch, event_sequence: self.events.sequence().map_err(unavailable)? };
        result("RuntimeState", &state)?;
        Ok(state)
    }
    /// Observe committed cross-process Core policy changes before acknowledging
    /// a request. Failed native revocation leaves the old observed epoch so the
    /// next request retries; no successful ACK or privacy event is published.
    pub(crate) fn synchronize_privacy(&self) -> Result<(), WireErrorCode> {
        let snapshot = self.history.runtime_snapshot(now_ms()?).map_err(unavailable)?;
        let mut observed = self.observed_epoch.lock().map_err(unavailable)?;
        if *observed == snapshot.privacy_epoch { return Ok(()); }
        self.actions.revoke_private_exports()?;
        self.snapshots.clear()?;
        self.uploads.clear()?;
        let state = RuntimeState { service_instance_id: self.instance.clone(), history_revision: snapshot.history_revision,
            privacy_epoch: snapshot.privacy_epoch, event_sequence: self.events.sequence().map_err(unavailable)? };
        self.events.push(EventKind::PrivacyStateChanged, state, EventData { entry_id: None, status: None }).map_err(unavailable)?;
        *observed = snapshot.privacy_epoch;
        self.actions.privacy_changed(&snapshot);
        Ok(())
    }
    pub fn initialize_privacy_shortcut(&self) -> Result<(), WireErrorCode> {
        let configured = self.history.privacy().hotkey().map_err(privacy_error)?;
        if configured.is_empty() { return Ok(()); }
        self.hotkey.lock().map_err(unavailable)?.as_mut().ok_or(WireErrorCode::Unavailable)?.set(&configured)?;
        Ok(())
    }
    /// In-process generated actions carry the same opaque ticket as IPC uploads.
    pub fn capture_generated(&self, frame:dzc_core::capture::CaptureFrame, source:dzc_core::capture::CaptureSource, timestamp:i64)->Result<bool,WireErrorCode> {
        if self.is_stopping() {return Err(WireErrorCode::Busy);}
        let lifecycle=self.lifecycle.lock().map_err(unavailable)?.clone().ok_or(WireErrorCode::Unavailable)?;
        lifecycle.capture_generated(frame,source,timestamp)
    }
    pub fn begin_work(&self)->Result<crate::CaptureWork,WireErrorCode> {
        if self.is_stopping() {return Err(WireErrorCode::Busy);}
        self.lifecycle.lock().map_err(unavailable)?.as_ref().ok_or(WireErrorCode::Unavailable)?.begin_work()
    }
    pub(crate) fn try_toggle_privacy(&self) -> Result<bool, WireErrorCode> {
        let _request = match self.requests.try_lock() {
            Ok(request) => request,
            Err(std::sync::TryLockError::WouldBlock) => return Ok(false),
            Err(_) => return Err(WireErrorCode::Unavailable),
        };
        if self.is_stopping() { return Err(WireErrorCode::Busy); }
        self.history.privacy().toggle(now_ms()?).map_err(privacy_error)?;
        self.synchronize_privacy()?;
        Ok(true)
    }
    pub fn publish(&self, event: EventKind, data: EventData) -> Result<(), WireErrorCode> {
        self.synchronize_privacy()?;
        result("EventData", &data)?;
        self.events.push(event, self.snapshot()?, data).map_err(unavailable)
    }
    pub(crate) fn copy_payload(&self,payload:dzc_core::ports::ClipboardPayload)->Result<(),WireErrorCode> {
        let hint=dzc_platform::capture::clipboard_echo(&payload).map_err(|_|WireErrorCode::Unavailable)?;
        let timestamp=now_ms()?;
        *self.copy_echo.lock().map_err(unavailable)?=hint.map(|hint|(hint,timestamp));
        let result=self.actions.copy(payload);
        if result.is_err() {self.clear_copy_echo();}
        result
    }
    pub(crate) fn own_copy(&self,timestamp:i64)->dzc_core::capture::OwnClipboardCopy {
        self.copy_echo.lock().ok().and_then(|hint|*hint).map(|(hint,time)|dzc_core::capture::OwnClipboardCopy{
            primary:hint.primary,alternative:hint.alternative,age_seconds:timestamp.saturating_sub(time).max(0) as u64/1000}).unwrap_or_default()
    }
    pub(crate) fn clear_copy_echo(&self) {if let Ok(mut hint)=self.copy_echo.lock(){*hint=None;}}
    fn privacy_state(&self) -> Result<PrivacyState, WireErrorCode> {
        let snapshot = self.history.runtime_snapshot(now_ms()?).map_err(unavailable)?;
        Ok(PrivacyState { active: snapshot.session.active, expires_at_ms: snapshot.session.expires_at_ms, privacy_epoch: snapshot.privacy_epoch })
    }
    pub fn hello(&self, hello: HelloParams) -> Result<(Session, HelloResult), WireErrorCode> {
        self.synchronize_privacy()?;
        if hello.protocol_version.major != wire::VERSION.major { return Err(WireErrorCode::ProtocolMismatch); }
        let session = self.grants.accept(&hello)?;
        let available = self.capabilities(&session);
        if hello.required_capabilities.iter().any(|capability| !available.methods.contains(capability)
            && !available.permissions.iter().any(|permission| serde_json::to_value(permission).ok().and_then(|value| value.as_str().map(str::to_owned)).as_ref() == Some(capability))) {
            return Err(WireErrorCode::UnsupportedCapability);
        }
        let response = HelloResult { session_id: session.id().into(), protocol_version: wire::VERSION,
            core_version: env!("CARGO_PKG_VERSION").into(), permissions: session.permissions().to_vec(), state: self.snapshot()?, max_frame_bytes: wire::MAX_FRAME_BYTES as u32 };
        result("HelloResult", &response)?;
        Ok((session, response))
    }
    fn capabilities(&self, session: &Session) -> CapabilityReport {
        let service_transition = dzc_platform::ipc::ProcessExit::ensure_supported().is_ok();
        let mut methods = vec!["capabilities", "health", "history.list", "history.search", "privacy.get", "events.subscribe", "service.status", "surface.capabilities", "surface.history.list", "surface.history.search"];
        let can = |permission| session.permissions().contains(&permission);
        if can(Permission::HistoryBody) { methods.push("history.get"); }
        if can(Permission::ClipboardCopy) && self.actions.can_copy() { methods.extend(["clipboard.copy", "surface.copy"]); }
        if can(Permission::ClipboardPaste) && self.actions.can_copy() && self.actions.can_paste() { methods.push("surface.paste"); }
        if can(Permission::PrivacyWrite) { methods.extend(["privacy.set", "privacy.toggle", "privacy.hotkey.get", "privacy.hotkey.set"]); }
        if can(Permission::DesktopHistory) && session.role() == ClientRole::Desktop {
            methods.extend(["desktop.history.page", "desktop.history.get", "desktop.history.summary", "desktop.history.mutate", "desktop.snapshot.read", "desktop.capture.begin", "desktop.capture.write", "desktop.capture.finish"]);
            if self.actions.can_copy() {methods.push("desktop.clipboard.copy");}
            if can(Permission::PrivacyWrite) {methods.extend(["desktop.privacy.rules", "desktop.privacy.decision", "desktop.privacy.feedback", "desktop.privacy.remove_rule"]);}
        }
        if can(Permission::ServiceManage) {
            methods.extend(["service.stop", "service.grant", "service.revoke"]);
            if service_transition { methods.push("service.handoff"); }
        }
        if can(Permission::SurfaceControl) && self.actions.can_activate() { methods.extend(["surface.activate", "surface.dismiss"]); }
        let mut platform = self.actions.platform_capabilities();
        if let Some(platform) = platform.as_object_mut() {
            if can(Permission::DesktopHistory) && session.role()==ClientRole::Desktop {
                platform.insert("desktop_generated_copy".into(),serde_json::json!({"supported":self.actions.can_copy(),"version":1}));
            }
            platform.insert("service_start".into(),serde_json::json!({"supported":service_transition,
                "reason":if service_transition {"available"} else {"process_exit_observation_unavailable"}}));
        }
        CapabilityReport { methods: methods.into_iter().map(str::to_owned).collect(), permissions: session.permissions().to_vec(), platform }
    }
    pub fn dispatch(&self, session: &Session, request: Request) -> Response {
        let id = request.request_id.clone();
        if serde_json::to_value(&request).ok().and_then(|value| wire::validate("Request", &value).ok()).is_none() {
            return Response::failure(id, WireErrorCode::InvalidRequest);
        }
        // Waiting for events owns neither the mutation lock nor the server's
        // mutation permit. Revalidate a delegated grant after the wait as well.
        if request.method == Method::EventsSubscribe {
            return match self.subscribe(session, request.params) { Ok(value) => Response::success(id, value), Err(code) => Response::failure(id, code) };
        }
        let Ok(_serial) = self.requests.lock() else { return Response::failure(id, WireErrorCode::Unavailable); };
        match self.route(session, request) { Ok(value) => Response::success(id, value), Err(code) => Response::failure(id, code) }
    }
    fn subscribe(&self, session: &Session, parameters: Value) -> Result<Value, WireErrorCode> {
        if !self.grants.still_valid(session) { return Err(WireErrorCode::PermissionDenied); }
        if self.is_stopping() { return Err(WireErrorCode::Busy); }
        self.synchronize_privacy()?;
        let request: SubscribeParams = params(parameters)?;
        let mut events = self.events.after(request.after_sequence, Duration::from_millis(u64::from(request.wait_ms.unwrap_or(0))), self.snapshot()?).map_err(unavailable)?;
        if !self.grants.still_valid(session) { return Err(WireErrorCode::PermissionDenied); }
        if self.is_stopping() { return Err(WireErrorCode::Busy); }
        self.synchronize_privacy()?;
        let latest = self.snapshot()?;
        events.state.history_revision = latest.history_revision;
        events.state.privacy_epoch = latest.privacy_epoch;
        result("EventsResult", events)
    }
    fn route(&self, session: &Session, request: Request) -> Result<Value, WireErrorCode> {
        if !self.grants.still_valid(session) { return Err(WireErrorCode::PermissionDenied); }
        self.synchronize_privacy()?;
        if self.is_stopping() && !matches!(request.method, Method::ServiceStatus | Method::Health | Method::ServiceStop | Method::ServiceHandoff) { return Err(WireErrorCode::Busy); }
        let now = now_ms()?;
        match request.method {
            Method::Hello => Err(WireErrorCode::InvalidRequest),
            Method::Capabilities | Method::SurfaceCapabilities => result("CapabilityReport", self.capabilities(session)),
            Method::Health => result("RuntimeState", self.snapshot()?),
            Method::ServiceStatus => result("ServiceStatus", ServiceStatus { mode: self.mode,
                capture_owner: self.capture_active(), state: self.snapshot()?, privacy: self.privacy_state()?,capture_unavailable_reason:self.capture_diagnostic.lock().map_err(unavailable)?.clone() }),
            Method::PrivacyGet => result("PrivacyState", self.privacy_state()?),
            Method::ServiceGrant => {
                session.require(Permission::ServiceManage)?;
                let request: GrantIssueParams = params(request.params)?;
                if !matches!(request.role, ClientRole::Surface | ClientRole::Mcp) { return Err(WireErrorCode::PermissionDenied); }
                let permissions = request.permissions.into_iter().map(|permission| match permission.as_str() {
                    "history_read" => Ok(Permission::HistoryRead),
                    "history_body" => Ok(Permission::HistoryBody),
                    "clipboard_copy" => Ok(Permission::ClipboardCopy),
                    "clipboard_paste" => Ok(Permission::ClipboardPaste),
                    "surface_control" => Ok(Permission::SurfaceControl),
                    _ => Err(WireErrorCode::PermissionDenied),
                }).collect::<Result<Vec<_>,_>>()?;
                let grant = self.grants.issue(request.role, permissions, Duration::from_millis(request.lifetime_ms))?;
                result("GrantResult", GrantResult { grant: grant.token().into(), expires_at_ms: now.checked_add(request.lifetime_ms as i64).ok_or(WireErrorCode::InvalidRequest)? })
            }
            Method::ServiceRevoke => {
                session.require(Permission::ServiceManage)?;
                let request: GrantRevokeParams = params(request.params)?;
                self.grants.revoke_delegated(&request.grant)?;
                self.events.interrupt();
                result("OperationResult", OperationResult { performed: true })
            }
            Method::PrivacySet => {
                session.require(Permission::PrivacyWrite)?;
                let request: PrivacySetParams = params(request.params)?;
                if !request.active && request.duration_ms.is_some() { return Err(WireErrorCode::InvalidRequest); }
                self.history.privacy().set_session(if request.active { PrivacySessionCommand::On { duration_ms: request.duration_ms } } else { PrivacySessionCommand::Off }, now).map_err(privacy_error)?;
                self.synchronize_privacy()?;
                result("PrivacyState", self.privacy_state()?)
            }
            Method::PrivacyToggle => {
                session.require(Permission::PrivacyWrite)?;
                self.history.privacy().toggle(now).map_err(privacy_error)?;
                self.synchronize_privacy()?;
                result("PrivacyState", self.privacy_state()?)
            }
            Method::PrivacyHotkeyGet => {
                session.require(Permission::PrivacyWrite)?;
                result("PrivacyHotkey", PrivacyHotkey { hotkey: self.history.privacy().hotkey().map_err(privacy_error)? })
            }
            Method::PrivacyHotkeySet => {
                session.require(Permission::PrivacyWrite)?;
                let request: PrivacyHotkey = params(request.params)?;
                let hotkey = self.hotkey.lock().map_err(unavailable)?.as_mut().ok_or(WireErrorCode::Unavailable)?.set(&request.hotkey)?;
                result("PrivacyHotkey", PrivacyHotkey { hotkey })
            }
            Method::DesktopHistoryPage | Method::DesktopHistoryGet | Method::DesktopSnapshotRead => self.desktop(session, request.method, request.params),
            Method::DesktopHistorySummary => self.desktop_summary(session,request.params),
            Method::DesktopClipboardCopy => self.desktop_copy(session,request.params),
            Method::DesktopHistoryMutate => self.desktop_mutate(session,request.params),
            Method::DesktopPrivacyRules|Method::DesktopPrivacyDecision|Method::DesktopPrivacyFeedback|Method::DesktopPrivacyRemoveRule=>self.desktop_policy(session,request.method,request.params),
            Method::DesktopCaptureBegin | Method::DesktopCaptureWrite | Method::DesktopCaptureFinish => self.upload(session, request.method, request.params),
            Method::HistoryList | Method::SurfaceHistoryList => {
                session.require(Permission::HistoryRead)?;
                let query: HistoryPageParams = params(request.params)?;
                let permit = self.history.privacy().authorize_read(PrivacyAccessIntent::RestrictedSummary, SensitiveAccess::Denied, None, now).map_err(privacy_error)?;
                let entries = self.history_page(summary_query(query), None, &permit, now)?;
                result("HistoryResult", HistoryResult { entries: entries.into_iter().map(summary).collect() })
            }
            Method::HistorySearch | Method::SurfaceHistorySearch => {
                session.require(Permission::HistoryRead)?;
                let request: HistorySearchParams = params(request.params)?;
                let permit = self.history.privacy().authorize_read(PrivacyAccessIntent::RestrictedSummary, SensitiveAccess::Denied, None, now).map_err(privacy_error)?;
                let query = api::SearchRequest { query: request.query, page: summary_query(request.page), mode: match request.mode {
                    SearchMode::Contains => api::SummarySearchMode::Contains, SearchMode::Fuzzy => api::SummarySearchMode::Fuzzy,
                    SearchMode::Regex => api::SummarySearchMode::Regex, SearchMode::Fts5 => api::SummarySearchMode::Fts5 } };
                let entries = self.history_page(query.page.clone(), Some(query), &permit, now)?;
                result("HistoryResult", HistoryResult { entries: entries.into_iter().map(summary).collect() })
            }
            Method::HistoryGet => {
                session.require(Permission::HistoryBody)?;
                let request: EntryIdParams = params(request.params)?;
                if request.id<0 && request.service_instance_id.as_deref()!=Some(self.instance.as_str()) {return Err(WireErrorCode::PrivacyBlocked);}
                let permit = self.history.privacy().authorize_read(PrivacyAccessIntent::AutomationRead, SensitiveAccess::Denied, None, now).map_err(privacy_error)?;
                let entry = self.entry_authorized(request.id, &permit, now)?;
                // Bodies are bounded ordinary text only; attachments and HTML stay
                // behind the explicit service-local clipboard copy operation.
                let text = if matches!(entry.content_type.as_str(), "text" | "code" | "url") && entry.html_content.is_none() && !entry.is_external {
                    Some(entry.content.chars().take(65_536).collect::<String>())
                } else { None };
                let truncated = text.as_ref().is_some_and(|text| text.len() < entry.content.len());
                let summary = EntrySummary { id: entry.id, kind: entry.content_type, preview: None,
                    tags: entry.tags, pinned: entry.is_pinned, timestamp: entry.timestamp, source_app: String::new(), privacy_visibility: "ordinary".into() };
                result("EntryResult", EntryResult { entry: summary, text, truncated })
            }
            Method::ClipboardCopy | Method::SurfaceCopy | Method::SurfacePaste => {
                session.require(Permission::ClipboardCopy)?;
                if !self.actions.can_copy() { return Err(WireErrorCode::Unavailable); }
                if request.method == Method::SurfacePaste { session.require(Permission::ClipboardPaste)?; if !self.actions.can_paste() { return Err(WireErrorCode::Unavailable); } }
                let request_body: ClipboardCopyParams = params(request.params)?;
                if request_body.id<0 && request_body.service_instance_id.as_deref()!=Some(self.instance.as_str()) {return Err(WireErrorCode::PrivacyBlocked);}
                let permit = self.history.privacy().authorize_selected_copy(request_body.id, request_body.service_instance_id.as_deref(), SensitiveAccess::Denied, None, now).map_err(privacy_error)?;
                let payload = if request_body.id < 0 {
                    let entries = self.session.lock().map_err(unavailable)?;
                    let entry = entries.iter().find(|entry| entry.id == request_body.id).ok_or(WireErrorCode::NotFound)?;
                    self.history.session_clipboard_payload_for_host_authorized(entry, &self.instance, &permit, now).map_err(privacy_error)?
                } else { self.history.clipboard_payload_authorized(request_body.id, &permit, now).map_err(privacy_error)? };
                self.history.privacy().check_read(&permit, now_ms()?).map_err(privacy_error)?;
                self.copy_payload(payload)?;
                if request.method == Method::SurfacePaste { self.history.privacy().check_read(&permit, now_ms()?).map_err(privacy_error)?; self.actions.paste()?; }
                result("OperationResult", OperationResult { performed: true })
            }
            Method::SurfaceActivate | Method::SurfaceDismiss => {
                session.require(Permission::SurfaceControl)?;
                if !self.actions.can_activate() { return Err(WireErrorCode::Unavailable); }
                let action: SurfaceActionParams = params(request.params)?;
                if action.entry_id.is_some_and(|id|id<0) && action.service_instance_id.as_deref()!=Some(self.instance.as_str()) {return Err(WireErrorCode::PrivacyBlocked);}
                if request.method == Method::SurfaceActivate { self.actions.activate(action.entry_id)?; } else { self.actions.dismiss()?; }
                result("OperationResult", OperationResult { performed: true })
            }
            Method::EventsSubscribe => Err(WireErrorCode::InvalidRequest),
            Method::ServiceStop | Method::ServiceHandoff => {
                session.require(Permission::ServiceManage)?;
                if request.method == Method::ServiceHandoff { dzc_platform::ipc::ProcessExit::ensure_supported().map_err(unavailable)?; }
                self.stop()?;
                result("OperationResult", OperationResult { performed: true })
            }
        }
    }
    fn entry_authorized(&self, id: i64, permit: &api::PrivacyReadPermit, now: i64) -> Result<ClipboardEntry, WireErrorCode> {
        if id < 0 {
            let session = self.session.lock().map_err(unavailable)?;
            let entry = session.iter().find(|entry| entry.id == id).ok_or(WireErrorCode::NotFound)?;
            self.history.authorize_session_entry(entry, permit, now).map_err(privacy_error)
        } else { self.history.get_entry_authorized(id, permit, now).map_err(privacy_error)?.ok_or(WireErrorCode::NotFound) }
    }
    fn history_page(&self, page: api::SummaryQuery, search: Option<api::SearchRequest>, permit: &api::PrivacyReadPermit, now: i64) -> Result<Vec<api::EntrySummary>, WireErrorCode> {
        let session: Vec<_> = self.session.lock().map_err(unavailable)?.iter().cloned().collect();
        let all_session = api::SummaryQuery { limit: 1000, offset: 0, ..page.clone() };
        let mut entries = if let Some(query) = &search {
            self.history.search_summaries_for_session_authorized(&session, &api::SearchRequest { page: all_session, ..query.clone() }, permit, now)
        } else { self.history.summaries_for_session_authorized(&session, &all_session, permit, now) }.map_err(privacy_error)?.entries;
        let start = page.offset.saturating_sub(entries.len() as u32);
        let persistent_page = api::SummaryQuery { limit: page.limit.saturating_add(entries.len() as u16).min(1000), offset: start, ..page.clone() };
        let persistent = if let Some(query) = search {
            self.history.search_summaries_authorized(&api::SearchRequest { page: persistent_page, ..query }, permit, now)
        } else { self.history.summaries_authorized(&persistent_page, permit, now) }.map_err(privacy_error)?.entries;
        entries.extend(persistent);
        // Core orders pinned entries by a private pinned_order field. Preserve
        // that order instead of reconstructing it from the external summary.
        entries.sort_by(|a,b| if a.pinned && b.pinned { std::cmp::Ordering::Equal } else {
            b.pinned.cmp(&a.pinned).then_with(|| b.timestamp.cmp(&a.timestamp)).then_with(|| if a.id < 0 && b.id < 0 { a.id.cmp(&b.id) } else { b.id.cmp(&a.id) })
        });
        Ok(entries.into_iter().skip((page.offset-start) as usize).take(page.limit as usize).collect())
    }
}
fn summary_query(query: HistoryPageParams) -> api::SummaryQuery {
    api::SummaryQuery { limit: query.limit, offset: query.offset, kind: query.kind, tag: query.tag, pinned: query.pinned.unwrap_or(false) }
}
fn summary(entry: api::EntrySummary) -> EntrySummary {
    EntrySummary { id: entry.id, kind: entry.kind, preview: entry.preview, tags: entry.tags, pinned: entry.pinned,
        timestamp: entry.timestamp, source_app: entry.source_app, privacy_visibility: "ordinary".into() }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::path::Path;

    #[derive(Default)]
    pub struct Actions { copied: Mutex<Vec<String>> }
    impl HostActions for Actions {
        fn platform_capabilities(&self) -> Value { serde_json::json!({"synthetic":true}) }
        fn can_copy(&self) -> bool { true }
        fn copy(&self, payload: dzc_core::ports::ClipboardPayload) -> Result<(), WireErrorCode> {
            if let dzc_core::ports::ClipboardPayload::Text(text) = payload { self.copied.lock().unwrap().push(text); Ok(()) } else { Err(WireErrorCode::Unavailable) }
        }
        fn can_paste(&self) -> bool { false }
        fn paste(&self) -> Result<(), WireErrorCode> { Err(WireErrorCode::Unavailable) }
        fn stop_helpers(&self) -> Result<(), WireErrorCode> { Ok(()) }
    }
    struct SyntheticKeys;
    fn sealed_fixture_values() -> &'static Mutex<std::collections::HashMap<String, String>> {
        static VALUES: std::sync::OnceLock<Mutex<std::collections::HashMap<String, String>>> = std::sync::OnceLock::new();
        VALUES.get_or_init(Mutex::default)
    }
    impl dzc_core::crypto::KeyProvider for SyntheticKeys {
        fn seal_text(&self, plain: &str) -> Result<String, dzc_core::crypto::EncryptionError> {
            // A deterministic process-local fixture uses the actual Core cipher;
            // this registry is not a platform key provider or a crypto claim.
            let sealed = dzc_core::crypto::encrypt_with_key(plain, || Ok([37;32]), |nonce| { nonce.fill(19); Ok(()) })?;
            sealed_fixture_values().lock().unwrap().insert(sealed.clone(), plain.into());
            Ok(sealed)
        }
        fn open_text(&self, cipher: &str) -> Option<String> {
            sealed_fixture_values().lock().unwrap().get(cipher).cloned()
        }
    }
    pub fn synthetic_host() -> Arc<Host> {
        static KEYS: SyntheticKeys = SyntheticKeys;
        let _ = dzc_core::crypto::install_key_provider(&KEYS);
        Host::new(HostOptions { mode: HostMode::Service, history: HistoryService::open(Path::new(":memory:")).unwrap(),
            directory: std::env::temp_dir().join(format!("dzc-host-{}", uuid::Uuid::new_v4())), actions: Arc::new(Actions::default()) }).unwrap()
    }
    fn hello(role: ClientRole) -> HelloParams {
        HelloParams { protocol_version: wire::VERSION, client_version: "fixture".into(), client_role: role, capabilities: vec![], required_capabilities: vec![], grant: None }
    }
    fn dispatch(host: &Host, session: &Session, method: Method, params: Value) -> Response {
        host.dispatch(session, Request { request_id: "synthetic-request".into(), method, params })
    }
    fn insert(host: &Host, text: &str, tags: &[&str]) -> i64 {
        let entry: ClipboardEntry = serde_json::from_value(serde_json::json!({"id":0,"content_type":"text","content":text,
            "preview":text,"source_app":"fixture","timestamp":1,"is_pinned":false,"tags":tags})).unwrap();
        host.history.save(&entry, None).unwrap()
    }
    #[test]
    fn core_visibility_applies_to_summaries_and_service_local_copy() {
        let host = synthetic_host();
        let ordinary = insert(&host, "ordinary synthetic entry", &[]);
        let protected = insert(&host, "private synthetic payload", &["sensitive"]);
        let mut hello = hello(ClientRole::Surface);
        let grant = host.grants.issue(ClientRole::Surface, vec![Permission::ClipboardCopy], Duration::from_secs(30)).unwrap();
        hello.grant = Some(grant.token().into());
        let (session, _) = host.hello(hello).unwrap();
        let response = dispatch(&host, &session, Method::SurfaceHistoryList, serde_json::json!({"limit":20,"offset":0}));
        let result: HistoryResult = serde_json::from_value(response.result.unwrap()).unwrap();
        assert_eq!(result.entries.iter().map(|entry| entry.id).collect::<Vec<_>>(), vec![ordinary]);
        assert!(!serde_json::to_string(&result).unwrap().contains("private synthetic"));
        let response = dispatch(&host, &session, Method::SurfaceCopy, serde_json::json!({"id":protected}));
        assert_eq!(response.error.unwrap().code, WireErrorCode::SensitiveAccessDenied);
        let response = dispatch(&host, &session, Method::SurfaceCopy, serde_json::json!({"id":ordinary}));
        assert_eq!(response.result.unwrap()["performed"], true);
        let response = dispatch(&host, &session, Method::HistoryGet, serde_json::json!({"id":ordinary}));
        assert_eq!(response.error.unwrap().code, WireErrorCode::PermissionDenied);
    }
    #[test]
    fn authenticated_role_labels_do_not_grant_management_or_body_access() {
        let host = synthetic_host();
        for role in [ClientRole::Desktop, ClientRole::Cli, ClientRole::Mcp, ClientRole::Surface] {
            let (session, _) = host.hello(hello(role)).unwrap();
            let response = dispatch(&host, &session, Method::PrivacySet, serde_json::json!({"active":true}));
            assert_eq!(response.error.unwrap().code, WireErrorCode::PermissionDenied);
            assert!(!host.privacy_state().unwrap().active);
        }
        let mut request = hello(ClientRole::Cli);
        request.required_capabilities.push("sql.execute".into());
        assert!(matches!(host.hello(request), Err(WireErrorCode::UnsupportedCapability)));
        let mut request = hello(ClientRole::Cli); request.protocol_version.major = 2;
        assert!(matches!(host.hello(request), Err(WireErrorCode::ProtocolMismatch)));
    }
    #[test]
    fn failed_stop_is_not_acknowledged_and_can_be_retried() {
        struct Lifecycle(AtomicBool);
        impl HostLifecycle for Lifecycle { fn stop_capture(&self) -> Result<(), WireErrorCode> {
            if self.0.load(Ordering::Acquire) { Ok(()) } else { Err(WireErrorCode::Busy) }
        } }
        let host = synthetic_host();
        let lifecycle = Arc::new(Lifecycle(AtomicBool::new(false)));
        host.attach_lifecycle(lifecycle.clone()).unwrap();
        let grant = host.grants.issue_control(ClientRole::Cli).unwrap();
        let mut request = hello(ClientRole::Cli); request.grant = Some(grant.token().into());
        let (session, _) = host.hello(request).unwrap();
        assert_eq!(dispatch(&host, &session, Method::ServiceStop, serde_json::json!({})).error.unwrap().code, WireErrorCode::Busy);
        assert!(!host.capture_stopped());
        lifecycle.0.store(true, Ordering::Release);
        assert_eq!(dispatch(&host, &session, Method::ServiceStop, serde_json::json!({})).result.unwrap()["performed"], true);
        assert!(host.capture_stopped());
    }
    #[test]
    fn explicit_copy_records_the_shared_native_echo_before_write_and_clears_failure() {
        struct EchoActions {host:Mutex<std::sync::Weak<Host>>,fail:AtomicBool}
        impl HostActions for EchoActions {
            fn platform_capabilities(&self)->Value {serde_json::json!({})}
            fn can_copy(&self)->bool {true}
            fn copy(&self,payload:dzc_core::ports::ClipboardPayload)->Result<(),WireErrorCode> {
                let host=self.host.lock().unwrap().upgrade().unwrap();
                let expected=dzc_platform::capture::clipboard_echo(&payload).unwrap().unwrap();
                assert_eq!(host.own_copy(now_ms().unwrap()).primary,expected.primary,"the notification may arrive during the native write");
                if self.fail.load(Ordering::SeqCst) {Err(WireErrorCode::Unavailable)}else{Ok(())}
            }
            fn can_paste(&self)->bool {false}
            fn paste(&self)->Result<(),WireErrorCode> {Err(WireErrorCode::Unavailable)}
            fn stop_helpers(&self)->Result<(),WireErrorCode> {Ok(())}
        }
        let original=synthetic_host();
        let actions=Arc::new(EchoActions{host:Mutex::new(std::sync::Weak::new()),fail:AtomicBool::new(true)});
        let host=Host::new(HostOptions{mode:HostMode::Service,history:original.history.clone(),directory:original.directory.clone(),actions:actions.clone()}).unwrap();
        *actions.host.lock().unwrap()=Arc::downgrade(&host);
        let payload=||dzc_core::ports::ClipboardPayload::Text("synthetic selected copy".into());
        assert_eq!(host.copy_payload(payload()),Err(WireErrorCode::Unavailable));
        assert_eq!(host.own_copy(now_ms().unwrap()).primary,0);
        actions.fail.store(false,Ordering::SeqCst);host.copy_payload(payload()).unwrap();
        assert_ne!(host.own_copy(now_ms().unwrap()).primary,0);
        host.clear_copy_echo();assert_eq!(host.own_copy(now_ms().unwrap()).primary,0);
    }
}
