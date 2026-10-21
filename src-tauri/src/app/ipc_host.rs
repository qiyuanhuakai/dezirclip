//! The default desktop uses the shared Host in process. Only an explicitly
//! started service changes history hydration into local IPC.
use crate::{app_state::{AppDataDir, SessionHistory}, database::DbState};
use dzc_core::{api::{ClipboardEntry, HistoryService}, ports::{ClipboardWriter, PasteController}};
use dzc_host::{wire::*, CaptureManager, Client, ControlFile, Host, HostActions, HostOptions};
use serde::{Serialize, de::DeserializeOwned};
use std::{path::PathBuf, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}, time::Duration};
use tauri::{AppHandle, Emitter, Manager};

enum Mode { Starting, Embedded(Arc<Host>), Remote(String), Unavailable }
#[derive(Clone,Copy)]
enum RestoreMode {Embedded,Service{no_capture:bool}}
struct Inner { mode:Mutex<Mode>, history:HistoryService, directory:PathBuf, database:PathBuf,
    session:Arc<Mutex<std::collections::VecDeque<ClipboardEntry>>>, closing:AtomicBool,writer:Mutex<Option<dzc_platform::runtime::EmbeddedClipboardWriter>>,
    server:Mutex<Option<ServerTask>>,restore:Mutex<Option<RestoreMode>> }
struct ServerTask {stop:tokio::sync::watch::Sender<bool>,done:std::sync::mpsc::Receiver<()>}
#[derive(Clone)]
pub struct DesktopHost(Arc<Inner>);
pub struct GeneratedUpload {client:Client,id:String}
impl GeneratedUpload {
    pub async fn finish(mut self,body:GeneratedBody,source:String,protected:bool)->Result<bool,String> {
        use base64::Engine;
        let bytes=serde_json::to_vec(&body).map_err(|_|failed())?;
        if bytes.len()>32*1024*1024 {return Err("GeneratedBodyTooLarge".into());}
        for (index,bytes) in bytes.chunks(192*1024).enumerate() {
            let _:OperationResult=self.client.call(Method::DesktopCaptureWrite,CaptureWriteParams {capture_id:self.id.clone(),offset:(index*192*1024) as u32,
                data_base64:base64::engine::general_purpose::STANDARD.encode(bytes)},"OperationResult").await.map_err(remote_error)?;
        }
        let result:OperationResult=self.client.call(Method::DesktopCaptureFinish,CaptureFinishParams{capture_id:self.id,source_app:source.chars().take(256).collect(),inherited_protection:protected},"OperationResult").await.map_err(remote_error)?;
        Ok(result.performed)
    }
}

#[derive(Serialize)]
pub struct HostedEntry {
    #[serde(flatten)] pub entry:ClipboardEntry,
    #[serde(skip_serializing_if="Option::is_none")] pub host_instance_id:Option<String>,
}
impl HostedEntry { pub fn local(entry:ClipboardEntry)->Self {Self{entry,host_instance_id:None}} }
impl std::ops::Deref for HostedEntry {type Target=ClipboardEntry;fn deref(&self)->&ClipboardEntry {&self.entry}}

