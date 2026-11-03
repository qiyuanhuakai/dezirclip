use dezirclip::infrastructure::desktop_tools::{self, AgentSkill, CliInfo, CliPathResult, find_filesystem_skill_path_in, install_folder_from_skill_path, open_folder};
use std::path::{Path, PathBuf};
use tauri::{path::BaseDirectory, AppHandle, Manager};

#[tauri::command]
pub async fn get_cli_info(app: AppHandle) -> Result<CliInfo, String> {
    tauri::async_runtime::spawn_blocking(move || desktop_tools::cli_info(find_bundled_cli_binary(), |skill| find_skill_path_buf(&app, skill)))
        .await
        .map_err(|e| format!("CLI info worker failed: {e}"))
}

#[tauri::command]
pub fn open_install_folder(app: AppHandle) -> Result<(), String> {
    let folder = find_install_folder(&app).ok_or_else(|| "install folder not found".to_string())?;
    open_folder(&folder).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_agent_skill_folder(app: AppHandle, skill: AgentSkill) -> Result<(), String> {
    let folder = find_skill_path_buf(&app, skill)
        .ok_or_else(|| format!("{} skill folder not found", skill.name()))?;
    open_folder(&folder).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn add_cli_to_path() -> Result<CliPathResult, String> {
    tauri::async_runtime::spawn_blocking(|| desktop_tools::install_cli_path(find_bundled_cli_binary()))
        .await
        .map_err(|e| format!("CLI PATH worker failed: {e}"))?
}

fn find_bundled_cli_binary() -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let win_path = dir.join("dzc.exe");
            if win_path.is_file() {
                return Some(win_path);
            }
            let unix_path = dir.join("dzc");
            if unix_path.is_file() {
                return Some(unix_path);
            }
        }
    }

    None
}

fn find_skill_path_buf(app: &AppHandle, skill: AgentSkill) -> Option<PathBuf> {
    bundled_skill_path(app, skill).or_else(|| find_filesystem_skill_path(skill))
}

fn bundled_skill_path(app: &AppHandle, skill: AgentSkill) -> Option<PathBuf> {
    [format!("skills/{}", skill.name()), format!("../skills/{}", skill.name())]
        .into_iter()
        .find_map(|resource| {
            app.path()
                .resolve(resource, BaseDirectory::Resource)
                .ok()
                .filter(|path| path.join("SKILL.md").is_file())
        })
}

fn find_filesystem_skill_path(skill: AgentSkill) -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    find_filesystem_skill_path_in(executable.parent()?, skill)
}

fn find_install_folder(app: &AppHandle) -> Option<PathBuf> {
    find_skill_path_buf(app, AgentSkill::DzcCli)
        .or_else(|| find_skill_path_buf(app, AgentSkill::DezirclipTheme))
        .and_then(|path| install_folder_from_skill_path(&path))
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(Path::to_path_buf))
        })
}
