use crate::app_state::AppDataDir;
use crate::database::DbState;
use crate::error::{AppError, AppResult};
use base64::Engine;
use image::ImageFormat;
use serde::Serialize;
use std::path::Path;
use tauri::State;

#[derive(Serialize)]
pub struct FileSize {
    pub size: u64,
}

#[tauri::command]
pub fn get_file_size(path: String) -> AppResult<FileSize> {
    use std::fs;
    let metadata = fs::metadata(&path).map_err(AppError::from)?;
    Ok(FileSize {
        size: metadata.len(),
    })
}

fn normalize_image_ext(ext: &str) -> Option<&'static str> {
    match ext.to_lowercase().as_str() {
        "png" => Some("png"),
        "jpg" | "jpeg" => Some("jpg"),
        "webp" => Some("webp"),
        "gif" => Some("gif"),
        _ => None,
    }
}

pub(crate) fn image_ext_from_filename(name: &str) -> Option<&'static str> {
    let ext = Path::new(name)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    normalize_image_ext(ext)
}

pub(crate) fn image_ext_from_bytes(bytes: &[u8]) -> Option<&'static str> {
    let format = image::guess_format(bytes).ok()?;
    match format {
        ImageFormat::Png => Some("png"),
        ImageFormat::Jpeg => Some("jpg"),
        ImageFormat::Gif => Some("gif"),
        ImageFormat::WebP => Some("webp"),
        _ => None,
    }
}

pub(crate) fn list_emoji_favorite_paths_in_dir(data_dir: &Path) -> AppResult<Vec<String>> {
    dzc_platform::user_resource::list_emoji_favorites(data_dir).map_err(AppError::Internal)
}

#[tauri::command]
pub async fn save_emoji_favorite(
    app_data: State<'_, AppDataDir>,
    db: State<'_, DbState>,
    source_path: String,
) -> AppResult<String> {
    let conn = db.conn.lock().map_err(|error| AppError::Internal(error.to_string()))?;
    crate::database::ensure_writable(&conn).map_err(AppError::Internal)?;
    let source_path = source_path.trim();
    if source_path.is_empty() {
        return Err(AppError::Validation("source_path is empty".to_string()));
    }

    if image_ext_from_filename(source_path).is_none() { return Err(AppError::Validation("unsupported file type".into())); }
    let bytes = dzc_platform::user_resource::read_image_bytes(Path::new(source_path)).map_err(AppError::Internal)?;
    let data_dir = app_data.0.lock().unwrap().clone();
    dzc_platform::user_resource::prepare_emoji_favorite(&data_dir, &bytes).and_then(|image| image.publish()).map_err(AppError::Internal)
}

#[tauri::command]
pub async fn remove_emoji_favorite(app_data: State<'_, AppDataDir>, db: State<'_, DbState>, path: String) -> AppResult<()> {
    let conn = db.conn.lock().map_err(|error| AppError::Internal(error.to_string()))?;
    crate::database::ensure_writable(&conn).map_err(AppError::Internal)?;
    if path.trim().is_empty() {
        return Ok(());
    }

    let data_dir = app_data.0.lock().unwrap().clone();
    dzc_platform::user_resource::remove_emoji_favorite(&data_dir, Path::new(&path)).map_err(AppError::Internal)
}

#[tauri::command]
pub fn list_emoji_favorites(app_data: State<'_, AppDataDir>) -> AppResult<Vec<String>> {
    let data_dir = app_data.0.lock().unwrap().clone();
    list_emoji_favorite_paths_in_dir(&data_dir)
}

#[tauri::command]
pub async fn save_emoji_favorite_data_url(
    app_data: State<'_, AppDataDir>,
    db: State<'_, DbState>,
    data_url: String,
    file_name: Option<String>,
) -> AppResult<String> {
    let conn = db.conn.lock().map_err(|error| AppError::Internal(error.to_string()))?;
    crate::database::ensure_writable(&conn).map_err(AppError::Internal)?;
    let _ = file_name;
    let payload = if data_url.starts_with("data:") {
        data_url.split_once(',').map(|(_, data)| data).unwrap_or("")
    } else { &data_url };
    if payload.len() > 22_369_624 { return Err(AppError::Validation("image exceeds 16 MiB".into())); }
    let bytes = base64::engine::general_purpose::STANDARD.decode(payload).map_err(|error| AppError::Validation(error.to_string()))?;
    let data_dir = app_data.0.lock().unwrap().clone();
    dzc_platform::user_resource::prepare_emoji_favorite(&data_dir, &bytes).and_then(|image| image.publish()).map_err(AppError::Internal)
}
