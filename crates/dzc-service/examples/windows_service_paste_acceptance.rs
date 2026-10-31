//! Disposable CI client; never packaged or exposed as a Desktop command.
use dzc_host::{Client, controller};
use dzc_protocol::{build_info::{BuildInfo, Component}, wire::*};
use serde_json::{Value, json};
use std::{io::{self, BufRead, Read, Write}, path::{Path, PathBuf}, time::{Duration, SystemTime, UNIX_EPOCH}};

fn require(value: bool, message: &str) -> Result<(), Box<dyn std::error::Error>> {
    if !value { return Err(message.into()); }
    Ok(())
}
#[derive(Debug, Default)]
enum StartupIntent {
    #[default]
    Uninitialized,
    Reserved,
    Failed,
    Frozen(RuntimeState),
}
impl StartupIntent {
    fn reserve(&mut self)->Result<(),Box<dyn std::error::Error>> {
        require(matches!(self,Self::Uninitialized),"StartupBaselineAlreadyAttempted")?;
        *self=Self::Reserved;Ok(())
    }
    fn freeze(&mut self,initial:&RuntimeState,desktop:&RuntimeState,status:&ServiceStatus,health:&RuntimeState)->Result<RuntimeState,Box<dyn std::error::Error>> {
        require(matches!(std::mem::replace(self,Self::Failed),Self::Reserved),"StartupBaselineNotReserved")?;
        require(status.mode==HostMode::Service && !status.capture_owner && !status.privacy.active,"ExplicitNoCaptureServiceRequired")?;
        require(!initial.service_instance_id.is_empty() && desktop.service_instance_id==initial.service_instance_id
            && desktop.privacy_epoch>=initial.privacy_epoch,"OriginalStartupServiceRequired")?;
        for state in [&status.state,health] {
            require(state.service_instance_id==desktop.service_instance_id && state.privacy_epoch==desktop.privacy_epoch,"StartupBaselineDisagrees")?;
        }
        require(status.privacy.privacy_epoch==desktop.privacy_epoch,"StartupPrivacyStateDisagrees")?;
        let state=status.state.clone();*self=Self::Frozen(state.clone());Ok(state)
    }
    fn state(&self)->Result<&RuntimeState,Box<dyn std::error::Error>> {
        if let Self::Frozen(state)=self {Ok(state)}else {Err("StartupBaselineRequired".into())}
    }
    fn check(&self,current:&RuntimeState)->Result<(),Box<dyn std::error::Error>> {
        let fixed=self.state()?;
        require(current.service_instance_id==fixed.service_instance_id && current.privacy_epoch==fixed.privacy_epoch,"FrozenStartupIntentChanged")
    }
}
async fn checked_state(client:&mut Client,intent:&StartupIntent)->Result<RuntimeState,Box<dyn std::error::Error>> {
    intent.state()?;
    let state:RuntimeState=client.call(Method::Health,json!({}),"RuntimeState").await?;
    intent.check(&state)?;Ok(state)
}
fn generated_marker(text:&str)->bool {
    text.strip_prefix("Service paste synthetic generated ").is_some_and(|suffix|suffix.len()==32
        && suffix.bytes().all(|byte|byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()))
}
fn fixture(database: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    require(cfg!(target_os="windows") && std::env::var("GITHUB_ACTIONS").as_deref()==Ok("true")
        && std::env::var("RUNNER_ENVIRONMENT").as_deref()==Ok("github-hosted")
        && std::env::var("GITHUB_REPOSITORY").as_deref()==Ok("qiyuanhuakai/dezirclip"), "DisposableWindowsCiRequired")?;
    let expected=BuildInfo::current_with_host(Component::Desktop,dzc_core::api::DATABASE_SCHEMA_VERSION);
    require(expected.source_state=="verified_checkout" && expected.source_revision==std::env::var("GITHUB_SHA").ok(), "SourceIdentityRequired")?;
    let temporary=PathBuf::from(std::env::var_os("RUNNER_TEMP").ok_or("RunnerTempRequired")?).canonicalize()?;
    require(database.is_absolute() && database.file_name().is_some_and(|name|name=="clipboard.db"), "PrivateDatabaseRequired")?;
    let parent=database.parent().ok_or("PrivateDatabaseRequired")?.canonicalize()?;
    let root=parent.parent().ok_or("PrivateRootRequired")?;
    require(parent.file_name().is_some_and(|name|name=="profile") && root.starts_with(&temporary)
        && root!=temporary && root.file_name().is_some_and(|name|name.to_string_lossy().starts_with("dezirclip-service-paste-")), "PrivateRootRequired")?;
    let mut current=Some(parent.as_path());
    while let Some(path)=current {
        let metadata=std::fs::symlink_metadata(path)?;
        require(metadata.is_dir() && !metadata.file_type().is_symlink(), "PrivateDirectoryChanged")?;
        #[cfg(target_os="windows")]
        { use std::os::windows::fs::MetadataExt; require(metadata.file_attributes()&0x400==0,"PrivateDirectoryReparsePoint")?; }
        if path==temporary {break;}
        current=path.parent();
    }
    if database.exists() {
        let metadata=std::fs::symlink_metadata(database)?;
        require(metadata.is_file() && !metadata.file_type().is_symlink(),"PrivateDatabaseChanged")?;
        #[cfg(target_os="windows")]
        {use std::os::windows::fs::MetadataExt;require(metadata.file_attributes()&0x400==0,"PrivateDatabaseReparsePoint")?;}
    }
    Ok(parent.join("clipboard.db"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generated_body_encoding_has_exact_standard_base64_padding() {
        for (input,expected) in [("",""),("f","Zg=="),("fo","Zm8="),("foo","Zm9v"),("foobar","Zm9vYmFy")] {
            assert_eq!(base64(input.as_bytes()),expected);
        }
    }
    #[test]
    fn control_lines_require_a_newline_and_reject_oversize_instead_of_reinterpreting_a_tail() {
        assert_eq!(read_line(&mut io::Cursor::new(b"{\"id\":1}\n")).unwrap().unwrap()["id"],1);
        assert!(read_line(&mut io::Cursor::new(b"{}")).is_err());
        assert!(read_line(&mut io::Cursor::new(vec![b' ';8193])).is_err());
        assert!(read_line(&mut io::Cursor::new(b"")).unwrap().is_none());
    }
    #[test]
    fn generated_marker_requires_exact_unique_lowercase_nonce() {
        assert!(generated_marker(&format!("Service paste synthetic generated {}","a1".repeat(16))));
        for text in ["Service paste synthetic generated ".into(),format!("Service paste synthetic generated {}","A".repeat(32)),
            format!("Service paste synthetic generated {}","a".repeat(33)),format!("unrelated{}","a".repeat(32))] {
            assert!(!generated_marker(&text));
        }
    }
    fn state(epoch:u64)->RuntimeState {
        RuntimeState{service_instance_id:"original-service".into(),privacy_epoch:epoch,history_revision:2,event_sequence:1}
    }
    fn service(epoch:u64)->ServiceStatus {
        ServiceStatus{mode:HostMode::Service,capture_owner:false,state:state(epoch),
            privacy:PrivacyState{active:false,privacy_epoch:epoch,expires_at_ms:None},capture_unavailable_reason:Some("disabled_by_request".into())}
    }
    #[test]
    fn startup_intent_is_frozen_only_once_after_both_peers_agree() {
        let initial=state(0);let mut intent=StartupIntent::default();
        assert!(intent.state().is_err());intent.reserve().unwrap();assert!(intent.state().is_err());
        let frozen=intent.freeze(&initial,&state(4),&service(4),&state(4)).unwrap();
        assert_eq!(frozen.privacy_epoch,4);assert_eq!(initial.privacy_epoch,0);
        assert!(intent.reserve().is_err());assert_eq!(intent.state().unwrap().privacy_epoch,4);
        assert!(intent.check(&state(4)).is_ok());assert!(intent.check(&state(0)).is_err());assert!(intent.check(&state(5)).is_err());
        let mut changed=state(4);changed.service_instance_id="replacement-service".into();assert!(intent.check(&changed).is_err());
        let mut changed=state(4);changed.history_revision+=1;changed.event_sequence+=1;assert!(intent.check(&changed).is_ok());
        assert_eq!(intent.state().unwrap().privacy_epoch,4);
    }
    #[test]
    fn failed_baseline_cannot_be_retried_or_used_for_an_operation() {
        for variant in 0..7 {
            let initial=state(3);let mut desktop=state(4);let mut status=service(4);let mut health=state(4);
            match variant {
                0=>desktop.privacy_epoch=2,
                1=>status.state.privacy_epoch=5,
                2=>health.privacy_epoch=5,
                3=>health.service_instance_id="replacement-service".into(),
                4=>status.capture_owner=true,
                5=>status.privacy.active=true,
                _=>status.privacy.privacy_epoch=0,
            }
            let mut intent=StartupIntent::default();intent.reserve().unwrap();
            assert!(intent.freeze(&initial,&desktop,&status,&health).is_err());
            assert!(intent.state().is_err());assert!(intent.reserve().is_err());assert!(intent.check(&state(4)).is_err());
        }
        let mut intent=StartupIntent::default();
        assert!(intent.freeze(&state(0),&state(4),&service(4),&state(4)).is_err());assert!(intent.reserve().is_err());
    }
}
fn read_line(reader: &mut impl BufRead) -> Result<Option<Value>, Box<dyn std::error::Error>> {
    let mut bytes=Vec::new();
    let size=reader.take(8193).read_until(b'\n',&mut bytes)?;
    if size==0 {return Ok(None);}
    require(size<=8192 && bytes.last()==Some(&b'\n'),"BoundedCommandRequired")?;
    Ok(Some(serde_json::from_slice(&bytes)?))
}
fn emit(value: Value) -> Result<(), Box<dyn std::error::Error>> {
    println!("{}",value);io::stdout().flush()?;Ok(())
}
fn base64(bytes:&[u8])->String {
    const TABLE:&[u8;64]=b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output=String::new();
    for part in bytes.chunks(3) {
        let bits=(u32::from(part[0])<<16)|(u32::from(*part.get(1).unwrap_or(&0))<<8)|u32::from(*part.get(2).unwrap_or(&0));
        output.push(TABLE[((bits>>18)&63) as usize] as char);output.push(TABLE[((bits>>12)&63) as usize] as char);
        output.push(if part.len()>1 {TABLE[((bits>>6)&63) as usize] as char}else{'='});
        output.push(if part.len()>2 {TABLE[(bits&63) as usize] as char}else{'='});
    }
    output
}
fn main()->Result<(),Box<dyn std::error::Error>> {
    let mode=std::env::args().nth(1).ok_or("ModeRequired")?;
    let database=fixture(&PathBuf::from(std::env::args_os().nth(2).ok_or("DatabaseRequired")?))?;
    let mut input=io::stdin().lock();
    if mode=="--seed" {
        require(!database.exists(),"SeedMustPrecedeService")?;
        let body=read_line(&mut input)?.ok_or("SeedRequired")?;
        let history=dzc_core::api::HistoryService::open(&database)?;
        let now=i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())?;
        let mut ids=Vec::new();
        for (index,key) in ["first","second"].iter().enumerate() {
            let text=body[*key].as_str().filter(|text|text.len()<=256 && text.starts_with("Service paste synthetic ")).ok_or("SyntheticSeedRequired")?;
            let entry:dzc_core::api::ClipboardEntry=serde_json::from_value(json!({"id":0,"content_type":"text","content":text,
                "preview":text,"source_app":"Service CI fixture","timestamp":now+index as i64,"is_pinned":false}))?;
            ids.push(history.save_manual(&entry,None,dzc_core::api::ManualPersistenceIntent::Ordinary,now+index as i64)?);
        }
        return emit(json!({"seeded_ids":ids,"build":BuildInfo::current_with_host(Component::Desktop,dzc_core::api::DATABASE_SCHEMA_VERSION)}));
    }
    require(mode=="--client","InvalidMode")?;
    let expected_pid: u32=std::env::args().nth(3).ok_or("ServicePidRequired")?.parse()?;
    let target_pid: u32=std::env::args().nth(4).ok_or("OriginalTargetPidRequired")?.parse()?;
    require(expected_pid>0 && target_pid>0 && expected_pid!=target_pid,"DistinctOriginalProcessesRequired")?;
    let expected=BuildInfo::current_with_host(Component::Desktop,dzc_core::api::DATABASE_SCHEMA_VERSION);
    tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(async {
        let endpoint=controller::endpoint(&database)?;
        let deadline=tokio::time::Instant::now()+Duration::from_secs(15);
        let mut client=loop {
            match Client::control(&endpoint,ClientRole::Desktop).await {
                Ok(client)=>break client,
                Err(error) if tokio::time::Instant::now()<deadline=>{
                    if !matches!(error.kind(),io::ErrorKind::NotFound|io::ErrorKind::ConnectionRefused|io::ErrorKind::TimedOut|io::ErrorKind::WouldBlock) {return Err(error.into());}
                    tokio::time::sleep(Duration::from_millis(30)).await;
                },
                Err(error)=>return Err(error.into()),
            }
        };
        require(client.peer.pid()==Some(expected_pid),"OriginalServicePeerRequired")?;
        client.require_windows_bound_paste(&expected).await?;
        let status:ServiceStatus=client.call(Method::ServiceStatus,json!({}),"ServiceStatus").await?;
        require(status.mode==HostMode::Service && !status.capture_owner,"ExplicitNoCaptureServiceRequired")?;
        let capabilities:CapabilityReport=client.call(Method::Capabilities,json!({}),"CapabilityReport").await?;
        emit(json!({"event":"ready","status":status,"capabilities":capabilities,"build":expected,"peer_pid":expected_pid}))?;
        let mut intent=StartupIntent::default();
        let mut held=None;
        let mut original_target=None;
        while let Some(command)=read_line(&mut input)? {
            let operation=command["operation"].as_str().ok_or("OperationRequired")?;
            let result:Value=match operation {
                "freeze-startup-baseline"=>{
                    intent.reserve()?;
                    let desktop:RuntimeState=serde_json::from_value(command["expected_state"].clone())?;
                    require(client.peer.pid()==Some(expected_pid),"OriginalServicePeerRequired")?;
                    client.require_windows_bound_paste(&expected).await?;
                    let current:ServiceStatus=client.call(Method::ServiceStatus,json!({}),"ServiceStatus").await?;
                    let health:RuntimeState=client.call(Method::Health,json!({}),"RuntimeState").await?;
                    let frozen=intent.freeze(&status.state,&desktop,&current,&health)?;
                    json!({"immutable":true,"same_connection_capability_rechecked":true,"state":frozen,"status":current,"health":health})
                },
                "snapshot"=>{
                    let state=checked_state(&mut client,&intent).await?;
                    let descriptor:BodySnapshot=client.call(Method::DesktopHistoryPage,DesktopPageParams{limit:20,offset:0,kind:None,query:None},"BodySnapshot").await?;
                    intent.check(&descriptor.state)?;
                    let entries:DesktopEntries=client.read_snapshot(descriptor,"DesktopEntries").await?;
                    checked_state(&mut client,&intent).await?;
                    json!({"state":state,"entries":entries.entries})
                },
                "reject-copy"=>{
                    checked_state(&mut client,&intent).await?;
                    let fixed=intent.state()?.clone();
                    let target:WindowsPasteTarget=serde_json::from_value(command["target"].clone())?;
                    require(target.pid==target_pid && original_target.as_ref().is_none_or(|original|original==&target),"OriginalFixtureTargetRequired")?;
                    original_target=Some(target.clone());
                    let id=command["entry_id"].as_i64().filter(|id|*id>0).ok_or("SeedIdRequired")?;
                    let response=client.call::<OperationResult>(Method::DesktopClipboardCopy,DesktopCopyParams {
                        id,service_instance_id:fixed.service_instance_id,expected_epoch:fixed.privacy_epoch,reveal:false,paste:true,
                        with_format:Some(false),derived_text:None,image_as_base64:None,delete_after_use:Some(true),move_to_top:Some(true),expected_paste_target:Some(target),
                    },"OperationResult").await;
                    checked_state(&mut client,&intent).await?;
                    let error=response.err().ok_or("ChangedTargetUnexpectedlyAccepted")?;
                    require(error.kind()==io::ErrorKind::PermissionDenied && error.to_string()==serde_json::to_string(&WireErrorCode::Unavailable)?,"ExpectedTargetRefusalRequired")?;
                    json!({"rejected":true,"error":error.to_string()})
                },
                "begin-generated"=>{
                    checked_state(&mut client,&intent).await?;
                    let fixed=intent.state()?.clone();
                    require(held.is_none(),"OneGeneratedTransactionRequired")?;
                    let text=command["text"].as_str().filter(|text|generated_marker(text)).ok_or("UniqueGeneratedFixtureRequired")?;
                    let target:WindowsPasteTarget=serde_json::from_value(command["target"].clone())?;
                    require(target.pid==target_pid && original_target.as_ref()==Some(&target),"SameOriginalFixtureTargetRequired")?;
                    let started=tokio::time::Instant::now();
                    let begin:CaptureBeginResult=client.call(Method::DesktopCaptureBegin,CaptureBeginParams{
                        service_instance_id:fixed.service_instance_id,expected_epoch:fixed.privacy_epoch,copy_to_clipboard:Some(true),expected_paste_target:Some(target),
                    },"CaptureBeginResult").await?;
                    intent.check(&begin.state)?;
                    let id=begin.capture_id.ok_or("GeneratedTransactionRequired")?;
                    let body=json!({"kind":"text","text":text});
                    let copied:OperationResult=client.call(Method::DesktopCaptureWrite,CaptureWriteParams{
                        capture_id:id.clone(),offset:0,data_base64:base64(&serde_json::to_vec(&body)?),
                    },"OperationResult").await?;
                    require(copied.performed,"GeneratedWriteRequired")?;
                    checked_state(&mut client,&intent).await?;
                    held=Some((id,started+Duration::from_secs(5)));json!({"prepared":true,"state":begin.state,"conservative_ttl_ms":5000})
                },
                "reject-finish"=>{
                    let (id,deadline)=held.take().ok_or("OriginalGeneratedTransactionRequired")?;
                    require(tokio::time::Instant::now()<deadline,"GeneratedFixtureExpiredBeforeFinish")?;
                    client.require_windows_bound_paste(&expected).await?;
                    checked_state(&mut client,&intent).await?;
                    require(tokio::time::Instant::now()<deadline,"GeneratedFixtureExpiredBeforeFinish")?;
                    let response=client.call::<OperationResult>(Method::DesktopCaptureFinish,CaptureFinishParams{
                        capture_id:id,source_app:"Service CI fixture".into(),inherited_protection:false,paste:Some(true),
                    },"OperationResult").await;
                    require(tokio::time::Instant::now()<deadline,"GeneratedFixtureExpiredAfterFinish")?;
                    checked_state(&mut client,&intent).await?;
                    require(tokio::time::Instant::now()<deadline,"GeneratedFixtureExpiredAfterFinish")?;
                    let error=response.err().ok_or("ChangedGeneratedTargetUnexpectedlyAccepted")?;
                    require(error.kind()==io::ErrorKind::PermissionDenied && error.to_string()==serde_json::to_string(&WireErrorCode::Unavailable)?,"ExpectedHeldTargetRefusalRequired")?;
                    json!({"rejected":true,"error":error.to_string(),"inside_conservative_ttl":true,"same_connection_capability_rechecked":true,"clipboard_may_have_been_copied":true})
                },
                "stop"=>{
                    checked_state(&mut client,&intent).await?;
                    let result:OperationResult=client.call(Method::ServiceStop,json!({}),"OperationResult").await?;
                    emit(json!({"id":command["id"],"value":result}))?;break;
                },
                _=>return Err("UnknownOperation".into()),
            };
            emit(json!({"id":command["id"],"value":result}))?;
        }
        Ok::<(),Box<dyn std::error::Error>>(())
    })
}
