mod utils;

use crate::app_state::{PasteQueue, SettingsState};
pub use crate::database::DbState;
use dzc_core::capture::{CaptureContext, CaptureEngine, CaptureEvent, CaptureFrame, CaptureOutcome, CaptureReadMetadata, CaptureSettings, CaptureSource, StoredCapture, OwnClipboardCopy};
use std::sync::{Arc, Mutex, atomic::Ordering};
use tauri::{AppHandle, Emitter, Manager};
#[cfg(target_os = "linux")]
use crate::infrastructure::linux_api::window_tracker::{get_clipboard_capture_app_info as resolve_source,ActiveAppInfo};
#[cfg(target_os = "windows")]
use crate::infrastructure::windows_api::window_tracker::{get_clipboard_source_app_info as resolve_source,ActiveAppInfo};
pub use dzc_core::capture::CaptureData as ClipboardData;
pub use utils::truncate_html_for_preview;

pub(crate) fn summarize_captured_entry(entry: crate::domain::models::ClipboardEntry)->crate::domain::models::ClipboardEntry {
    match utils::truncate_entry_for_ui(&entry) {std::borrow::Cow::Borrowed(_)=>entry,std::borrow::Cow::Owned(summary)=>summary}
}
fn timestamp()->i64 {chrono::Utc::now().timestamp_millis()}
fn app()->Option<&'static AppHandle> {crate::global_state::GLOBAL_APP_HANDLE.get()}
fn source(snapshot:Option<ActiveAppInfo>,override_name:Option<String>)->CaptureSource {
    let snapshot=snapshot.unwrap_or_else(resolve_source);
    CaptureSource{app_name:override_name.clone().unwrap_or(snapshot.app_name),process_path:if override_name.is_some(){None}else{snapshot.process_path}}
}
fn deliver_capture()->bool {
    crate::app::idle_destroyer::should_deliver_capture(crate::app::idle_destroyer::main_window_on_screen(),!crate::global_state::IS_DESTROYED.load(Ordering::Relaxed))
}
fn own_copy(timestamp:i64)->OwnClipboardCopy {
    OwnClipboardCopy{primary:crate::LAST_APP_SET_HASH.load(Ordering::SeqCst),alternative:crate::LAST_APP_SET_HASH_ALT.load(Ordering::SeqCst),
        age_seconds:(timestamp.max(0) as u64/1000).saturating_sub(crate::LAST_APP_SET_TIMESTAMP.load(Ordering::SeqCst))}
}
fn clear_own_copy() {crate::LAST_APP_SET_HASH.store(0,Ordering::SeqCst);crate::LAST_APP_SET_HASH_ALT.store(0,Ordering::SeqCst);}