fn app()->Option<&'static AppHandle> {crate::global_state::GLOBAL_APP_HANDLE.get()}
fn emit(name:&str,value:impl Serialize+Clone) {if let Some(app)=app() {let _=app.emit(name,value);}}
fn failed()->String {"ClipboardHostUnavailable".into()}
fn public_code(code:WireErrorCode)->String {match code {
    WireErrorCode::PrivacyStateUnavailable=>"PrivacyStateUnavailable",WireErrorCode::PrivacyEpochChanged=>"PrivacyEpochChanged",
    WireErrorCode::PrivacySessionActive=>"PrivacySessionActive",WireErrorCode::SensitiveAccessDenied=>"SensitiveAccessDenied",
    WireErrorCode::ManualPersistenceConfirmationRequired=>"ManualPersistenceConfirmationRequired",WireErrorCode::PrivacyDurationInvalid=>"PrivacyDurationInvalid",
    WireErrorCode::NotFound=>"ClipboardRecordUnavailable",WireErrorCode::Busy=>"ClipboardHostBusy",WireErrorCode::PrivacyBlocked=>"SensitiveAccessDenied",
    WireErrorCode::PermissionDenied=>"ClipboardHostPermissionDenied",_=>"ClipboardHostUnavailable",
}.into()}
fn remote_error(error:impl ToString)->String {
    let message=error.to_string();
    serde_json::from_str::<WireErrorCode>(&message).map(public_code).unwrap_or_else(|_|match message.as_str() {
        "PrivacyStateUnavailable"|"PrivacyEpochChanged"|"PrivacySessionActive"|"SensitiveAccessDenied"|"ManualPersistenceConfirmationRequired"|"PrivacyDurationInvalid"=>message,
        _=>failed(),
    })
}
fn now()->i64 {chrono::Utc::now().timestamp_millis()}
struct DesktopActions {writer:dzc_platform::runtime::EmbeddedClipboardWriter}
impl HostActions for DesktopActions {
    fn platform_capabilities(&self)->serde_json::Value {serde_json::to_value(dzc_platform::runtime::standalone_capabilities()).unwrap_or_default()}
    fn can_copy(&self)->bool {self.writer.capability().is_available()}
    fn copy(&self,payload:dzc_core::ports::ClipboardPayload)->Result<(),WireErrorCode> {self.writer.write_suppressing_recapture(payload).map_err(|_|WireErrorCode::Unavailable)}
    fn can_paste(&self)->bool {dzc_platform::runtime::NativePasteController.capability().is_available()}
    fn paste(&self)->Result<(),WireErrorCode> {dzc_platform::runtime::NativePasteController.paste().map_err(|_|WireErrorCode::Unavailable)}
    fn can_activate(&self)->bool {app().is_some()}
    fn activate(&self,_id:Option<i64>)->Result<(),WireErrorCode> {crate::app::window_manager::focus_clipboard_window(app().ok_or(WireErrorCode::Unavailable)?.clone()).map_err(|_|WireErrorCode::Unavailable)}
    fn dismiss(&self)->Result<(),WireErrorCode> {crate::app::window_manager::hide_window_cmd(app().ok_or(WireErrorCode::Unavailable)?.clone()).map_err(|_|WireErrorCode::Unavailable)}
    fn stop_helpers(&self)->Result<(),WireErrorCode> {self.writer.stop().map_err(|_|WireErrorCode::Busy)}
    fn privacy_changed(&self,snapshot:&dzc_core::api::RuntimeSnapshot) {emit("privacy-state-changed",snapshot.clone());}
}

