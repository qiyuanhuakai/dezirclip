use tauri::{AppHandle, Manager, WebviewWindow};

#[tauri::command]
pub async fn show_compact_preview_without_activation(app: AppHandle, window: WebviewWindow) -> Result<(), String> {
    if !crate::infrastructure::preview_activation::caller_allowed(window.label()) {
        return Err("MainWindowRequired".into());
    }
    let (send, receive) = tokio::sync::oneshot::channel();
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let result = handle.get_webview_window("compact-preview")
            .ok_or_else(|| "CompactPreviewUnavailable".to_string())
            .and_then(|preview| crate::infrastructure::preview_activation::show(&preview));
        let _ = send.send(result);
    }).map_err(|error| error.to_string())?;
    receive.await.map_err(|_| "PreviewShowDispatchUnavailable".to_string())?
}