/// No AppHandle is retained by native workers. The actual Host owns the shared
/// session, bounded processor, original-ticket OCR and capture lease.
pub(crate) struct DesktopHooks;
impl dzc_host::CaptureHooks for DesktopHooks {
    fn source(&self)->Option<CaptureSource> {Some(source(None,None))}
    fn paused(&self)->bool {crate::CLIPBOARD_MONITOR_PAUSED.load(Ordering::Relaxed)}
    fn own_copy(&self,time:i64)->OwnClipboardCopy {own_copy(time)}
    fn clear_own_copy(&self) {clear_own_copy();}
    fn sequential_echo(&self)->Option<String> {
        let app=app()?;
        if !app.state::<SettingsState>().sequential_mode.load(Ordering::Relaxed) {return None;}
        let state=app.state::<PasteQueue>();
        let queue=state.0.lock().ok()?;
        queue.last_action_was_paste.then(||queue.last_pasted_content.clone()).flatten()
    }
    fn event(&self,event:CaptureEvent) {
        match event {
            CaptureEvent::Removed{id}=>if let Some(app)=app() {if deliver_capture(){let _=app.emit("clipboard-removed",id);}},
            CaptureEvent::Rejected(reason)=>crate::warn!("[capture] rejected: {:?}",reason),
            CaptureEvent::Failed(error)=>crate::warn!("[capture] {:?}: {}",error.stage,error.code),
            _=>(),
        }
    }
    fn stored(&self,capture:StoredCapture,work:dzc_host::CaptureWork) {if let Some(app)=app() {distribute(app,capture,work);}}
}
fn distribute(app:&AppHandle,capture:StoredCapture,work:dzc_host::CaptureWork) {
    let history=app.state::<DbState>().history_service();
    let Some(ticket)=capture.privacy_ticket else {return;};
    if history.privacy().check_capture(&ticket,timestamp()).is_err() {return;}
    let settings=app.state::<SettingsState>();
    let id=capture.entry.id;
    let updated=Arc::new(move|id:i64,status:&'static str| {
        if let Some(app)=self::app() {
            let _=app.emit("ocr:complete",crate::app::commands::ocr_cmd::OcrResult{item_id:id,text:String::new(),confidence:None,status:status.into()});
            if let Some(bridge)=app.try_state::<crate::app::ipc_host::DesktopHost>() {
                if let Ok(Some(host))=bridge.embedded() {let _=host.publish(dzc_host::wire::EventKind::EntryUpdated,dzc_host::wire::EventData{entry_id:Some(id),status:Some(status.into())});}
            }
        }
    });
    if dzc_host::schedule_capture_ocr(history,&capture,work,updated).is_err() {crate::warn!("[capture] CaptureFollowUpUnavailable");}
    let deliver=deliver_capture();
    if !deliver {crate::app::idle_destroyer::mark_frontend_catchup_pending();}
    if settings.sequential_mode.load(Ordering::Relaxed) {
        let state=app.state::<PasteQueue>();
        let mut queue=state.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if queue.last_action_was_paste {queue.items.clear();queue.reset_tracking();}
        queue.items.push_back(id);let length=queue.items.len();drop(queue);
        let _=app.emit("paste-queue-changed",length);
    }
    if settings.sound_enabled.load(Ordering::Relaxed) {let _=app.emit("play-sound","copy");}
    if deliver {
        let instance=app.try_state::<crate::app::ipc_host::DesktopHost>().and_then(|bridge|bridge.instance().ok());
        let _=app.emit("clipboard-captured",serde_json::json!({"id":id,"host_instance_id":instance}));
    }
}
pub fn start_clipboard_monitor(app:AppHandle) {crate::app::ipc_host::DesktopHost::start(&app);}

