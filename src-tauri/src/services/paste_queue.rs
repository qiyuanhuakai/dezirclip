use crate::app::ipc_host::DesktopHost;
use dzc_host::wire::*;
use tauri::{AppHandle, Emitter, Manager, State};

async fn queue_state(host: &DesktopHost) -> Result<(RuntimeState, QueueState), String> {
    let state: RuntimeState = host
        .call(Method::Health, serde_json::json!({}), "RuntimeState")
        .await?;
    let queue = host
        .call(
            Method::DesktopQueueGet,
            LibraryStateParams {
                service_instance_id: state.service_instance_id.clone(),
                expected_epoch: state.privacy_epoch,
            },
            "QueueState",
        )
        .await?;
    Ok((state, queue))
}

#[tauri::command]
pub async fn get_paste_queue(host: State<'_, DesktopHost>) -> Result<Vec<i64>, String> {
    Ok(queue_state(&host).await?.1.ids)
}

#[tauri::command]
pub async fn set_paste_queue(app_handle: AppHandle, item_ids: Vec<i64>) -> Result<(), String> {
    let host = app_handle.state::<DesktopHost>();
    let state: RuntimeState = host
        .call(Method::Health, serde_json::json!({}), "RuntimeState")
        .await?;
    // This legacy command has no originating Host ID for RAM-only selections.
    if item_ids.iter().any(|id| *id <= 0) {
        return Err("SessionQueueUnavailable".into());
    }
    let empty = item_ids.is_empty();
    let _: OperationResult = host
        .call(
            Method::DesktopQueueSet,
            QueueReplaceParams {
                service_instance_id: state.service_instance_id,
                expected_epoch: state.privacy_epoch,
                ids: item_ids,
            },
            "OperationResult",
        )
        .await?;
    if empty {
        let _ = app_handle.emit("queue-finished", ());
    }
    Ok(())
}

#[tauri::command]
pub async fn clear_paste_queue(app_handle: AppHandle) -> Result<(), String> {
    set_paste_queue(app_handle, vec![]).await
}

async fn next(
    app: &AppHandle,
    target: Result<QueueTarget, String>,
    explicit: bool,
) -> Result<(), String> {
    let host = app.state::<DesktopHost>();
    let (state, queue) = queue_state(&host).await?;
    if queue.ids.is_empty() {
        if explicit || queue.preferences.sequential {
            let _ = app.emit("paste-queue-empty", ());
            let _ = app.emit("queue-finished", ());
        }
        return Ok(());
    }
    let result: QueueStepResult = host
        .blocking_call(
            Method::DesktopQueueNext,
            QueueNextParams {
                service_instance_id: state.service_instance_id,
                expected_epoch: state.privacy_epoch,
                expected_revision: queue.revision,
                target: target?,
            },
            "QueueStepResult",
        )
        .await?;
    if result.status == "metadata_failed" {
        let _ = app.emit(
            "paste-queue-unavailable",
            "PasteCompletedMetadataUpdateFailed",
        );
    }
    if result.status == "pasted" || result.status == "metadata_failed" {
        let _ = app.emit("queue-item-pasted", result.id);
        if queue.preferences.delete_after_paste && result.status == "pasted" {
            let _ = app.emit("clipboard-removed", result.id);
        }
    }
    if result.remaining == 0 {
        let _ = app.emit("queue-finished", ());
    }
    Ok(())
}

pub(crate) fn start_step(app: AppHandle, explicit: bool) {
    // Freeze the destination at hotkey dispatch, before scheduling any IPC.
    let target = dzc_platform::runtime::QueueInputTarget::foreground()
        .map_err(|_| "PasteTargetUnavailable".to_owned());
    tauri::async_runtime::spawn(async move {
        if let Err(error) = next(&app, target, explicit).await {
            let _ = app.emit("paste-queue-unavailable", error);
        }
    });
}

#[tauri::command]
pub fn paste_next_step(app_handle: AppHandle) {
    start_step(app_handle, true);
}

#[cfg(test)]
mod authorization_tests {
    use std::sync::{Arc, Mutex};
    fn history(protected: bool, kind: &str) -> dzc_core::api::HistoryService {
        let connection = crate::database::init_db(":memory:").unwrap();
        connection.execute("INSERT INTO clipboard_history(id,content_type,content,preview,tags,source_app,timestamp,is_pinned) VALUES(1,?1,?2,'synthetic preview',?3,'Synthetic',1,0)",
            rusqlite::params![kind,"synthetic queue body",if protected {"[\"sensitive\"]"} else {"[]"}]).unwrap();
        dzc_core::api::HistoryService::from_legacy_connection(Arc::new(Mutex::new(connection)))
    }

    #[test]
    fn queue_cannot_decrypt_protected_text_html_images_or_files_without_a_user_read_grant() {
        for kind in ["text", "rich_text", "image", "files", "video"] {
            let history = history(true, kind);
            assert_eq!(
                history.get_entry_content(1).unwrap().as_deref(),
                Some("synthetic queue body")
            );
            assert_eq!(
                history.prepare_queue_entry(1, None, 10).unwrap_err(),
                dzc_core::api::PrivacyError::SensitiveAccessDenied,
                "{kind}"
            );
        }
    }

    #[test]
    fn ordinary_queue_reads_keep_the_original_epoch_and_work_while_recording_is_paused() {
        let history = history(false, "text");
        let before = history.prepare_queue_entry(1, None, 10).unwrap().unwrap();
        history.privacy().toggle(11).unwrap();
        assert_eq!(
            history
                .privacy()
                .check_read(&before.permit, 12)
                .unwrap_err(),
            dzc_core::api::PrivacyError::PrivacyEpochChanged
        );
        let active = history.prepare_queue_entry(1, None, 12).unwrap().unwrap();
        assert_eq!(active.entry.content, "synthetic queue body");
        assert!(history.privacy().check_read(&active.permit, 12).is_ok());
        assert!(history.privacy().capture_ticket(12).is_err());
        assert!(history.prepare_queue_entry(2, None, 12).unwrap().is_none());
        assert!(history.prepare_queue_entry(-1, None, 12).is_err());
    }
}
