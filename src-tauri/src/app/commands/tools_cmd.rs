use crate::infrastructure::cli_path;
use serde::Serialize;
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
    let (command, arg) = path_lookup_command();
    let output = std::process::Command::new(command).arg(arg).output().ok()?;
    if !output.status.success() {
        return None;
    }

    let path = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_string();
    if path.is_empty() {
        None
    } else {
        Some(path)
    }
}

fn path_lookup_command() -> (&'static str, &'static str) {
    if cfg!(target_os = "windows") {
        ("where", "dzc.exe")
    } else {
        ("which", "dzc")
    }
}

fn get_cli_version(cli_path: &Option<String>) -> String {
    let path = match cli_path {
        Some(p) => p,
        None => return "not installed".to_string(),
    };

    std::process::Command::new(path)
        .arg("--version")
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
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
    fn test_path_lookup_command_uses_platform_binary_name() {
        let (command, arg) = path_lookup_command();
        if cfg!(target_os = "windows") {
            assert_eq!(command, "where");
            assert_eq!(arg, "dzc.exe");
        } else {
            assert_eq!(command, "which");
            assert_eq!(arg, "dzc");
        }
    }
}