pub enum GeneratedCaptureTicket {Local{ticket:dzc_core::api::CaptureTicket,_work:dzc_host::CaptureWork,instance:String},Remote(crate::app::ipc_host::GeneratedUpload)}
/// Acquire authorization before producing data, including before screenshot IO.
pub async fn generated_capture_ticket(app:&AppHandle)->Result<Option<GeneratedCaptureTicket>,String> {
    let bridge=app.state::<crate::app::ipc_host::DesktopHost>();
    if bridge.embedded()?.is_none() {return bridge.begin_generated().await.map(|ticket|ticket.map(GeneratedCaptureTicket::Remote));}
    match app.state::<DbState>().history_service().privacy().capture_ticket(timestamp()) {
        Ok(ticket)=>{let host=bridge.embedded()?.ok_or("ClipboardHostInstanceChanged")?;let work=host.begin_work().map_err(|_|"CaptureWorkUnavailable")?;
            Ok(Some(GeneratedCaptureTicket::Local{ticket,_work:work,instance:bridge.instance()?}))},Err(dzc_core::api::PrivacyError::PrivacySessionActive)=>Ok(None),Err(error)=>Err(error.to_string())
    }
}
fn generated_frame(data:ClipboardData,ticket:Option<dzc_core::api::CaptureTicket>)->Option<CaptureFrame> {
    ticket.map(|ticket|CaptureFrame{data,metadata:CaptureReadMetadata{privacy_ticket:Some(ticket),..Default::default()}})
}
#[cfg(test)]
fn persistence_result(outcome:CaptureOutcome)->Result<Option<StoredCapture>,String> {
    match outcome {CaptureOutcome::Stored(capture)=>Ok(Some(capture)),CaptureOutcome::Skipped(dzc_core::capture::CaptureSkip::PrivacyEpochChanged)=>Err("PrivacyEpochChanged".into()),
        CaptureOutcome::Skipped(_)=>Ok(None),CaptureOutcome::Rejected(reason)=>Err(format!("{reason:?}")),CaptureOutcome::Failed(error)=>Err(error.code)}
}
pub async fn process_new_entry(app:&AppHandle,data:ClipboardData,source_override:Option<String>,source_snapshot:Option<ActiveAppInfo>,ticket:Option<GeneratedCaptureTicket>)->Result<bool,String> {
    process_new_entry_with_protection(app,data,source_override,source_snapshot,ticket,false).await
}
pub async fn process_new_entry_with_protection(app:&AppHandle,data:ClipboardData,source_override:Option<String>,source_snapshot:Option<ActiveAppInfo>,ticket:Option<GeneratedCaptureTicket>,protected:bool)->Result<bool,String> {
    let Some(ticket)=ticket else {return Ok(false);};
    let source=source(source_snapshot,source_override);
    match ticket {
        GeneratedCaptureTicket::Local{ticket,_work,instance}=>{
            if app.state::<crate::app::ipc_host::DesktopHost>().instance()?!=instance {return Err("ClipboardHostInstanceChanged".into());}
            let mut frame=generated_frame(data,Some(ticket)).ok_or("CaptureTicketMissing")?;frame.metadata.inherited_protection=protected;
            app.state::<crate::app::ipc_host::DesktopHost>().embedded()?.ok_or("ClipboardHostInstanceChanged")?.capture_generated(frame,source,timestamp()).map_err(|code|format!("{code:?}"))
        },
        GeneratedCaptureTicket::Remote(upload)=>{
            let body=match data {
                ClipboardData::Text(text)=>dzc_host::wire::GeneratedBody{kind:"text".into(),text,html:None},
                ClipboardData::RichText{text,html}=>dzc_host::wire::GeneratedBody{kind:"html".into(),text,html:Some(html)},
                ClipboardData::Image{data_url}=>dzc_host::wire::GeneratedBody{kind:"image".into(),text:data_url,html:None},
                _=>return Err("GeneratedPayloadKindUnavailable".into()),
            };
            upload.finish(body,source.app_name,protected).await
        }
    }
}

#[cfg(all(test, target_os = "windows", not(feature = "portable")))]
mod tests {
    use super::*;
    use crate::infrastructure::repository::{
        clipboard_repo::SqliteClipboardRepository,
        settings_repo::SqliteSettingsRepository,
        tag_repo::{SqliteTagRepository, TagRepository},
    };
    use dzc_core::privacy::{PrivacyAction, PrivacyReason};
    use dzc_core::capture::CaptureStore;
    use std::collections::VecDeque;

