use crate::error::{AppError, AppResult};
use dzc_platform::apps::{self, AppInfo};
use std::process::Command;

#[tauri::command]
pub fn scan_installed_apps() -> AppResult<Vec<AppInfo>> {
    apps::scan_installed_apps().map_err(|e| AppError::Internal(e.to_string()))
}

#[tauri::command]
pub fn get_system_default_app(content_type: String) -> AppResult<String> {
    apps::get_system_default_app(content_type).map_err(|e| AppError::Internal(e.to_string()))
}

#[tauri::command]
pub fn get_associated_apps(extension: String) -> AppResult<Vec<AppInfo>> {
    apps::get_associated_apps(extension).map_err(|e| AppError::Internal(e.to_string()))
}

#[tauri::command]
pub fn get_executable_icon(executable_path: String) -> AppResult<Option<String>> {
    apps::get_executable_icon(executable_path).map_err(|e| AppError::Internal(e.to_string()))
}

pub fn launch_uwp_with_file(file: String) -> AppResult<()> {
    Command::new("xdg-open")
        .arg(&file)
        .status()
        .map_err(|e| AppError::Internal(format!("xdg-open failed: {}", e)))?;
    Ok(())
}
