use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub struct CliInfo {
    pub cli_path: Option<String>,
    pub cli_version: String,
    pub skill_path: Option<String>,
}

#[tauri::command]
pub fn get_cli_info() -> Result<CliInfo, String> {
    let cli_path = find_cli_binary();
    let cli_version = get_cli_version(&cli_path);
    let skill_path = find_skill_path();

    Ok(CliInfo {
        cli_path,
        cli_version,
        skill_path,
    })
}

fn find_cli_binary() -> Option<String> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let win_path = dir.join("tiez-c.exe");
            if win_path.exists() {
                return Some(win_path.to_string_lossy().to_string());
            }
            let unix_path = dir.join("tiez-c");
            if unix_path.exists() {
                return Some(unix_path.to_string_lossy().to_string());
            }
        }
    }

    if let Ok(output) = std::process::Command::new("which").arg("tiez-c").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Some(path);
            }
        }
    }

    None
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
                dir.join("skills").join("tiez-c-cli"),
                dir.join("../skills").join("tiez-c-cli"),
                dir.join("../../skills").join("tiez-c-cli"),
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
