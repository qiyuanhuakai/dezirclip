use crate::app_state::{PasteQueue, PasteQueueState};
use crate::database::DbState;
use crate::error::AppResult;
use crate::infrastructure::repository::settings_repo::SettingsRepository;
use tauri::{Emitter, Manager, State};

#[allow(dead_code)]
const WM_PASTE: u32 = 0x0302;

#[derive(Debug)]
struct PreparedQueueEntry {
    entry: dzc_core::api::ClipboardEntry,
    permit: dzc_core::api::PrivacyReadPermit,
}

fn prepare_queue_entry(history:&dzc_core::api::HistoryService,id:i64,now:i64)->Result<Option<PreparedQueueEntry>,dzc_core::api::PrivacyError> {
    if id<=0 {return Err(dzc_core::api::PrivacyError::SensitiveAccessDenied);}
    let permit=history.privacy().authorize_selected_copy(id,None,
        dzc_core::api::SensitiveAccess::Denied,None,now)?;
    let entry=history.get_entry_authorized(id,&permit,now)?;
    if entry.is_none() {
        let metadata=history.query(&dzc_core::api::HistoryQuery{id:Some(id),limit:1,metadata:true,..Default::default()})
            .map_err(|_|dzc_core::api::PrivacyError::PrivacyStateUnavailable)?;
        history.privacy().check_read(&permit,now)?;
        if !metadata.is_empty() {return Err(dzc_core::api::PrivacyError::SensitiveAccessDenied);}
    }
    Ok(entry.map(|entry|PreparedQueueEntry{entry,permit}))
}

fn reset_paste_tracking(queue: &mut PasteQueueState) {
    queue.reset_tracking();
}

fn emit_queue_finished(app_handle: &tauri::AppHandle, state: &State<'_, PasteQueue>) {
    let empty={let mut queue=state.0.lock().unwrap();if queue.items.is_empty() {reset_paste_tracking(&mut queue);true}else{false}};
    if empty {let _ = app_handle.emit("queue-finished", ());}
}

#[tauri::command]
pub fn get_paste_queue(state: State<'_, PasteQueue>) -> Vec<i64> {
    state
        .inner()
        .0
        .lock()
        .unwrap()
        .items
        .iter()
        .copied()
        .collect()
}

#[tauri::command]
pub fn set_paste_queue(
    app_handle: tauri::AppHandle,
    state: State<'_, PasteQueue>,
    item_ids: Vec<i64>,
) -> AppResult<()> {
    if item_ids.iter().any(|id|*id<=0) {
        return Err(crate::error::AppError::Validation("SessionQueueUnavailable".into()));
    }
    if item_ids.is_empty() {
        {
            let mut queue = state.inner().0.lock().unwrap();
            queue.items.clear();
            reset_paste_tracking(&mut queue);
        }
        let _ = app_handle.emit("queue-finished", ());
        return Ok(());
    }

    let queued = {
        let mut queue = state.inner().0.lock().unwrap();
        queue.items.clear();
        for id in item_ids {
            queue.items.push_back(id);
        }
        reset_paste_tracking(&mut queue);
        queue.items.len()
    };
    // Replacing the queue outright used to be silent, so the count kept showing
    // whatever the previous batch left behind. Same signal the capture path
    // emits, carrying the length so the frontend adopts it without a round trip.
    let _ = app_handle.emit("paste-queue-changed", queued);

    Ok(())
}

/// Drop everything queued and forget the paste tracking that decides when the
/// next capture starts a new sequence.
///
/// Turning the setting off used to leave the queue intact, and the hotkey
/// accepts a non-empty queue even when the mode is off (`setup.rs`: `is_seq ||
/// has_items`), so a batch built before the switch stayed pasteable. Clearing
/// it here is what makes "off" mean off; the settings switch calls this when
/// it is turned off, and the queue row calls it to abandon a batch by hand.
#[tauri::command]
pub fn clear_paste_queue(app_handle: tauri::AppHandle, state: State<'_, PasteQueue>) {
    {
        let mut queue = state.inner().0.lock().unwrap();
        queue.items.clear();
        reset_paste_tracking(&mut queue);
    }
    let _ = app_handle.emit("queue-finished", ());
}


