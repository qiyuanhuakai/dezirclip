use crate::app_state::{AppDataDir, PasteQueue, PasteQueueState, SessionHistory};
use crate::database::DbState;
use crate::error::AppResult;
use crate::infrastructure::repository::clipboard_repo::ClipboardRepository;
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
    let permit=history.privacy().authorize_read(dzc_core::api::PrivacyAccessIntent::SelectedCopy,
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
    queue.last_action_was_paste = false;
    queue.last_pasted_content = None;
}

fn clear_paste_tracking(state: &State<'_, PasteQueue>) {
    let mut queue = state.inner().0.lock().unwrap();
    reset_paste_tracking(&mut queue);
}

fn pop_front_if_matches(state: &State<'_, PasteQueue>, id: i64) -> bool {
    let mut queue = state.inner().0.lock().unwrap();
    if queue.items.front().copied() == Some(id) {
        queue.items.pop_front();
    }
    queue.items.is_empty()
}

fn emit_queue_finished(app_handle: &tauri::AppHandle, state: &State<'_, PasteQueue>) {
    clear_paste_tracking(state);
    let _ = app_handle.emit("queue-finished", ());
}

fn emit_queue_finished_if_empty(app_handle: &tauri::AppHandle, state: &State<'_, PasteQueue>) {
    let should_emit = {
        let mut queue = state.inner().0.lock().unwrap();
        if queue.items.is_empty() {
            reset_paste_tracking(&mut queue);
            true
        } else {
            false
        }
    };

    if should_emit {
        let _ = app_handle.emit("queue-finished", ());
    }
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

#[tauri::command]
pub fn paste_next_step(app_handle: tauri::AppHandle) {
    let state = app_handle.state::<PasteQueue>();
    let db_state = app_handle.state::<DbState>();
    let session = app_handle.state::<SessionHistory>();
    let _settings = app_handle.state::<crate::app_state::SettingsState>();

    // Peek first so a failed paste keystroke does not lose the queued item.
    let id_opt = {
        let queue = state.inner().0.lock().unwrap();
        queue.items.front().copied()
    };

    if let Some(id) = id_opt {
        if id<=0 {
            let _=app_handle.emit("paste-queue-unavailable","SessionQueueUnavailable");
            return;
        }
        let bridge=app_handle.state::<crate::app::ipc_host::DesktopHost>();
        match bridge.embedded() {
            Err(_)=>{let _=app_handle.emit("paste-queue-unavailable","ClipboardHostUnavailable");return;},
            Ok(None)=>{
                let bridge=bridge.inner().clone();let app=app_handle.clone();
                tauri::async_runtime::spawn(async move {
                    let result=async {
                        let privacy:dzc_host::wire::PrivacyState=bridge.call(dzc_host::wire::Method::PrivacyGet,serde_json::json!({}),"PrivacyState").await?;
                        bridge.call::<dzc_host::wire::OperationResult>(dzc_host::wire::Method::DesktopClipboardCopy,dzc_host::wire::DesktopCopyParams{
                            id,service_instance_id:bridge.instance()?,expected_epoch:privacy.privacy_epoch,reveal:false,paste:true,
                            derived_text:None,image_as_base64:None,with_format:Some(true),delete_after_use:Some(false),move_to_top:Some(false)},"OperationResult").await
                    }.await;
                    if result.is_ok() {
                        let queue=app.state::<PasteQueue>();
                        if pop_front_if_matches(&queue,id) {emit_queue_finished(&app,&queue);}
                    }else {let _=app.emit("paste-queue-unavailable","ClipboardHostUnavailable");}
                });
                return;
            },
            Ok(Some(_))=>(),
        }
        let original_instance=match bridge.instance() {
            Ok(instance)=>instance,
            Err(_)=>{let _=app_handle.emit("paste-queue-unavailable","ClipboardHostUnavailable");return;},
        };
        let history=db_state.history_service();
        let content_opt=match prepare_queue_entry(&history,id,chrono::Utc::now().timestamp_millis()) {
            Ok(entry)=>entry,
            Err(_)=>{clear_paste_tracking(&state);let _=app_handle.emit("paste-queue-unavailable","SensitiveAccessDenied");return;},
        };
        if let Some(PreparedQueueEntry{entry,permit}) = content_opt {
            let (content,c_type,html_content)=(entry.content,entry.content_type,entry.html_content);
            // CRITICAL: Update last_pasted_content BEFORE modifying clipboard to prevent race condition
            // where the monitor sees the change before we've marked it as an echo.
            {
                let mut queue = state.inner().0.lock().unwrap();
                queue.last_action_was_paste = true;
                queue.last_pasted_content = Some(content.clone());
            }

            let paste_with_format = c_type == "rich_text" && html_content.as_deref().is_some();

            if bridge.call_embedded::<dzc_host::wire::OperationResult>(dzc_host::wire::Method::DesktopClipboardCopy,
                dzc_host::wire::DesktopCopyParams{id,service_instance_id:original_instance.clone(),expected_epoch:permit.epoch(),reveal:false,paste:false,
                    derived_text:None,image_as_base64:None,with_format:Some(paste_with_format),delete_after_use:Some(false),move_to_top:Some(false)})
                .is_err()
            {
                clear_paste_tracking(&state);
                return;
            }

            // Get paste method from settings
            let paste_method = {
                let db_state = app_handle.state::<DbState>();
                db_state
                    .settings_repo
                    .get("app.paste_method")
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| "shift_insert".to_string())
            };

            // Send paste keystroke using centralized logic
            // We measure Alt state BEFORE sending keys to know if we should restore it
            let alt_was_down = {
                #[cfg(target_os = "windows")]
                unsafe {
                    (windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(0x12) as i16) < 0
                }
                #[cfg(not(target_os = "windows"))]
                false
            };

            if bridge.instance().ok().as_deref()!=Some(original_instance.as_str())
                || history.privacy().check_read(&permit,chrono::Utc::now().timestamp_millis()).is_err() {
                clear_paste_tracking(&state);
                let _=app_handle.emit("paste-queue-unavailable","PrivacyEpochChanged");
                return;
            }

            if crate::services::clipboard_ops::send_paste_keystroke(
                &paste_method,
                Some(&content),
                Some(&c_type),
            )
            .is_err()
            {
                clear_paste_tracking(&state);
                return;
            }

            let queue_became_empty = pop_front_if_matches(&state, id);

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

            // Perform deletion if delete_after_paste is enabled
            let delete_after_paste = {
                let settings_state = app_handle.state::<crate::app_state::SettingsState>();
                settings_state
                    .delete_after_paste
                    .load(std::sync::atomic::Ordering::Relaxed)
            };

            if delete_after_paste {
                // Remove from session history first
                {
                    let mut s = session.inner().0.lock().unwrap();
                    if let Some(pos) = s.iter().position(|i| i.id == id) {
                        s.remove(pos);
                    }
                }

                if id > 0 {
                    // Persistent item: Delete from DB (and cleanup file)
                    let app_data = app_handle.state::<AppDataDir>();
                    let data_dir = app_data.0.lock().unwrap();
                    if db_state.repo.delete(id, Some(&data_dir)).is_ok() {
                        let _ = app_handle.emit("clipboard-removed", id);
                    }
                } else {
                    // Session item only
                    let _ = app_handle.emit("clipboard-removed", id);
                }
            } else {
                // If not deleting, increment use count
                if id > 0 {
                    let _ = db_state.repo.increment_use_count(id);
                }
            }

            // Emit event to update UI queue state
            let _ = app_handle.emit("queue-item-pasted", id);
            if queue_became_empty {
                emit_queue_finished(&app_handle, &state);
            }
        } else {
            let queue_became_empty = pop_front_if_matches(&state, id);
            clear_paste_tracking(&state);
            if queue_became_empty {
                emit_queue_finished_if_empty(&app_handle, &state);
            }
        }
    } else {
        // The hotkey fired with nothing queued. `queue-finished` alone says
        // "the queue is now empty", which the UI cannot tell apart from a
        // batch that just ran dry -- so the press would look like it did
        // nothing at all. This event names the cause.
        let _ = app_handle.emit("paste-queue-empty", ());
        emit_queue_finished(&app_handle, &state);
    }
}

#[cfg(test)]
mod authorization_tests {
    use super::*;
    use std::sync::{Arc,Mutex};

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
