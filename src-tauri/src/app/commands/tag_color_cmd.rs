use crate::app::ipc_host::DesktopHost;
use crate::error::{AppError, AppResult};
use dzc_host::wire::{LibraryColorParams, LibraryColors, Method};
use std::collections::HashMap;
use tauri::{AppHandle, Manager, State};

#[tauri::command]
pub async fn set_tag_color(app: AppHandle, name: String, color: Option<String>) -> AppResult<()> {
    let state = super::history_cmd::library_state(&app.state::<DesktopHost>()).await?;
    super::history_cmd::edit_library(&app, Method::DesktopTagsColor, LibraryColorParams {
        service_instance_id: state.service_instance_id, expected_epoch: state.privacy_epoch, name, color,
    }).await
}

#[tauri::command]
pub async fn get_tag_colors(host: State<'_, DesktopHost>) -> AppResult<HashMap<String, String>> {
    let result: LibraryColors = host.library_snapshot(Method::DesktopTagsColors, "LibraryColors").await.map_err(AppError::Validation)?;
    serde_json::from_value(result.values).map_err(|error| AppError::Internal(error.to_string()))
}