#[derive(Clone)]
struct QueueStep { id:i64, version:std::sync::Arc<()>, token:std::sync::Arc<()> }
fn reserve_step(queue:&mut PasteQueueState)->Result<Option<QueueStep>, &'static str> {
    if queue.in_flight.is_some() {return Err("PasteQueueBusy");}
    let Some(id)=queue.items.front().copied() else {return Ok(None);};
    let token=std::sync::Arc::new(());
    queue.in_flight=Some(token.clone());
    Ok(Some(QueueStep{id,version:queue.version.clone(),token}))
}
fn current_step(queue:&PasteQueueState,step:&QueueStep)->bool {
    std::sync::Arc::ptr_eq(&queue.version,&step.version)
        && queue.in_flight.as_ref().is_some_and(|token|std::sync::Arc::ptr_eq(token,&step.token))
        && queue.items.front()==Some(&step.id)
}
fn consume_step(queue:&mut PasteQueueState,step:&QueueStep,missing:bool)->Option<bool> {
    if !current_step(queue,step) {return None;}
    queue.items.pop_front();
    if missing {reset_paste_tracking(queue);}
    Some(queue.items.is_empty())
}
fn release_step(queue:&mut PasteQueueState,step:&QueueStep) {
    if queue.in_flight.as_ref().is_some_and(|token|std::sync::Arc::ptr_eq(token,&step.token)) {queue.in_flight=None;}
}
struct QueueStepGuard {queue:std::sync::Arc<std::sync::Mutex<PasteQueueState>>, step:QueueStep}
impl QueueStepGuard {
    fn same_version(&self)->bool {
        self.queue.lock().map(|queue|std::sync::Arc::ptr_eq(&queue.version,&self.step.version)).unwrap_or(false)
    }
    fn current(&self)->bool {
        self.queue.lock().map(|queue|current_step(&queue,&self.step)).unwrap_or(false)
    }
    fn consume(&self,missing:bool)->Option<bool> {
        self.queue.lock().ok().and_then(|mut queue|consume_step(&mut queue,&self.step,missing))
    }
    fn accept_keystroke(&self,result:Result<(),String>)->Result<Option<bool>,String> {
        result.map(|_|self.consume(false))
    }
    fn skip_missing(&self,app:&tauri::AppHandle) {
        if let Ok(mut queue)=self.queue.lock() {
            if let Some(length)=skip_missing_step(&mut queue,&self.step) {
                let _=app.emit("paste-queue-changed",length);
                if length==0 {let _=app.emit("queue-finished",());}
            }
        }
    }
    fn track(&self,content:Option<String>) {
        if let Ok(mut queue)=self.queue.lock() {
            if current_step(&queue,&self.step) {queue.last_action_was_paste=true;queue.last_pasted_content=content;}
        }
    }
    fn fail(&self,app:&tauri::AppHandle,error:&str) {
        let pending=self.queue.lock().map(|mut queue| {
            if !current_step(&queue,&self.step) {return false;}
            reset_paste_tracking(&mut queue);true
        }).unwrap_or(false);
        if pending {let _=app.emit("paste-queue-unavailable",error);}
    }
}
fn skip_missing_step(queue:&mut PasteQueueState,step:&QueueStep)->Option<usize> {
    consume_step(queue,step,true).map(|_|queue.items.len())
}
impl Drop for QueueStepGuard {
    fn drop(&mut self) {
        if let Ok(mut queue)=self.queue.lock() {release_step(&mut queue,&self.step);}
    }
}
fn finish_pasted(app:&tauri::AppHandle,guard:&QueueStepGuard,empty:Option<bool>,completion:Result<(),String>,delete:bool) {
    let id=guard.step.id;
    if let Err(error)=completion {
        let _=app.emit("paste-queue-unavailable",format!("PasteCompletedMetadataUpdateFailed:{error}"));
    } else if delete {let _=app.emit("clipboard-removed",id);}
    if guard.same_version() {
        let _=app.emit("queue-item-pasted",id);
        if empty==Some(true) {emit_queue_finished(app,&app.state::<PasteQueue>());}
    }
}