    #[test]
    fn engine_recapture_keeps_manually_protected_text_encrypted_in_the_same_row() {
        dzc_platform::install_default_key_provider();
        let directory =
            std::env::temp_dir().join(format!("dezirclip-capture-dpapi-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let connection = Arc::new(Mutex::new(crate::database::init_db(":memory:").unwrap()));
        let state = DbState {
            repo: SqliteClipboardRepository::new(connection.clone()),
            settings_repo: SqliteSettingsRepository::new(connection.clone()),
            tag_repo: SqliteTagRepository::new(connection.clone()),
            conn: connection,
        };
        let session = Mutex::new(VecDeque::new());
        let mut context = CaptureContext {
            source: CaptureSource {
                app_name: "Editor".into(),
                process_path: None,
            },
            timestamp: 1,
            settings: CaptureSettings {
                persistent: true,
                deduplicate: true,
                ..CaptureSettings::default()
            },
            session: &session,
            paused: false,
            sequential_echo: None,
        };
        let history = state.history_service();
        let store = history.capture_store(&directory);
        let text = "ordinary manually protected text";
        let frame = || CaptureFrame {
            data: ClipboardData::Text(text.into()),
            metadata: CaptureReadMetadata { privacy_ticket: store.capture_ticket(chrono::Utc::now().timestamp_millis()).unwrap(), ..Default::default() },
        };
        let CaptureOutcome::Stored(first) =
            CaptureEngine::capture_frame(&context, frame(), &store, &mut ())
        else {
            panic!("first capture should store");
        };
        state
            .tag_repo
            .update_entry_tags(first.entry.id, vec!["sensitive".into(), "work".into()])
            .unwrap();
        context.timestamp = 2;
        let CaptureOutcome::Stored(second) =
            CaptureEngine::capture_frame(&context, frame(), &store, &mut ())
        else {
            panic!("protected recapture should store");
        };
        assert_eq!(first.entry.id, second.entry.id);
        assert_eq!(second.entry.tags, vec!["sensitive", "work"]);
        assert_eq!(second.privacy.action, PrivacyAction::Encrypt);
        assert_eq!(
            second.privacy.reason,
            PrivacyReason::ExistingRecordProtected
        );
        let connection = state.conn.lock().unwrap();
        let (ciphertext, timestamp): (String, i64) = connection
            .query_row(
                "SELECT content,timestamp FROM clipboard_history WHERE id=?1",
                [first.entry.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!(crate::infrastructure::encryption::has_ciphertext_envelope(
            &ciphertext
        ));
        assert_eq!(
            crate::infrastructure::encryption::decode_for_mutation(&ciphertext).unwrap(),
            text
        );
        assert_eq!(timestamp, 2);
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM clipboard_history", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM clipboard_fts WHERE clipboard_fts MATCH 'protected'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        drop(connection);
        drop(store);
        drop(state);
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn generated_screenshot_and_text_paths_keep_the_start_epoch_and_report_rejection() {
        use dzc_core::api::{HistoryService, PrivacySessionCommand};
        let directory = std::env::temp_dir().join(format!("dzc-generated-ticket-{}", uuid::Uuid::new_v4()));
        let history = HistoryService::open(&directory.join("synthetic.db")).unwrap();
        let store = history.capture_store(&directory);
        let session = Mutex::new(VecDeque::new());
        let context = CaptureContext { source: CaptureSource { app_name: "generated fixture".into(), process_path: None }, timestamp: 1,
            settings: CaptureSettings { persistent: true, ..Default::default() }, session: &session, paused: false, sequential_echo: None };
        let now = chrono::Utc::now().timestamp_millis();
        let data = || [ClipboardData::RgbaImage { width: 1, height: 1, bytes: vec![1, 2, 3, 255] },
            ClipboardData::Text("image converted to base64 synthetic body".into()), ClipboardData::Text("OCR derived synthetic text".into())];
        for value in data() {
            let ticket = history.privacy().capture_ticket(now).unwrap();
            let frame = generated_frame(value, Some(ticket)).unwrap();
            assert!(persistence_result(CaptureEngine::capture_frame(&context, frame, &store, &mut ())).unwrap().is_some());
        }
        let before = history.get_count().unwrap();
        let ticket = history.privacy().capture_ticket(now).unwrap();
        history.privacy().set_session(PrivacySessionCommand::On { duration_ms: None }, now).unwrap();
        assert!(generated_frame(ClipboardData::Text("not recorded".into()), None).is_none());
        history.privacy().set_session(PrivacySessionCommand::Off, now).unwrap();
        for value in data() {
            let frame = generated_frame(value, Some(ticket)).unwrap();
            assert_eq!(persistence_result(CaptureEngine::capture_frame(&context, frame, &store, &mut ())).err().unwrap(), "PrivacyEpochChanged");
        }
        assert_eq!(history.get_count().unwrap(), before);
        assert_eq!(persistence_result(CaptureOutcome::Rejected(dzc_core::capture::CaptureRejection::SensitivePersistenceRejected)).err().unwrap(), "SensitivePersistenceRejected");
        drop(store); drop(history);
        std::fs::remove_dir_all(directory).unwrap();
    }

}
