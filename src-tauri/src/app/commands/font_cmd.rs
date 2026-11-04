use crate::{app::ipc_host::DesktopHost, error::{AppError, AppResult}};
use dzc_host::wire::{DesktopFont, DesktopFontList, Method};
use tauri::State;

#[tauri::command]
pub async fn list_system_fonts(host: State<'_, DesktopHost>) -> AppResult<Vec<DesktopFont>> {
    let list: DesktopFontList = host.call(Method::DesktopSettingsFonts, serde_json::json!({}), "DesktopFontList")
        .await.map_err(AppError::Validation)?;
    Ok(list.fonts)
}