fn send_queue_input(
    validate:impl FnOnce()->Result<(),String>,
    send:impl FnOnce()->Result<(),String>,
    restore:impl FnOnce(),
)->Result<(),String> {
    validate()?;
    let result=send();
    restore();
    result
}

fn send_queue_keystroke(method:&str,content:Option<&str>,content_type:&str,validate:impl FnOnce()->Result<(),String>)->Result<(),String> {
            let alt_was_down = {
                #[cfg(target_os = "windows")]
                unsafe {
                    (windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(0x12) as i16) < 0
                }
                #[cfg(not(target_os = "windows"))]
                false
            };


    send_queue_input(validate,|| {
        crate::services::clipboard_ops::send_paste_keystroke(method,content,Some(content_type)).map_err(|error|format!("PasteKeystrokeFailed:{error}"))
    },|| {
            // Settle time
            std::thread::sleep(std::time::Duration::from_millis(20));

            // Restore Alt state if it was physically held down
            // This is crucial for sequential paste flow (holding Alt while tapping V)
            if alt_was_down {
                #[cfg(target_os = "windows")]
                unsafe {
                    use windows::Win32::UI::Input::KeyboardAndMouse::{
                        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, VK_MENU,
                    };
                    let alt_restore = INPUT {
                        r#type: INPUT_KEYBOARD,
                        Anonymous: INPUT_0 {
                            ki: KEYBDINPUT {
                                wVk: VK_MENU,
                                dwFlags:
                                    windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS(
                                        0,
                                    ),
                                ..Default::default()
                            },
                        },
                    };
                    SendInput(&[alt_restore], std::mem::size_of::<INPUT>() as i32);
                    println!("[DEBUG] Restored Alt key state for continuous sequential paste");
                }
            }

    })
}

async fn paste_service_step(app:tauri::AppHandle,bridge:crate::app::ipc_host::DesktopHost,guard:QueueStepGuard,instance:String,epoch:u64,method:String,delete:bool) {
    let id=guard.step.id;
    let mut prepared=match bridge.prepare_service_queue(id,&instance,epoch).await {
        Ok(prepared)=>prepared,
        Err(error) if error=="ClipboardRecordUnavailable"=>{
            guard.skip_missing(&app);
            return;
        },
        Err(error)=>{guard.fail(&app,&error);return;},
    };
    if !guard.current() {return;}
    guard.track(prepared.content.clone());
    if let Err(error)=prepared.copy().await {guard.fail(&app,&error);return;}
    if let Err(error)=prepared.check(&bridge).await {guard.fail(&app,&error);return;}
    let result=send_queue_keystroke(&method,prepared.content.as_deref(),&prepared.content_type,|| {
        if !guard.current() {return Err("PasteQueueChanged".into());}
        bridge.check_queue_state(&instance,epoch)?;
        Ok(())
    });
    let empty=match guard.accept_keystroke(result) {Ok(empty)=>empty,Err(error)=>{guard.fail(&app,&error);return;}};
    let completed=prepared.complete(delete).await;
    finish_pasted(&app,&guard,empty,completed,delete);
}

