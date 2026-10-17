use crate::app_state::{AppDataDir, PasteQueue, PasteQueueState, SessionHistory};
use crate::database::DbState;
use crate::error::AppResult;
use crate::infrastructure::repository::clipboard_repo::ClipboardRepository;
use crate::infrastructure::repository::settings_repo::SettingsRepository;
use tauri::{Emitter, Manager, State};

#[allow(dead_code)]
const WM_PASTE: u32 = 0x0302;

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
        // 2. Get Content (DB Lock acquired here, safe because Queue lock is released)
        let content_opt = if id < 0 {
            let s = session.inner().0.lock().unwrap();
            s.iter().find(|i| i.id == id).map(|i| {
                (
                    i.content.clone(),
                    i.content_type.clone(),
                    i.html_content.clone(),
                )
            })
        } else {
            db_state
                .repo
                .get_entry_content_with_html(id)
                .unwrap_or(None)
        };

        if let Some((content, c_type, html_content)) = content_opt {
            // CRITICAL: Update last_pasted_content BEFORE modifying clipboard to prevent race condition
            // where the monitor sees the change before we've marked it as an echo.
            {
                let mut queue = state.inner().0.lock().unwrap();
                queue.last_action_was_paste = true;
                queue.last_pasted_content = Some(content.clone());
            }

            let paste_with_format = c_type == "rich_text" && html_content.as_deref().is_some();

            if crate::services::clipboard_ops::write_content_to_system_clipboard(
                &content,
                crate::services::clipboard_ops::ClipboardWriteOptions {
                    content_type: &c_type,
                    html_content: html_content.as_deref(),
                    paste_with_format,
                    mark_as_self_copy: true,
                },
            )
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