impl DesktopHost {
    pub fn start(app:&AppHandle) {
        let directory=app.state::<AppDataDir>().0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        let bridge=Self(Arc::new(Inner {mode:Mutex::new(Mode::Starting),history:app.state::<DbState>().history_service(),database:directory.join("clipboard.db"),directory,
            session:app.state::<SessionHistory>().0.clone(),closing:AtomicBool::new(false),writer:Mutex::new(None),server:Mutex::new(None),restore:Mutex::new(None)}));
        app.manage(bridge.clone());
        tauri::async_runtime::spawn(async move {if bridge.initialize().await.is_err() {bridge.set_mode(Mode::Unavailable);emit("clipboard-host-unavailable",());}});
    }
    fn set_mode(&self,mode:Mode) {if let Ok(mut current)=self.0.mode.lock() {*current=mode;}}
    fn invalidate_views(&self) {
        if let Ok(snapshot)=self.0.history.runtime_snapshot(now()) {
            if let Ok(mut value)=serde_json::to_value(snapshot) {
                if let Some(value)=value.as_object_mut() {value.insert("host_instance_id".into(),self.instance().ok().map(serde_json::Value::String).unwrap_or(serde_json::Value::Null));}
                emit("privacy-state-changed",value);
            }
        }
        emit("clipboard-host-changed",self.instance().ok());
        emit("clipboard-changed",());
    }
    pub fn embedded(&self)->Result<Option<Arc<Host>>,String> {match &*self.0.mode.lock().map_err(|_|failed())? {
        Mode::Embedded(host) if !host.is_stopping()=>Ok(Some(host.clone())),Mode::Remote(_)=>Ok(None),_=>Err(failed())}}
    pub fn instance(&self)->Result<String,String> {match &*self.0.mode.lock().map_err(|_|failed())? {
        Mode::Embedded(host)=>host.snapshot().map(|state|state.service_instance_id).map_err(|_|failed()),Mode::Remote(instance)=>Ok(instance.clone()),_=>Err(failed())}}
    pub fn writer(&self)->Result<dzc_platform::runtime::EmbeddedClipboardWriter,String> {
        if self.embedded()?.is_none() {return Err("ClipboardWriteRequiresHost".into());}
        self.0.writer.lock().map_err(|_|failed())?.clone().ok_or_else(failed)
    }
    pub fn check_instance(&self,id:i64,expected:Option<&str>)->Result<(),String> {
        if id<0 && expected!=Some(self.instance()?.as_str()) {return Err("ClipboardHostInstanceChanged".into());} Ok(())
    }
    pub fn hosted(&self,entry:ClipboardEntry)->Result<HostedEntry,String> {Ok(HostedEntry{entry,host_instance_id:Some(self.instance()?)})}
    pub fn snapshot_after(&self,privacy:PrivacyState)->Result<dzc_core::api::RuntimeSnapshot,String> {
        let snapshot=self.0.history.runtime_snapshot(now()).map_err(remote_error)?;
        if snapshot.privacy_epoch!=privacy.privacy_epoch {return Err("PrivacyEpochChanged".into());}
        Ok(snapshot)
    }
    async fn service_client(&self)->Result<Client,String> {
        let endpoint=dzc_host::controller::endpoint(&self.0.database).map_err(|_|failed())?;
        let mut client=Client::control(&endpoint,ClientRole::Desktop).await.map_err(|_|failed())?;
        let status:ServiceStatus=client.call(Method::ServiceStatus,serde_json::json!({}),"ServiceStatus").await.map_err(|_|failed())?;
        if status.mode!=HostMode::Service {return Err(failed());} Ok(client)
    }
    pub async fn call<T:DeserializeOwned>(&self,method:Method,params:impl Serialize,schema:&str)->Result<T,String> {
        if let Some(host)=self.embedded()? {
            let grant=host.grants().issue_control(ClientRole::Desktop).map_err(|_|failed())?;
            let accepted=host.hello(HelloParams{protocol_version:dzc_host::wire::VERSION,client_version:env!("CARGO_PKG_VERSION").into(),client_role:ClientRole::Desktop,
                capabilities:vec![],required_capabilities:vec![],grant:Some(grant.token().into())});
            let (session,_)=match accepted {Ok(value)=>value,Err(_)=>{host.grants().revoke(&grant);return Err(failed());}};
            let response=host.dispatch(&session,Request{request_id:"embedded-desktop".into(),method,params:serde_json::to_value(params).map_err(|_|failed())?});
            host.grants().revoke(&grant);
            if let Some(error)=response.error {return Err(public_code(error.code));}
            return serde_json::from_value(response.result.ok_or_else(failed)?).map_err(|_|failed());
        }
        let mut client=self.service_client().await?;
        if client.hello.state.service_instance_id!=self.instance()? {return Err("ClipboardHostInstanceChanged".into());}
        client.call(method,params,schema).await.map_err(remote_error)
    }
    pub async fn page(&self,limit:i32,offset:i32,kind:Option<String>,query:Option<String>)->Result<Option<Vec<HostedEntry>>,String> {
        if self.embedded()?.is_some() {return Ok(None);}
        let mut client=self.service_client().await?;
        let instance=client.hello.state.service_instance_id.clone();
        if instance!=self.instance()? {return Err("ClipboardHostInstanceChanged".into());}
        let body:BodySnapshot=client.call(Method::DesktopHistoryPage,DesktopPageParams {limit:limit.clamp(1,150) as u16,offset:offset.max(0) as u32,kind,query},"BodySnapshot").await.map_err(remote_error)?;
        let result:DesktopEntries=client.read_snapshot(body,"DesktopEntries").await.map_err(remote_error)?;
        result.entries.into_iter().map(|entry|hosted(entry,&instance)).collect::<Result<Vec<_>,_>>().map(Some)
    }
    pub async fn entry(&self,id:i64,epoch:u64,reveal:bool,thumbnail:Option<u32>,expected:Option<&str>)->Result<Option<HostedEntry>,String> {
        if self.embedded()?.is_some() {return Ok(None);}
        self.check_instance(id,expected)?;
        let mut client=self.service_client().await?;
        let instance=self.instance()?;
        if client.hello.state.service_instance_id!=instance {return Err("ClipboardHostInstanceChanged".into());}
        let body:BodySnapshot=client.call(Method::DesktopHistoryGet,DesktopEntryParams{id,expected_epoch:epoch,reveal,thumbnail_dimension:thumbnail,service_instance_id:instance.clone()},"BodySnapshot").await.map_err(remote_error)?;
        let result:DesktopEntry=client.read_snapshot(body,"DesktopEntry").await.map_err(remote_error)?;
        hosted(result,&instance).map(Some)
    }
    pub async fn summary(&self,id:i64,expected:Option<&str>)->Result<Option<HostedEntry>,String> {
        if self.embedded()?.is_some() {return Ok(None);}
        self.check_instance(id,expected)?;
        let mut client=self.service_client().await?;
        let state=client.hello.state.clone();
        if state.service_instance_id!=self.instance()? {return Err("ClipboardHostInstanceChanged".into());}
        let body:BodySnapshot=client.call(Method::DesktopHistorySummary,DesktopEntryParams{id,service_instance_id:state.service_instance_id.clone(),expected_epoch:state.privacy_epoch,reveal:false,thumbnail_dimension:None},"BodySnapshot").await.map_err(remote_error)?;
        let result:DesktopEntry=client.read_snapshot(body,"DesktopEntry").await.map_err(remote_error)?;
        hosted(result,&state.service_instance_id).map(Some)
    }
    pub async fn begin_generated(&self)->Result<Option<GeneratedUpload>,String> {
        let mut client=self.service_client().await?;
        if client.hello.state.service_instance_id!=self.instance()? {return Err("ClipboardHostInstanceChanged".into());}
        let state=client.hello.state.clone();
        let result:CaptureBeginResult=client.call(Method::DesktopCaptureBegin,CaptureBeginParams{service_instance_id:state.service_instance_id,expected_epoch:state.privacy_epoch},"CaptureBeginResult").await.map_err(remote_error)?;
        Ok(result.capture_id.map(|id|GeneratedUpload{client,id}))
    }
    pub async fn mutation(&self,mut request:DesktopMutationParams,expected:Option<&str>)->Result<Option<i64>,String> {
        self.check_instance(request.id,expected)?;
        if let Some(host)=self.embedded()? {return host.mutate_owned(request).map(|result|Some(result.id)).map_err(public_code);}
        request.service_instance_id=self.instance()?;
        let result:DesktopMutationResult=self.call(Method::DesktopHistoryMutate,request,"DesktopMutationResult").await?;
        Ok(Some(result.id))
    }
    pub fn mutation_request(&self,id:i64,action:&str,epoch:Option<u64>)->Result<DesktopMutationParams,String> {
        Ok(DesktopMutationParams{id,action:action.into(),service_instance_id:self.instance()?,expected_epoch:epoch.unwrap_or(self.0.history.runtime_snapshot(now()).map_err(remote_error)?.privacy_epoch),
            reveal:false,pinned:None,tags:None,content:None,confirm_persistence:None})
    }
    pub async fn feedback(&self,id:i64,action:dzc_core::api::PrivacyFeedbackAction,epoch:u64,expected:Option<&str>)->Result<dzc_core::api::RuntimeSnapshot,String> {
        self.check_instance(id,expected)?;
        if let Some(host)=self.embedded()? {return host.policy_feedback_owned(id,action,epoch).map_err(public_code);}
        let action=serde_json::to_value(action).map_err(|_|failed())?.as_str().ok_or_else(failed)?.to_owned();
        let result:DesktopRuntimeValue=self.call(Method::DesktopPrivacyFeedback,DesktopPolicyParams{service_instance_id:self.instance()?,expected_epoch:epoch,id:Some(id),action:Some(action),rule_id:None},"DesktopRuntimeValue").await?;
        self.snapshot_after(result.privacy)
    }
    pub async fn remove_rule(&self,id:String,epoch:u64)->Result<dzc_core::api::RuntimeSnapshot,String> {
        if let Some(host)=self.embedded()? {return host.policy_remove_owned(&id,epoch).map_err(public_code);}
        let result:DesktopRuntimeValue=self.call(Method::DesktopPrivacyRemoveRule,DesktopPolicyParams{service_instance_id:self.instance()?,expected_epoch:epoch,id:None,action:None,rule_id:Some(id)},"DesktopRuntimeValue").await?;
        self.snapshot_after(result.privacy)
    }
    pub async fn policy_decision(&self,id:i64,epoch:u64,expected:Option<&str>)->Result<dzc_core::api::PrivacyDecisionReport,String> {
        self.check_instance(id,expected)?;
        if let Some(host)=self.embedded()? {return host.policy_decision_owned(id,epoch).map_err(public_code);}
        let result:DesktopPolicyDecision=self.call(Method::DesktopPrivacyDecision,DesktopPolicyParams{service_instance_id:self.instance()?,expected_epoch:epoch,id:Some(id),action:None,rule_id:None},"DesktopPolicyDecision").await?;
        serde_json::from_value(serde_json::to_value(result).map_err(|_|failed())?).map_err(|_|failed())
    }
    pub async fn policy_rules(&self)->Result<Vec<dzc_core::api::PrivacyRuleSummary>,String> {
        let epoch=self.0.history.runtime_snapshot(now()).map_err(remote_error)?.privacy_epoch;
        if let Some(host)=self.embedded()? {return host.policy_rules_owned(epoch).map_err(public_code);}
        let mut client=self.service_client().await?;
        let descriptor:BodySnapshot=client.call(Method::DesktopPrivacyRules,DesktopPolicyParams{service_instance_id:self.instance()?,expected_epoch:epoch,id:None,action:None,rule_id:None},"BodySnapshot").await.map_err(remote_error)?;
        let result:DesktopPolicyRules=client.read_snapshot(descriptor,"DesktopPolicyRules").await.map_err(remote_error)?;
        serde_json::from_value(serde_json::to_value(result.rules).map_err(|_|failed())?).map_err(|_|failed())
    }
    async fn initialize(&self)->Result<(),String> {
        if let Ok(client)=self.service_client().await {self.begin_remote(client);return Ok(());}
        self.begin_embedded()
    }
    fn begin_embedded(&self)->Result<(),String> {
        if self.0.closing.load(Ordering::Acquire) {return Err(failed());}
        let endpoint=dzc_host::controller::endpoint(&self.0.database).map_err(|_|failed())?;
        let listener=endpoint.bind().map_err(|_|failed())?;
        let writer=dzc_platform::runtime::EmbeddedClipboardWriter::default();
        let host=Host::with_session(HostOptions {mode:HostMode::Embedded,history:self.0.history.clone(),directory:self.0.directory.clone(),
            actions:Arc::new(DesktopActions{writer:writer.clone()})},self.0.session.clone()).map_err(|_|failed())?;
        let (capture,ready)=CaptureManager::start_with_hooks(&host,Arc::new(crate::services::clipboard::DesktopHooks)).map_err(|_|failed())?;
        if ready.is_err() {capture.stop().map_err(|_|"CaptureStopUnconfirmed".to_owned())?;return Err("ClipboardCaptureUnavailable".into());}
        if host.initialize_privacy_shortcut().is_err() {host.shutdown().map_err(|_|"CaptureStopUnconfirmed".to_owned())?;return Err("PrivacyShortcutUnavailable".into());}
        let credentials=match ControlFile::publish(&host,&endpoint) {Ok(credentials)=>credentials,Err(_)=>{host.shutdown().map_err(|_|"CaptureStopUnconfirmed".to_owned())?;return Err(failed());}};
        *self.0.writer.lock().map_err(|_|failed())?=Some(writer);
        self.set_mode(Mode::Embedded(host.clone()));
        self.invalidate_views();
        let bridge=self.clone();
        let (sender,receiver)=tokio::sync::watch::channel(false);
        let (done_sender,done)=std::sync::mpsc::sync_channel(1);
        *self.0.server.lock().map_err(|_|failed())?=Some(ServerTask{stop:sender,done});
        tauri::async_runtime::spawn(async move {
            let _=dzc_host::serve_listener(host.clone(),listener,receiver).await;
            drop(credentials);
            let stopped=host.shutdown();
            let _=done_sender.send(());
            if stopped.is_err() {bridge.set_mode(Mode::Unavailable);return;}
            if bridge.0.closing.load(Ordering::Acquire) {return;}
            bridge.set_mode(Mode::Starting);
            let deadline=tokio::time::Instant::now()+Duration::from_secs(15);
            while tokio::time::Instant::now()<deadline {
                if let Ok(client)=bridge.service_client().await {bridge.begin_remote(client);return;}
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            if bridge.begin_embedded().is_err() {bridge.set_mode(Mode::Unavailable);emit("clipboard-host-unavailable",());}
        });
        Ok(())
    }
    fn begin_remote(&self,mut client:Client) {
        let instance=client.hello.state.service_instance_id.clone();
        let mut after=client.hello.state.event_sequence;
        self.set_mode(Mode::Remote(instance.clone()));
        self.invalidate_views();
        let bridge=self.clone();
        tauri::async_runtime::spawn(async move {
            let mut last_revision=client.hello.state.history_revision;
            while !bridge.0.closing.load(Ordering::Acquire) {
                let result:Result<EventsResult,_>=client.call(Method::EventsSubscribe,SubscribeParams{after_sequence:after,wait_ms:Some(1000)},"EventsResult").await;
                let Ok(events)=result else {break;};
                if events.state.service_instance_id!=instance {break;}
                for event in events.events {
                    match event.event {
                        EventKind::EntryAdded|EventKind::EntryUpdated=>{if let Some(id)=event.data.entry_id {emit("clipboard-captured",serde_json::json!({"id":id,"host_instance_id":instance}));}},
                        EventKind::EntryDeleted=>{if let Some(id)=event.data.entry_id {emit("clipboard-removed",id);}},
                        EventKind::PrivacyStateChanged=>{if let Ok(snapshot)=bridge.0.history.runtime_snapshot(now()) {emit("privacy-state-changed",snapshot);}},
                        _=>(),
                    }
                }
                if events.resync_required || events.state.history_revision!=last_revision {emit("clipboard-changed",());}
                last_revision=events.state.history_revision;after=events.state.event_sequence;
            }
            if bridge.0.closing.load(Ordering::Acquire) {return;}
            // A broken transport is not proof that the previous worker stopped.
            // Kernel ownership still prevents a recovery listener from doubling it.
            let process=dzc_platform::ipc::ProcessExit::open(&client.peer);
            drop(client);
            let exited=match process {Ok(process)=>tokio::task::spawn_blocking(move||process.wait(Duration::from_secs(5))).await.is_ok_and(|result|result.is_ok()),Err(_)=>false};
            if exited && bridge.begin_embedded().is_ok() {return;}
            bridge.set_mode(Mode::Unavailable);bridge.invalidate_views();emit("clipboard-host-unavailable",());
        });
    }
    pub fn shutdown(&self)->Result<(),String> {
        self.0.closing.store(true,Ordering::Release);
        let host=match &*self.0.mode.lock().map_err(|_|failed())? {
            Mode::Embedded(host)=>Some(host.clone()),Mode::Starting=>return Err("ClipboardTransitionUnconfirmed".into()),_=>None};
        if let Some(host)=host {host.shutdown().map_err(|_|"ClipboardStopUnconfirmed".to_owned())?;}
        let mut server=self.0.server.lock().map_err(|_|failed())?;
        if let Some(task)=server.as_ref() {
            let _=task.stop.send(true);
            task.done.recv_timeout(Duration::from_secs(5)).map_err(|_|"ClipboardListenerStopUnconfirmed")?;
        }
        *server=None;
        Ok(())
    }
    async fn prepare_update(&self)->Result<(),String> {
        let remote=self.embedded()?.is_none();
        let desired=if remote {
            let mut client=self.service_client().await?;
            if client.hello.state.service_instance_id!=self.instance()? {return Err("ClipboardHostInstanceChanged".into());}
            let status:ServiceStatus=client.call(Method::ServiceStatus,serde_json::json!({}),"ServiceStatus").await.map_err(remote_error)?;
            RestoreMode::Service{no_capture:!status.capture_owner}
        }else {RestoreMode::Embedded};
        *self.0.restore.lock().map_err(|_|failed())?=Some(desired);
        self.0.closing.store(true,Ordering::Release);
        if remote {dzc_host::controller::stop(&self.0.database).await.map_err(remote_error)?;Ok(())}
        else {self.shutdown()}
    }
    async fn restore_after_update(&self)->Result<(),String> {
        self.restore_after_update_with(dzc_host::controller::sibling_executable().map_err(|_|failed())?).await
    }
    async fn restore_after_update_with(&self,executable:PathBuf)->Result<(),String> {
        let desired=self.0.restore.lock().map_err(|_|failed())?.take();
        self.0.closing.store(false,Ordering::Release);
        if let Some(RestoreMode::Service{no_capture})=desired {
            dzc_host::controller::start(&self.0.database,&executable,no_capture).await.map_err(remote_error)?;
            let client=self.service_client().await?;self.begin_remote(client);Ok(())
        }else if desired.is_some() {self.begin_embedded()}else{Ok(())}
    }
}
fn hosted(entry:DesktopEntry,instance:&str)->Result<HostedEntry,String> {
    let entry:ClipboardEntry=serde_json::from_value(serde_json::to_value(entry).map_err(|_|failed())?).map_err(|_|failed())?;
    Ok(HostedEntry{entry,host_instance_id:Some(instance.into())})
}

/// The updater calls this only after signature verification. Coordination runs
/// off the Tauri runtime thread and installation waits for actual owner exit.
pub struct DesktopUpdateHost;
pub async fn notify_privacy_change(app:&AppHandle)->Result<dzc_core::api::RuntimeSnapshot,String> {
    let bridge=app.state::<DesktopHost>();
    let privacy:PrivacyState=bridge.call(Method::PrivacyGet,serde_json::json!({}),"PrivacyState").await?;
    bridge.snapshot_after(privacy)
}
fn coordinate(operation:impl FnOnce(DesktopHost)->Result<(),String>+Send+'static)->Result<(),()> {
    let bridge=app().and_then(|app|app.try_state::<DesktopHost>()).map(|bridge|bridge.inner().clone()).ok_or(())?;
    std::thread::Builder::new().name("dzc-update-coordinator".into()).spawn(move||operation(bridge)).map_err(|_|())?.join().map_err(|_|())?.map_err(|_|())
}
impl crate::domain::update_host::UpdateHost for DesktopUpdateHost {
    fn prepare_install(&self)->Result<(),()> {coordinate(|bridge|tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|_|failed())?.block_on(bridge.prepare_update()))}
    fn install_aborted(&self) {if coordinate(|bridge|tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|_|failed())?.block_on(bridge.restore_after_update())).is_err() {emit("clipboard-host-unavailable",());}}
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let database=self.0.join("clipboard.db");
            let _=std::thread::spawn(move|| {
                if let Ok(runtime)=tokio::runtime::Builder::new_current_thread().enable_all().build() {
                    let _=runtime.block_on(dzc_host::controller::stop(&database));
                }
            }).join();
            let _=std::fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    #[ignore="requires an explicitly built same-source service helper; no native clipboard is initialized"]
    fn verified_update_prepare_and_abort_restore_the_real_no_capture_service() {
        let executable=PathBuf::from(std::env::var_os("DEZIRCLIP_SERVICE_TEST_BIN").expect("explicit synthetic service helper"));
        assert!(executable.is_file());
        let directory=std::env::temp_dir().join(format!("dzc-update-host-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();let _cleanup=Cleanup(directory.clone());
        let database=directory.join("clipboard.db");let history=HistoryService::open(&database).unwrap();
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            let original=dzc_host::controller::start(&database,&executable,true).await.unwrap();
            assert!(!original.capture_owner);
            let bridge=DesktopHost(Arc::new(Inner{mode:Mutex::new(Mode::Remote(original.state.service_instance_id.clone())),history,
                directory,database:database.clone(),session:Arc::new(Mutex::new(Default::default())),closing:AtomicBool::new(false),writer:Mutex::new(None),server:Mutex::new(None),restore:Mutex::new(None)}));
            bridge.prepare_update().await.unwrap();
            assert!(dzc_host::controller::status(&database).await.is_err());
            assert!(matches!(*bridge.0.restore.lock().unwrap(),Some(RestoreMode::Service{no_capture:true})));
            bridge.restore_after_update_with(executable).await.unwrap();
            let restored=dzc_host::controller::status(&database).await.unwrap();
            assert!(!restored.capture_owner);assert_ne!(restored.state.service_instance_id,original.state.service_instance_id);
            bridge.0.closing.store(true,Ordering::Release);
            dzc_host::controller::stop(&database).await.unwrap();
        });
    }
}