#[tauri::command]
pub fn paste_next_step(app_handle:tauri::AppHandle) {
    let state=app_handle.state::<PasteQueue>();
    let reserved={let mut queue=state.0.lock().unwrap();reserve_step(&mut queue)};
    let step=match reserved {
        Ok(Some(step))=>step,
        Ok(None)=>{let _=app_handle.emit("paste-queue-empty",());emit_queue_finished(&app_handle,&state);return;},
        Err(error)=>{let _=app_handle.emit("paste-queue-unavailable",error);return;},
    };
    let guard=QueueStepGuard{queue:state.0.clone(),step};
    let id=guard.step.id;
    if id<=0 {guard.fail(&app_handle,"SessionQueueUnavailable");return;}
    let bridge=app_handle.state::<crate::app::ipc_host::DesktopHost>().inner().clone();
    let (instance,epoch)=match bridge.queue_state() {Ok(state)=>state,Err(error)=>{guard.fail(&app_handle,&error);return;}};
    let db=app_handle.state::<DbState>();
    let method=db.settings_repo.get("app.paste_method").ok().flatten().unwrap_or_else(||"shift_insert".into());
    let delete=app_handle.state::<crate::app_state::SettingsState>().delete_after_paste.load(std::sync::atomic::Ordering::Relaxed);
    match bridge.embedded() {
        Err(error)=>{guard.fail(&app_handle,&error);return;},
        Ok(None)=>{
            tauri::async_runtime::spawn(paste_service_step(app_handle.clone(),bridge,guard,instance,epoch,method,delete));
            return;
        },
        Ok(Some(_))=>(),
    }
    let history=db.history_service();
    let prepared=match prepare_queue_entry(&history,id,chrono::Utc::now().timestamp_millis()) {
        Ok(Some(prepared))=>prepared,
        Ok(None)=>{
            guard.skip_missing(&app_handle);
            return;
        },
        Err(_)=>{guard.fail(&app_handle,"SensitiveAccessDenied");return;},
    };
    if !guard.current() {return;}
    let PreparedQueueEntry{entry,permit}=prepared;
    if permit.epoch()!=epoch {guard.fail(&app_handle,"PrivacyEpochChanged");return;}
    if let Err(error)=bridge.check_queue_state(&instance,epoch) {guard.fail(&app_handle,&error);return;}
    guard.track(Some(entry.content.clone()));
    let copied=bridge.call_embedded::<dzc_host::wire::OperationResult>(dzc_host::wire::Method::DesktopClipboardCopy,dzc_host::wire::DesktopCopyParams{
        id,service_instance_id:instance.clone(),expected_epoch:permit.epoch(),reveal:false,paste:false,derived_text:None,image_as_base64:None,
        with_format:Some(entry.content_type=="rich_text" && entry.html_content.is_some()),delete_after_use:Some(false),move_to_top:Some(false)});
    match copied {
        Ok(result) if result.performed=>(),
        Ok(_)=>{guard.fail(&app_handle,"ClipboardCopyFailed");return;},
        Err(error)=>{guard.fail(&app_handle,&error);return;},
    }
    let pasted=send_queue_keystroke(&method,Some(&entry.content),&entry.content_type,|| {
        if !guard.current() {return Err("PasteQueueChanged".into());}
        if bridge.instance()?!=instance {return Err("ClipboardHostInstanceChanged".into());}
        history.privacy().check_read(&permit,chrono::Utc::now().timestamp_millis()).map_err(|error|error.to_string())
    });
    let empty=match guard.accept_keystroke(pasted) {Ok(empty)=>empty,Err(error)=>{guard.fail(&app_handle,&error);return;}};
    let completed=bridge.complete_embedded_queue(id,&instance,permit.epoch(),delete);
    finish_pasted(&app_handle,&guard,empty,completed,delete);
}
#[cfg(test)]
mod authorization_tests {
    use super::*;
    use std::sync::{Arc,Mutex};
    #[test]
    fn held_alt_is_restored_after_input_failure_without_masking_the_error_or_consuming() {
        use std::cell::{Cell,RefCell};
        let held=Cell::new(true);let order=RefCell::new(Vec::new());
        let original="PasteKeystrokeFailed:accepted 1 of 2 events".to_owned();
        let result=send_queue_input(
            ||{order.borrow_mut().push("validate");Ok(())},
            ||{order.borrow_mut().push("send");held.set(false);Err(original.clone())},
            ||{order.borrow_mut().push("restore");held.set(true)},
        );
        assert_eq!(result,Err(original));
        assert!(held.get());
        assert_eq!(*order.borrow(),vec!["validate","send","restore"]);
        let shared=Arc::new(Mutex::new(PasteQueueState::default()));shared.lock().unwrap().items.push_back(17);
        let step=reserve_step(&mut shared.lock().unwrap()).unwrap().unwrap();
        let guard=QueueStepGuard{queue:shared.clone(),step};
        assert!(guard.accept_keystroke(result).is_err());
        assert_eq!(shared.lock().unwrap().items.front(),Some(&17));
        drop(guard);assert!(shared.lock().unwrap().in_flight.is_none());
        order.borrow_mut().clear();
        let denied=send_queue_input(||Err("PrivacyEpochChanged".into()),||{order.borrow_mut().push("send");Ok(())},||{order.borrow_mut().push("restore");});
        assert_eq!(denied,Err("PrivacyEpochChanged".into()));
        assert!(order.borrow().is_empty(),"validation failure must not inject or restore keys");
        assert_eq!(send_queue_input(||Ok(()),||Ok(()),||{order.borrow_mut().push("restore");}),Ok(()));
        assert_eq!(*order.borrow(),vec!["restore"]);
    }
    #[test]
    fn rejected_windows_input_keeps_the_queue_and_drop_releases_the_original_reservation() {
        use crate::infrastructure::paste_input::{self,Input,InputDriver};
        struct Rejected { call:usize,fail:usize,accepted:usize }
        impl InputDriver for Rejected {
            fn send(&mut self,inputs:&[Input])->usize {let call=self.call;self.call+=1;if call==self.fail {self.accepted}else{inputs.len()}}
            fn pause(&mut self,_:u64) {}
            fn key_is_down(&self,_:u16)->bool {false}
            fn begin_text(&mut self)->bool {true}
            fn same_target(&self)->bool {true}
            fn end_text(&mut self) {}
        }
        for (method,text,fail,accepted) in [
            ("ctrl_v",None,0,4),("ctrl_v",None,1,0),("ctrl_v",None,1,1),("ctrl_v",None,2,1),
            ("shift_insert",None,2,0),("game_mode",None,2,0),("game_mode",Some("中😀\r\n"),7,0),
        ] {
            let shared=Arc::new(Mutex::new(PasteQueueState::default()));
            shared.lock().unwrap().items.extend([17,18]);
            let step=reserve_step(&mut shared.lock().unwrap()).unwrap().unwrap();
            {
                let guard=QueueStepGuard{queue:shared.clone(),step};
                let result=paste_input::send_paste(&mut Rejected{call:0,fail,accepted},method,text,Some("text")).map_err(|error|error.to_string());
                let mut completions=0;
                let accepted=guard.accept_keystroke(result).map(|_|{completions+=1;});
                assert!(accepted.is_err(),"{method} failed input must not acknowledge a queue item");
                assert_eq!(completions,0,"neither service nor embedded completion may delete/count after failed input");
                assert!(guard.current());
                assert_eq!(shared.lock().unwrap().items.iter().copied().collect::<Vec<_>>(),vec![17,18]);
            }
            assert!(shared.lock().unwrap().in_flight.is_none());
            assert_eq!(reserve_step(&mut shared.lock().unwrap()).unwrap().unwrap().id,17);
        }
    }
#[test]
    fn dropping_a_cancelled_async_step_releases_its_reservation_without_consuming() {
        let shared=Arc::new(Mutex::new(PasteQueueState::default()));
        shared.lock().unwrap().items.push_back(1);
        let step=reserve_step(&mut shared.lock().unwrap()).unwrap().unwrap();
        {
            let pending=QueueStepGuard{queue:shared.clone(),step};
            assert!(reserve_step(&mut shared.lock().unwrap()).is_err());
            reset_paste_tracking(&mut shared.lock().unwrap());
            assert!(!pending.current());
        }
        let mut queue=shared.lock().unwrap();
        assert_eq!(queue.items.front(),Some(&1));
        assert!(queue.in_flight.is_none());
        let next=reserve_step(&mut queue).unwrap().unwrap();
        drop(queue);
        let pending=QueueStepGuard{queue:shared.clone(),step:next.clone()};
        release_step(&mut shared.lock().unwrap(),&next);
        let replacement=reserve_step(&mut shared.lock().unwrap()).unwrap().unwrap();
        drop(pending);
        assert!(current_step(&shared.lock().unwrap(),&replacement),"a stale Drop must not unlock a newer step");
        release_step(&mut shared.lock().unwrap(),&replacement);
    }
#[test]
    fn service_step_reservation_blocks_duplicate_work_and_failure_keeps_the_front() {
        let mut queue=PasteQueueState::default();queue.items.extend([1,2]);
        let first=reserve_step(&mut queue).unwrap().unwrap();
        assert!(reserve_step(&mut queue).is_err());
        release_step(&mut queue,&first);
        assert_eq!(queue.items.iter().copied().collect::<Vec<_>>(),vec![1,2]);
        let retry=reserve_step(&mut queue).unwrap().unwrap();
        assert!(!std::sync::Arc::ptr_eq(&first.token,&retry.token));
        release_step(&mut queue,&first);
        assert!(reserve_step(&mut queue).is_err(),"old cleanup must not release the current reservation");
        assert_eq!(consume_step(&mut queue,&retry,false),Some(false));
        assert_eq!(consume_step(&mut queue,&retry,false),None,"a pasted item is consumed only once");
        release_step(&mut queue,&retry);
        assert_eq!(reserve_step(&mut queue).unwrap().unwrap().id,2);
    }
    #[test]
    fn replaced_or_cleared_queue_rejects_old_completion_even_for_the_same_id() {
        let mut queue=PasteQueueState::default();queue.items.push_back(1);
        let old=reserve_step(&mut queue).unwrap().unwrap();
        queue.items.clear();queue.items.push_back(1);reset_paste_tracking(&mut queue);
        assert!(!current_step(&queue,&old));
        assert_eq!(consume_step(&mut queue,&old,false),None);
        assert!(reserve_step(&mut queue).is_err(),"replacement must not overlap the pending cleanup");
        release_step(&mut queue,&old);
        let current=reserve_step(&mut queue).unwrap().unwrap();
        queue.items.clear();reset_paste_tracking(&mut queue);
        assert_eq!(consume_step(&mut queue,&current,false),None);
        release_step(&mut queue,&current);
        assert!(reserve_step(&mut queue).unwrap().is_none());
    }
    #[test]
    fn missing_step_can_be_skipped_without_releasing_another_pending_operation() {
        let mut queue=PasteQueueState::default();queue.items.extend([1,2]);
        let step=reserve_step(&mut queue).unwrap().unwrap();
        assert_eq!(skip_missing_step(&mut queue,&step),Some(1));
        assert_eq!(skip_missing_step(&mut queue,&step),None,"stale skips cannot emit a replacement length");
        assert_eq!(queue.items.front(),Some(&2));
        assert!(!queue.last_action_was_paste);
        assert!(reserve_step(&mut queue).is_err());
        release_step(&mut queue,&step);
        let next=reserve_step(&mut queue).unwrap().unwrap();
        assert_eq!(next.id,2);
        assert_eq!(skip_missing_step(&mut queue,&next),Some(0));
    }

