use crate::app_state::AppDataDir;
use crate::database::DbState;
use crate::infrastructure::repository::settings_repo::SettingsRepository;
use crate::infrastructure::user_resource;
use std::path::Path;
use tauri::{AppHandle, Manager};

fn require_selected_background(configured: Option<&str>, expected_path: &str) -> Result<(), String> {
    if expected_path.is_empty() || expected_path.len() > 32_768 { return Err("UserResourceUnavailable".into()); }
    if configured != Some(expected_path) { return Err("CustomBackgroundPending".into()); }
    Ok(())
}

#[tauri::command]
pub async fn get_custom_background_resource(app: AppHandle, expected_path: String) -> Result<String, String> {
    let configured = app.state::<DbState>().settings_repo.get("app.custom_background").map_err(|_| "UserResourceUnavailable")?;
    require_selected_background(configured.as_deref(), &expected_path)?;
    let path = expected_path.clone();
    let data_dir = app.state::<AppDataDir>().0.lock().map_err(|_| "UserResourceUnavailable")?.clone();
    let data = tauri::async_runtime::spawn_blocking(move || {
        let selected = user_resource::selected_background_path(&data_dir, Path::new(&path))?;
        user_resource::read_image(&selected)
    }).await.map_err(|_| "UserResourceUnavailable")??;
    let current = app.state::<DbState>().settings_repo.get("app.custom_background").map_err(|_| "UserResourceUnavailable")?;
    require_selected_background(current.as_deref(), &expected_path)?;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use crate::infrastructure::repository::settings_repo::SqliteSettingsRepository;
    use std::sync::{Arc, Mutex};

    #[test]
    fn selected_background_png_roundtrips_real_settings_filesystem_and_decoder() {
        let owned = std::env::temp_dir().join(format!("dzc-background-chain-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&owned).unwrap();
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
        let _cleanup = Cleanup(owned.clone());
        let data_dir = owned.join("private app data"); std::fs::create_dir(&data_dir).unwrap();
        let selected = owned.join("背景 测试.png");
        image::DynamicImage::new_rgba8(64, 48).save(&selected).unwrap();
        let expected = selected.to_str().unwrap();
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute("CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)", []).unwrap();
        let repo = SqliteSettingsRepository::new(Arc::new(Mutex::new(conn)));
        assert_eq!(require_selected_background(repo.get("app.custom_background").unwrap().as_deref(), expected).unwrap_err(), "CustomBackgroundPending");
        repo.set("app.custom_background", expected).unwrap();
        let persisted = repo.get("app.custom_background").unwrap();
        require_selected_background(persisted.as_deref(), expected).unwrap();
        let path = user_resource::selected_background_path(&data_dir, Path::new(expected)).unwrap();
        let url = user_resource::read_image(&path).unwrap();
        let bytes = base64::engine::general_purpose::STANDARD.decode(url.strip_prefix("data:image/png;base64,").unwrap()).unwrap();
        assert_eq!(bytes, std::fs::read(&selected).unwrap());
        assert_eq!(image::load_from_memory(&bytes).unwrap().width(), 64);
        assert_eq!(image::load_from_memory(&bytes).unwrap().height(), 48);
        repo.set("app.custom_background", "new-selection.png").unwrap();
        assert_eq!(require_selected_background(repo.get("app.custom_background").unwrap().as_deref(), expected).unwrap_err(), "CustomBackgroundPending");
    }
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
