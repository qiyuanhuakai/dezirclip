use crate::infrastructure::cli_path;
use serde::Serialize;
use std::ffi::OsStr;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub struct CliInfo {
    pub cli_path: Option<String>,
    pub cli_version: String,
    pub skill_path: Option<String>,
    pub cli_on_path: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CliPathResult {
    pub installed_path: String,
    pub path_entry: String,
    pub already_linked: bool,
    pub requires_new_terminal: bool,
}

#[tauri::command]
pub fn get_cli_info() -> Result<CliInfo, String> {
    let cli_on_path = find_cli_on_path().is_some();
    let cli_path = find_cli_binary();
    let cli_version = get_cli_version(&cli_path);
    let skill_path = find_skill_path();

    Ok(CliInfo {
        cli_path,
        cli_version,
        skill_path,
        cli_on_path,
    })
}

#[tauri::command]
pub fn add_cli_to_path() -> Result<CliPathResult, String> {
    let cli_path = find_bundled_cli_binary()
        .or_else(|| find_cli_on_path().map(PathBuf::from))
        .ok_or_else(|| "dzc executable not found".to_string())?;
    let result = cli_path::add_cli_to_path(&cli_path)?;
    Ok(CliPathResult {
        installed_path: result.installed_path.to_string_lossy().to_string(),
        path_entry: result.path_entry.to_string_lossy().to_string(),
        already_linked: result.already_linked,
        requires_new_terminal: result.requires_new_terminal,
    })
}

fn find_cli_binary() -> Option<String> {
    find_bundled_cli_binary()
        .map(|path| path.to_string_lossy().to_string())
        .or_else(find_cli_on_path)
}

fn find_bundled_cli_binary() -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let win_path = dir.join("dzc.exe");
            if win_path.exists() {
                return Some(win_path);
            }
            let unix_path = dir.join("dzc");
            if unix_path.exists() {
                return Some(unix_path);
            }
        }
    }

    None
}

fn find_cli_on_path() -> Option<String> {
    let path_value = std::env::var_os("PATH")?;
    find_cli_on_path_in(&path_value, cli_binary_name())
        .map(|path| path.to_string_lossy().to_string())
}

fn find_cli_on_path_in(path_value: &OsStr, binary_name: &str) -> Option<PathBuf> {
    std::env::split_paths(path_value)
        .map(|entry| entry.join(binary_name))
        .find(|candidate| candidate.is_file())
}

fn cli_binary_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "dzc.exe"
    } else {
        "dzc"
    }
}

fn get_cli_version(cli_path: &Option<String>) -> String {
    if cli_path.is_some() {
        env!("CARGO_PKG_VERSION").to_string()
    } else {
        "not installed".to_string()
    }
}

fn find_skill_path() -> Option<String> {
    let candidates: Vec<PathBuf> = if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            vec![
                dir.join("skills").join("dzc-cli"),
                dir.join("../skills").join("dzc-cli"),
                dir.join("../../skills").join("dzc-cli"),
            ]
        } else {
            vec![]
        }
    } else {
        vec![]
    };

    for path in candidates {
        if path.exists() {
            return Some(path.to_string_lossy().to_string());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_cli_on_path_scans_path_without_shell_command() {
        let binary_name = cli_binary_name();
        let temp_dir = std::env::temp_dir().join(format!(
            "dezirclip-cli-path-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");
        let cli_path = temp_dir.join(binary_name);
        std::fs::write(&cli_path, b"").expect("create cli file");

        let path_value = std::env::join_paths([temp_dir.clone()]).expect("join path");

        assert_eq!(find_cli_on_path_in(&path_value, binary_name), Some(cli_path));

        std::fs::remove_dir_all(temp_dir).expect("remove temp dir");
    }
}
