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