    fn history(protected:bool,kind:&str)->dzc_core::api::HistoryService {
        let connection=crate::database::init_db(":memory:").unwrap();
        connection.execute("INSERT INTO clipboard_history(id,content_type,content,preview,tags,source_app,timestamp,is_pinned) VALUES(1,?1,?2,'synthetic preview',?3,'Synthetic',1,0)",
            rusqlite::params![kind,"synthetic queue body",if protected {"[\"sensitive\"]"} else {"[]"}]).unwrap();
        dzc_core::api::HistoryService::from_legacy_connection(Arc::new(Mutex::new(connection)))
    }

    #[test]
    fn queue_cannot_decrypt_protected_text_html_images_or_files_without_a_user_read_grant() {
        for kind in ["text","rich_text","image","files","video"] {
            let history=history(true,kind);
            assert_eq!(history.get_entry_content(1).unwrap().as_deref(),Some("synthetic queue body"));
            assert_eq!(prepare_queue_entry(&history,1,10).unwrap_err(),dzc_core::api::PrivacyError::SensitiveAccessDenied,"{kind}");
        }
    }

    #[test]
    fn ordinary_queue_reads_keep_the_original_epoch_and_work_while_recording_is_paused() {
        let history=history(false,"text");
        let before=prepare_queue_entry(&history,1,10).unwrap().unwrap();
        history.privacy().toggle(11).unwrap();
        assert_eq!(history.privacy().check_read(&before.permit,12).unwrap_err(),dzc_core::api::PrivacyError::PrivacyEpochChanged);
        let active=prepare_queue_entry(&history,1,12).unwrap().unwrap();
        assert_eq!(active.entry.content,"synthetic queue body");
        assert!(history.privacy().check_read(&active.permit,12).is_ok());
        assert!(history.privacy().capture_ticket(12).is_err());
        assert!(prepare_queue_entry(&history,2,12).unwrap().is_none());
        assert!(prepare_queue_entry(&history,-1,12).is_err());
    }
}
