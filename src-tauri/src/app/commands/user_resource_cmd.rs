use crate::app_state::AppDataDir;
use crate::database::DbState;
use crate::infrastructure::repository::settings_repo::SettingsRepository;
use crate::infrastructure::user_resource;
use std::path::Path;
use tauri::{AppHandle, Manager};

#[tauri::command]
pub async fn get_custom_background_resource(app: AppHandle, expected_path: String) -> Result<String, String> {
    if expected_path.is_empty() || expected_path.len() > 32_768 { return Err("UserResourceUnavailable".into()); }
    let configured = app.state::<DbState>().settings_repo.get("app.custom_background").map_err(|_| "UserResourceUnavailable")?;
    if configured.as_deref() != Some(expected_path.as_str()) { return Err("UserResourceUnavailable".into()); }
    let path = expected_path.clone();
    let data_dir = app.state::<AppDataDir>().0.lock().map_err(|_| "UserResourceUnavailable")?.clone();
    let data = tauri::async_runtime::spawn_blocking(move || {
        let selected = user_resource::selected_background_path(&data_dir, Path::new(&path))?;
        user_resource::read_image(&selected)
    }).await.map_err(|_| "UserResourceUnavailable")??;
    let current = app.state::<DbState>().settings_repo.get("app.custom_background").map_err(|_| "UserResourceUnavailable")?;
    if current.as_deref() != Some(expected_path.as_str()) { return Err("UserResourceUnavailable".into()); }
    Ok(data)
}

#[tauri::command]
pub async fn get_emoji_favorite_resource(app: AppHandle, path: String) -> Result<String, String> {
    if path.is_empty() || path.len() > 32_768 { return Err("UserResourceUnavailable".into()); }
    let data_dir = app.state::<AppDataDir>().0.lock().map_err(|_| "UserResourceUnavailable")?.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let selected = user_resource::managed_emoji_path(&data_dir, Path::new(&path))?;
        let data = user_resource::read_image(&selected)?;
        if user_resource::managed_emoji_path(&data_dir, Path::new(&path))? != selected {
            return Err("UserResourceUnavailable".into());
        }
        Ok(data)
    }).await.map_err(|_| "UserResourceUnavailable")?
}
