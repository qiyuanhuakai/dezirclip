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
    session:Arc<dzc_core::api::SessionHistoryStore>, closing:AtomicBool,writer:Mutex<Option<dzc_platform::runtime::EmbeddedClipboardWriter>>,
    server:Mutex<Option<ServerTask>>,restore:Mutex<Option<RestoreMode>> }
struct ServerTask {stop:tokio::sync::watch::Sender<bool>,done:std::sync::mpsc::Receiver<()>}
#[derive(Clone)]
pub struct DesktopHost(Arc<Inner>);
pub(crate) struct ServiceQueueCopy {
    client:Client,
    pub instance:String,
    pub epoch:u64,
    id:i64,
    pub content_type:String,
    pub content:Option<String>,
}
fn queue_completion(id:i64,instance:String,epoch:u64,delete:bool)->DesktopMutationParams {
    DesktopMutationParams{id,service_instance_id:instance,expected_epoch:epoch,reveal:false,action:if delete{"delete"}else{"used"}.into(),
        pinned:None,tags:None,content:None,confirm_persistence:None}
}
fn supports_queue_completion(report:&CapabilityReport)->bool {
    let feature=&report.platform["desktop_queue_completion"];
    report.permissions.contains(&Permission::DesktopHistory)
        && feature.get("supported").and_then(serde_json::Value::as_bool)==Some(true)
        && feature.get("version").and_then(serde_json::Value::as_u64)==Some(1)
}
fn check_queue_state(instance:&str,epoch:u64,actual_instance:&str,actual_epoch:u64)->Result<(),String> {
    if actual_instance!=instance {return Err("ClipboardHostInstanceChanged".into());}
    if actual_epoch!=epoch {return Err("PrivacyEpochChanged".into());} Ok(())
}
fn frozen_capture_request(instance:&str,epoch:u64,actual_instance:&str,actual_epoch:u64)->Result<CaptureBeginParams,String> {
    check_queue_state(instance,epoch,actual_instance,actual_epoch)?;
    Ok(CaptureBeginParams{service_instance_id:instance.into(),expected_epoch:epoch,copy_to_clipboard:Some(false)})
}
#[cfg(test)]
mod frozen_capture_tests {
    use super::*;
    #[test]
    fn frozen_remote_request_keeps_original_host_and_epoch_after_the_selection_wait() {
        let request=frozen_capture_request("original-host",7,"original-host",7).unwrap();
        assert_eq!(request.service_instance_id,"original-host");
        assert_eq!(request.expected_epoch,7);
        assert_eq!(request.copy_to_clipboard,Some(false));
        assert_eq!(frozen_capture_request("original-host",7,"replacement-host",7).err().unwrap(),"ClipboardHostInstanceChanged");
        for current in [8,9] {
            assert_eq!(frozen_capture_request("original-host",7,"original-host",current).err().unwrap(),"PrivacyEpochChanged");
        }
    }
}
async fn service_queue_entry(client:&mut Client,id:i64,instance:&str,epoch:u64,method:Method)->Result<DesktopEntry,String> {
    let descriptor:BodySnapshot=client.call(method,DesktopEntryParams{id,service_instance_id:instance.into(),expected_epoch:epoch,reveal:false,thumbnail_dimension:None},"BodySnapshot").await.map_err(remote_error)?;
    if descriptor.state.service_instance_id!=instance || descriptor.state.privacy_epoch!=epoch {return Err("PrivacyEpochChanged".into());}
    let entry:DesktopEntry=client.read_snapshot(descriptor,"DesktopEntry").await.map_err(remote_error)?;
    if entry.id!=id {return Err(failed());}
    Ok(entry)
}
async fn resolve_queue_body(body:Result<DesktopEntry,String>,metadata:impl std::future::Future<Output=Result<DesktopEntry,String>>)->Result<DesktopEntry,String> {
    match body {
        Err(error) if error=="ClipboardRecordUnavailable"=>match metadata.await {
            Ok(_)=>Err("SensitiveAccessDenied".into()),
            Err(error)=>Err(error),
        },
        result=>result,
    }
}
impl ServiceQueueCopy {
    pub async fn copy(&mut self)->Result<(),String> {
        let result:OperationResult=self.client.call(Method::DesktopClipboardCopy,DesktopCopyParams{id:self.id,service_instance_id:self.instance.clone(),expected_epoch:self.epoch,
            reveal:false,paste:false,derived_text:None,image_as_base64:None,with_format:Some(self.content_type=="rich_text"),delete_after_use:Some(false),move_to_top:Some(false)},"OperationResult").await.map_err(remote_error)?;
        if !result.performed {return Err(failed());} Ok(())
    }
    pub async fn check(&mut self,bridge:&DesktopHost)->Result<(),String> {
        let privacy:PrivacyState=self.client.call(Method::PrivacyGet,serde_json::json!({}),"PrivacyState").await.map_err(remote_error)?;
        if bridge.instance()?!=self.instance {return Err("ClipboardHostInstanceChanged".into());}
        if privacy.privacy_epoch!=self.epoch {return Err("PrivacyEpochChanged".into());} Ok(())
    }
    pub async fn complete(&mut self,delete:bool)->Result<(),String> {
        let result:DesktopMutationResult=self.client.call(Method::DesktopHistoryMutate,queue_completion(self.id,self.instance.clone(),self.epoch,delete),"DesktopMutationResult").await.map_err(remote_error)?;
        if result.id!=self.id || !result.performed {return Err(failed());} Ok(())
    }
}
pub struct GeneratedUpload {client:Client,id:String}
impl GeneratedUpload {
    pub async fn finish(self,body:GeneratedBody,source:String,protected:bool)->Result<bool,String> {
        self.finish_inner(body,source,protected,None).await
    }
    pub async fn finish_copy(self,body:GeneratedBody,paste:bool)->Result<(),String> {
        if !self.finish_inner(body,"DezirClip Generated".into(),false,Some(paste)).await? {return Err(failed());}
        Ok(())
    }
    async fn finish_inner(mut self,body:GeneratedBody,source:String,protected:bool,paste:Option<bool>)->Result<bool,String> {
        use base64::Engine;
        let bytes=serde_json::to_vec(&body).map_err(|_|failed())?;
        if bytes.len()>32*1024*1024 {return Err("GeneratedBodyTooLarge".into());}
        for (index,bytes) in bytes.chunks(192*1024).enumerate() {
            let _:OperationResult=self.client.call(Method::DesktopCaptureWrite,CaptureWriteParams {capture_id:self.id.clone(),offset:(index*192*1024) as u32,
                data_base64:base64::engine::general_purpose::STANDARD.encode(bytes)},"OperationResult").await.map_err(remote_error)?;
        }
        let result:OperationResult=self.client.call(Method::DesktopCaptureFinish,CaptureFinishParams{capture_id:self.id,source_app:source.chars().take(256).collect(),inherited_protection:protected,paste},"OperationResult").await.map_err(remote_error)?;
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
#[cfg(test)]
mod queue_contract_tests {
    use super::*;
    #[test]
    fn a_missing_queue_body_rechecks_metadata_without_renewing_or_hiding_authority_errors() {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            let metadata=||DesktopEntry{id:7,content_type:"text".into(),content:String::new(),preview:String::new(),html_content:None,source_app:String::new(),tags:vec![],is_pinned:false,timestamp:1,is_external:false,use_count:0,pinned_order:0,file_preview_exists:false,content_kinds:vec![],ocr_text:None,ocr_status:None};
            assert_eq!(resolve_queue_body(Err("ClipboardRecordUnavailable".into()),async {Err("ClipboardRecordUnavailable".into())}).await.unwrap_err(),"ClipboardRecordUnavailable","deleted between summary and body must be skipped");
            assert_eq!(resolve_queue_body(Err("ClipboardRecordUnavailable".into()),async {Ok(metadata())}).await.unwrap_err(),"SensitiveAccessDenied","an existing protected row must remain queued");
            for error in ["PrivacyEpochChanged","ClipboardHostInstanceChanged","ClipboardHostUnavailable","ClipboardHostPermissionDenied"] {
                assert_eq!(resolve_queue_body(Err("ClipboardRecordUnavailable".into()),async {Err(error.into())}).await.unwrap_err(),error,"a failed original-authority recheck must never be called missing");
                assert_eq!(resolve_queue_body(Err(error.into()),async {panic!("only a missing body may trigger the extra metadata request")}).await.unwrap_err(),error);
            }
            assert_eq!(resolve_queue_body(Ok(metadata()),async {panic!("an available body needs no extra metadata request")}).await.unwrap().id,7);
        });
    }
    #[test]
    fn reserved_queue_epoch_is_not_refreshed_after_async_preparation() {
        let history=HistoryService::from_legacy_connection(Arc::new(Mutex::new(crate::database::init_db(":memory:").unwrap())));
        let epoch=history.runtime_snapshot(10).unwrap().privacy_epoch;
        let check=||check_queue_state("original",epoch,"original",history.runtime_snapshot(12).unwrap().privacy_epoch);
        assert!(check().is_ok());
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            tokio::task::yield_now().await;
            history.privacy().toggle(11).unwrap();
            assert_eq!(check().unwrap_err(),"PrivacyEpochChanged","a toggle before connect or during capability awaits must revoke the reserved epoch");
            let remote_epoch=history.runtime_snapshot(12).unwrap().privacy_epoch;
            assert_eq!(check_queue_state("original",epoch,"original",remote_epoch).unwrap_err(),"PrivacyEpochChanged");
            history.privacy().toggle(13).unwrap();
            assert_eq!(check().unwrap_err(),"PrivacyEpochChanged","toggling back cannot renew the old step");
        });
        assert_eq!(check_queue_state("original",epoch,"replacement",epoch).unwrap_err(),"ClipboardHostInstanceChanged");
    }
    #[test]
    fn old_or_ungranted_service_cannot_start_the_new_queue_flow() {
        let report=|feature:serde_json::Value,permissions:Vec<Permission>|CapabilityReport{methods:vec![],permissions,platform:serde_json::json!({"desktop_queue_completion":feature})};
        for feature in [serde_json::Value::Null,serde_json::json!({"supported":false,"version":1}),serde_json::json!({"supported":true,"version":2}),serde_json::json!({"supported":"true","version":1})] {
            assert!(!supports_queue_completion(&report(feature,vec![Permission::DesktopHistory])));
        }
        assert!(!supports_queue_completion(&report(serde_json::json!({"supported":true,"version":1}),vec![])));
        assert!(supports_queue_completion(&report(serde_json::json!({"supported":true,"version":1}),vec![Permission::DesktopHistory])));
    }
    #[test]
    fn queue_completion_keeps_original_identity_epoch_and_never_requests_sensitive_read() {
        for (delete,action) in [(false,"used"),(true,"delete")] {
            let request=queue_completion(7,"original-service".into(),13,delete);
            assert_eq!(request.id,7);assert_eq!(request.service_instance_id,"original-service");assert_eq!(request.expected_epoch,13);
            assert!(!request.reveal);assert_eq!(request.action,action);
            assert!(request.content.is_none() && request.tags.is_none() && request.pinned.is_none() && request.confirm_persistence.is_none());
            let bytes=dzc_host::wire::encode("DesktopMutationParams",&request).unwrap();
            assert!(dzc_host::wire::decode::<DesktopMutationParams>("DesktopMutationParams",&bytes).is_ok());
        }
    }
}
fn emit(name:&str,value:impl Serialize+Clone) {
    if matches!(name, "privacy-state-changed" | "clipboard-host-unavailable") {
        if let Some(app) = app() { crate::app::commands::screenshot_cmd::cancel_region_capture(app); }
        else { crate::app::commands::screenshot_cmd::clear_region_capture(); }
    }
    if let Some(app)=app() {let _=app.emit(name,value);}
}
fn failed()->String {"ClipboardHostUnavailable".into()}
fn dispatch_embedded<T:DeserializeOwned>(host:&Host,method:Method,params:impl Serialize)->Result<T,String> {
    let params=serde_json::to_value(params).map_err(|_|failed())?;
    let grant=host.grants().issue_control(ClientRole::Desktop).map_err(|_|failed())?;
    let accepted=host.hello(HelloParams{protocol_version:dzc_host::wire::VERSION,client_version:env!("CARGO_PKG_VERSION").into(),client_role:ClientRole::Desktop,
        capabilities:vec![],required_capabilities:vec![],grant:Some(grant.token().into())});
    let (session,_)=match accepted {Ok(value)=>value,Err(_)=>{host.grants().revoke(&grant);return Err(failed());}};
    let response=host.dispatch(&session,Request{request_id:"embedded-desktop".into(),method,params});
    host.grants().revoke(&grant);
    if let Some(error)=response.error {return Err(public_code(error.code));}
    serde_json::from_value(response.result.ok_or_else(failed)?).map_err(|_|failed())
}
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
fn confirmed_fallback<T>(host:&Host,create:impl FnOnce()->Result<T,String>)->Result<T,String> {
    host.shutdown().map_err(|_|"CaptureStopUnconfirmed".to_owned())?;
    create()
}
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
    #[cfg(test)]
    pub(crate) fn synthetic_fixture(history:HistoryService,session:Arc<dzc_core::api::SessionHistoryStore>)->Self {
        struct Actions;
        impl HostActions for Actions {
            fn platform_capabilities(&self)->serde_json::Value {serde_json::json!({})}
            fn can_copy(&self)->bool {false}
            fn copy(&self,_:dzc_core::ports::ClipboardPayload)->Result<(),WireErrorCode>{Err(WireErrorCode::Unavailable)}
            fn can_paste(&self)->bool {false}
            fn paste(&self)->Result<(),WireErrorCode>{Err(WireErrorCode::Unavailable)}
            fn stop_helpers(&self)->Result<(),WireErrorCode>{Ok(())}
        }
        let directory=PathBuf::new();
        let host=Host::with_session(HostOptions{mode:HostMode::Embedded,history:history.clone(),directory:directory.clone(),actions:Arc::new(Actions)},session.clone()).unwrap();
        Self(Arc::new(Inner{mode:Mutex::new(Mode::Embedded(host)),history,database:PathBuf::from(":memory:"),directory,session,closing:AtomicBool::new(false),writer:Mutex::new(None),server:Mutex::new(None),restore:Mutex::new(None)}))
    }
    pub fn start(app:&AppHandle) {
        let directory=app.state::<AppDataDir>().0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        let bridge=Self(Arc::new(Inner {mode:Mutex::new(Mode::Starting),history:app.state::<DbState>().history_service(),database:directory.join("clipboard.db"),directory,
            session:app.state::<SessionHistory>().0.clone(),closing:AtomicBool::new(false),writer:Mutex::new(None),server:Mutex::new(None),restore:Mutex::new(None)}));
        app.manage(bridge.clone());
        tauri::async_runtime::spawn(async move {if bridge.initialize().await.is_err() {bridge.set_mode(Mode::Unavailable);emit("clipboard-host-unavailable",());}});
    }
    fn set_mode(&self,mode:Mode) {
        crate::app::commands::screenshot_cmd::clear_region_capture();
        if let Ok(mut current)=self.0.mode.lock() {*current=mode;}
    }
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
        Mode::Embedded(host)=>Ok(host.instance_id().into()),Mode::Remote(instance)=>Ok(instance.clone()),_=>Err(failed())}}
    pub(crate) fn queue_state(&self)->Result<(String,u64),String> {
        let instance=self.instance()?;
        let epoch=self.0.history.runtime_snapshot(now()).map_err(remote_error)?.privacy_epoch;
        Ok((instance,epoch))
    }
    pub(crate) fn check_queue_state(&self,instance:&str,epoch:u64)->Result<(),String> {
        let (actual_instance,actual_epoch)=self.queue_state()?;
        check_queue_state(instance,epoch,&actual_instance,actual_epoch)
    }
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
            return dispatch_embedded(&host,method,params);
        }
        let mut client=self.service_client().await?;
        if client.hello.state.service_instance_id!=self.instance()? {return Err("ClipboardHostInstanceChanged".into());}
        client.call(method,params,schema).await.map_err(remote_error)
    }
    pub(crate) fn call_embedded<T:DeserializeOwned>(&self,method:Method,params:impl Serialize)->Result<T,String> {
        let host=self.embedded()?.ok_or_else(failed)?;
        dispatch_embedded(&host,method,params)
    }
    pub(crate) fn complete_embedded_queue(&self,id:i64,instance:&str,epoch:u64,delete:bool)->Result<(),String> {
        let result:DesktopMutationResult=self.call_embedded(Method::DesktopHistoryMutate,queue_completion(id,instance.into(),epoch,delete))?;
        if result.id!=id || !result.performed {return Err(failed());} Ok(())
    }
    pub(crate) async fn prepare_service_queue(&self,id:i64,original_instance:&str,epoch:u64)->Result<ServiceQueueCopy,String> {
        self.check_queue_state(original_instance,epoch)?;
        let mut client=self.service_client().await?;
        check_queue_state(original_instance,epoch,&client.hello.state.service_instance_id,client.hello.state.privacy_epoch)?;
        self.check_queue_state(original_instance,epoch)?;
        let capabilities:CapabilityReport=client.call(Method::Capabilities,serde_json::json!({}),"CapabilityReport").await.map_err(remote_error)?;
        if !supports_queue_completion(&capabilities) {return Err("ClipboardHostCapabilityUnavailable".into());}
        let privacy:PrivacyState=client.call(Method::PrivacyGet,serde_json::json!({}),"PrivacyState").await.map_err(remote_error)?;
        check_queue_state(original_instance,epoch,original_instance,privacy.privacy_epoch)?;
        self.check_queue_state(original_instance,epoch)?;
        let summary=service_queue_entry(&mut client,id,original_instance,epoch,Method::DesktopHistorySummary).await;
        self.check_queue_state(original_instance,epoch)?;
        let summary=summary?;
        let content=if matches!(summary.content_type.as_str(),"text"|"code"|"url"|"rich_text") {
            let body=service_queue_entry(&mut client,id,original_instance,epoch,Method::DesktopHistoryGet).await;
            let entry=resolve_queue_body(body,service_queue_entry(&mut client,id,original_instance,epoch,Method::DesktopHistorySummary)).await;
            self.check_queue_state(original_instance,epoch)?;
            let entry=entry?;
            if entry.content_type!=summary.content_type {return Err(failed());}
            Some(entry.content)
        }else{None};
        Ok(ServiceQueueCopy{client,instance:original_instance.into(),epoch,id,content_type:summary.content_type,content})
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
    pub async fn tag_page(&self,tag:String,query:String,sort_by:String,limit:usize,offset:usize,epoch:u64)->Result<(dzc_core::api::TagItemsPage,String),String> {
        let instance=self.instance()?;
        self.check_queue_state(&instance,epoch)?;
        let request=DesktopTagPageParams{tag,query,sort_by,limit:u16::try_from(limit).map_err(|_|failed())?,
            offset:u32::try_from(offset).map_err(|_|failed())?,service_instance_id:instance.clone(),expected_epoch:epoch};
        let page=if let Some(host)=self.embedded()? {
            tauri::async_runtime::spawn_blocking(move||host.tag_page_owned(request).map_err(public_code)).await.map_err(|_|failed())??
        }else {
            let mut client=self.service_client().await?;
            if client.hello.state.service_instance_id!=instance {return Err("ClipboardHostInstanceChanged".into());}
            let descriptor:BodySnapshot=client.call(Method::DesktopTagsPage,request,"BodySnapshot").await.map_err(remote_error)?;
            if descriptor.state.service_instance_id!=instance || descriptor.state.privacy_epoch!=epoch {return Err("PrivacyEpochChanged".into());}
            client.read_snapshot::<DesktopTagItemsPage>(descriptor,"DesktopTagItemsPage").await.map_err(remote_error)?
        };
        self.check_queue_state(&instance,epoch)?;
        let items=page.items.into_iter().map(|entry|hosted(entry,&instance).map(|entry|entry.entry)).collect::<Result<Vec<_>,_>>()?;
        Ok((dzc_core::api::TagItemsPage{items,total:usize::try_from(page.total).map_err(|_|failed())?,
            has_more:page.has_more,next_offset:page.next_offset.map(|offset|offset as usize)},instance))
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
        self.begin_upload(false,None).await
    }
    pub(crate) async fn begin_generated_for_snapshot(&self,instance:&str,epoch:u64)->Result<Option<GeneratedUpload>,String> {
        if self.instance()?!=instance {return Err("ClipboardHostInstanceChanged".into());}
        let mut client=self.service_client().await?;
        let request=frozen_capture_request(instance,epoch,&client.hello.state.service_instance_id,client.hello.state.privacy_epoch)?;
        if self.instance()?!=instance {return Err("ClipboardHostInstanceChanged".into());}
        let result:CaptureBeginResult=client.call(Method::DesktopCaptureBegin,request,"CaptureBeginResult").await.map_err(remote_error)?;
        check_queue_state(instance,epoch,&result.state.service_instance_id,result.state.privacy_epoch)?;
        Ok(result.capture_id.map(|id|GeneratedUpload{client,id}))
    }
    pub async fn begin_generated_copy(&self,epoch:Option<u64>)->Result<GeneratedUpload,String> {
        self.begin_upload(true,epoch).await?.ok_or_else(failed)
    }
    async fn begin_upload(&self,copy_to_clipboard:bool,epoch:Option<u64>)->Result<Option<GeneratedUpload>,String> {
        let mut client=self.service_client().await?;
        if client.hello.state.service_instance_id!=self.instance()? {return Err("ClipboardHostInstanceChanged".into());}
        if copy_to_clipboard {client.require_generated_copy().await.map_err(|_|"ClipboardHostCapabilityUnavailable".to_owned())?;}
        let state=client.hello.state.clone();
        let result:CaptureBeginResult=client.call(Method::DesktopCaptureBegin,CaptureBeginParams{service_instance_id:state.service_instance_id,expected_epoch:epoch.unwrap_or(state.privacy_epoch),copy_to_clipboard:Some(copy_to_clipboard)},"CaptureBeginResult").await.map_err(remote_error)?;
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
        let (mut host,mut writer)=self.fresh_embedded()?;
        let unavailable=match CaptureManager::start_with_hooks(&host,Arc::new(crate::services::clipboard::DesktopHooks)) {
            Ok((_,Ok(())))=>None,
            Ok((_,Err(_)))=>Some("native_listener_unavailable"),
            Err(WireErrorCode::Busy)=>Some("capture_ownership_unavailable"),
            Err(_)=>Some("native_listener_unavailable"),
        };
        if let Some(reason)=unavailable {
            (host,writer)=confirmed_fallback(&host,||self.fresh_embedded())?;
            host.set_capture_diagnostic(reason).map_err(public_code)?;
            emit("clipboard-capture-unavailable",reason);
        }else if host.initialize_privacy_shortcut().is_err() {
            let _=host.publish(EventKind::ServiceStateChanged,EventData{entry_id:None,status:Some("PrivacyShortcutUnavailable".into())});
            emit("privacy-shortcut-unavailable",());
        }
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
    fn fresh_embedded(&self)->Result<(Arc<Host>,dzc_platform::runtime::EmbeddedClipboardWriter),String> {
        let writer=dzc_platform::runtime::EmbeddedClipboardWriter::default();
        let host=Host::with_session(HostOptions{mode:HostMode::Embedded,history:self.0.history.clone(),directory:self.0.directory.clone(),
            actions:Arc::new(DesktopActions{writer:writer.clone()})},self.0.session.clone()).map_err(public_code)?;
        Ok((host,writer))
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
    #[test]
    fn confirmed_fallback_keeps_history_only_host_behind_confirmed_worker_stop() {
        struct Actions;
        impl HostActions for Actions {
            fn platform_capabilities(&self)->serde_json::Value {serde_json::json!({})}
            fn can_copy(&self)->bool {false}
            fn copy(&self,_:dzc_core::ports::ClipboardPayload)->Result<(),WireErrorCode> {Err(WireErrorCode::Unavailable)}
            fn can_paste(&self)->bool {false}
            fn paste(&self)->Result<(),WireErrorCode> {Err(WireErrorCode::Unavailable)}
            fn stop_helpers(&self)->Result<(),WireErrorCode> {Ok(())}
            fn revoke_private_exports(&self)->Result<(),WireErrorCode> {Ok(())}
        }
        struct Lifecycle {fail:AtomicBool,released:std::sync::atomic::AtomicUsize}
        impl dzc_host::HostLifecycle for Lifecycle {
            fn stop_capture(&self)->Result<(),WireErrorCode> {
                if self.fail.load(Ordering::Acquire) {Err(WireErrorCode::Busy)}else{Ok(())}
            }
            fn release_ownership(&self)->Result<(),WireErrorCode> {self.released.fetch_add(1,Ordering::AcqRel);Ok(())}
            fn capture_active(&self)->bool {self.released.load(Ordering::Acquire)==0}
        }
        let directory=std::env::temp_dir().join(format!("dzc-history-fallback-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let history=HistoryService::open(&directory.join("clipboard.db")).unwrap();
        let fresh=||Host::new(HostOptions{mode:HostMode::Embedded,history:history.clone(),directory:directory.clone(),actions:Arc::new(Actions)}).map_err(public_code);
        let host=fresh().unwrap();
        let worker=Arc::new(Lifecycle{fail:AtomicBool::new(true),released:std::sync::atomic::AtomicUsize::new(0)});
        host.attach_lifecycle(worker.clone()).unwrap();
        let created=std::sync::atomic::AtomicUsize::new(0);
        let create=||{created.fetch_add(1,Ordering::AcqRel);fresh()};
        assert!(matches!(confirmed_fallback(&host,create),Err(error) if error=="CaptureStopUnconfirmed"));
        assert_eq!(created.load(Ordering::Acquire),0);
        assert_eq!(worker.released.load(Ordering::Acquire),0);
        assert!(!host.capture_stopped());
        worker.fail.store(false,Ordering::Release);
        let passive=confirmed_fallback(&host,create).unwrap();
        assert_eq!(created.load(Ordering::Acquire),1);
        assert_eq!(worker.released.load(Ordering::Acquire),1);
        assert!(host.capture_stopped());
        passive.set_capture_diagnostic("native_listener_unavailable").unwrap();
        assert_eq!(passive.set_capture_diagnostic("untrusted native environment"),Err(WireErrorCode::InvalidRequest));
        let (session,_)=passive.hello(HelloParams{protocol_version:VERSION,client_version:"synthetic-fallback".into(),client_role:ClientRole::Surface,
            capabilities:vec![],required_capabilities:vec![],grant:None}).unwrap();
        let response=passive.dispatch(&session,Request{request_id:"fallback-status".into(),method:Method::ServiceStatus,params:serde_json::json!({})});
        let status=response.result.unwrap();
        assert_eq!(status["capture_owner"],false);
        assert_eq!(status["capture_unavailable_reason"],"native_listener_unavailable");
        assert_ne!(passive.snapshot().unwrap().service_instance_id,host.snapshot().unwrap().service_instance_id);
        passive.shutdown().unwrap();drop(passive);drop(host);drop(history);
        std::fs::remove_dir_all(directory).unwrap();
    }
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
                directory,database:database.clone(),session:Arc::new(dzc_core::api::SessionHistoryStore::default()),closing:AtomicBool::new(false),writer:Mutex::new(None),server:Mutex::new(None),restore:Mutex::new(None)}));
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
