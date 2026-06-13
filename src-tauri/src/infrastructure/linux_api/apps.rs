use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{read_dir, read_to_string};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppInfo {
    pub name: String,
    pub path: String,
    pub icon: Option<String>,
}

#[tauri::command]
pub fn scan_installed_apps() -> AppResult<Vec<AppInfo>> {
    let mut apps = Vec::new();
    let mut seen = HashSet::new();

    let dirs = vec![
        PathBuf::from("/usr/share/applications/"),
        dirs::home_dir()
            .map(|h| h.join(".local/share/applications/"))
            .unwrap_or_default(),
    ];

    for dir in dirs {
        if !dir.exists() {
            continue;
        }
        if let Ok(entries) = read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("desktop") {
                    if let Some(app) = parse_desktop_file(&path) {
                        if seen.insert(app.path.clone()) {
                            apps.push(app);
                        }
                    }
                }
            }
        }
    }

    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(apps)
}

#[tauri::command]
pub fn get_system_default_app(content_type: String) -> AppResult<String> {
    let mime = map_to_mime(&content_type);
    let output = Command::new("xdg-mime")
        .args(["query", "default", &mime])
        .output()
        .map_err(|e| AppError::Internal(format!("xdg-mime failed: {}", e)))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Ok(String::new())
    }
}

#[tauri::command]
pub fn get_associated_apps(extension: String) -> AppResult<Vec<AppInfo>> {
    let mime = map_to_mime(&extension);
    let mut desktop_files = Vec::new();

    let mimeapps_paths = vec![
        dirs::home_dir()
            .map(|h| h.join(".config/mimeapps.list"))
            .unwrap_or_default(),
        PathBuf::from("/usr/share/applications/mimeapps.list"),
    ];

    for mimeapps_path in mimeapps_paths {
        if !mimeapps_path.exists() {
            continue;
        }
        if let Ok(content) = read_to_string(&mimeapps_path) {
            let mut in_relevant_section = false;

            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed == "[Default Applications]" || trimmed == "[Added Associations]" {
                    in_relevant_section = true;
                    continue;
                } else if trimmed.starts_with('[') && trimmed.ends_with(']') {
                    in_relevant_section = false;
                    continue;
                }

                if in_relevant_section && trimmed.contains('=') {
                    if let Some((key, value)) = trimmed.split_once('=') {
                        if key.trim() == mime {
                            for desktop in value.split(';') {
                                let d = desktop.trim();
                                if !d.is_empty() && !desktop_files.contains(&d.to_string()) {
                                    desktop_files.push(d.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let mut apps = Vec::new();
    let mut seen = HashSet::new();

    for desktop_name in desktop_files {
        let paths = vec![
            dirs::home_dir()
                .map(|h| h.join(".local/share/applications/").join(&desktop_name))
                .unwrap_or_default(),
            PathBuf::from("/usr/share/applications/").join(&desktop_name),
        ];

        for path in paths {
            if path.exists() {
                if let Some(app) = parse_desktop_file(&path) {
                    if seen.insert(app.path.clone()) {
                        apps.push(app);
                    }
                }
                break;
            }
        }
    }

    Ok(apps)
}

#[tauri::command]
pub fn get_executable_icon(executable_path: String) -> AppResult<Option<String>> {
    let dirs = vec![
        PathBuf::from("/usr/share/applications/"),
        dirs::home_dir()
            .map(|h| h.join(".local/share/applications/"))
            .unwrap_or_default(),
    ];

    let target_name = Path::new(&executable_path)
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| executable_path.clone());

    for dir in dirs {
        if !dir.exists() {
            continue;
        }
        if let Ok(entries) = read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("desktop") {
                    if let Some(exec) = get_desktop_field(&path, "Exec") {
                        let cleaned = strip_field_codes(&exec);
                        let exec_name = Path::new(&cleaned)
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or(&cleaned);
                        if exec_name == target_name || cleaned == executable_path {
                            if let Some(icon) = get_desktop_field(&path, "Icon") {
                                return Ok(resolve_icon_path(&icon));
                            }
                            return Ok(None);
                        }
                    }
                }
            }
        }
    }

    Ok(None)
}

pub fn launch_uwp_with_file(file: String) -> AppResult<()> {
    Command::new("xdg-open")
        .arg(&file)
        .status()
        .map_err(|e| AppError::Internal(format!("xdg-open failed: {}", e)))?;
    Ok(())
}

fn parse_desktop_file(path: &Path) -> Option<AppInfo> {
    let content = read_to_string(path).ok()?;
    let mut name = None;
    let mut exec = None;
    let mut icon = None;
    let mut no_display = false;
    let mut terminal = false;
    let mut in_desktop_entry = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[Desktop Entry]" {
            in_desktop_entry = true;
            continue;
        } else if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_desktop_entry = false;
            continue;
        }

        if !in_desktop_entry {
            continue;
        }

        if let Some((key, value)) = trimmed.split_once('=') {
            match key.trim() {
                "Name" => name = Some(value.trim().to_string()),
                "Exec" => exec = Some(value.trim().to_string()),
                "Icon" => icon = Some(value.trim().to_string()),
                "NoDisplay" => no_display = value.trim().eq_ignore_ascii_case("true"),
                "Terminal" => terminal = value.trim().eq_ignore_ascii_case("true"),
                _ => {}
            }
        }
    }

    if no_display || terminal {
        return None;
    }

    let exec_str = exec?;
    let cleaned_exec = strip_field_codes(&exec_str);
    if cleaned_exec.is_empty() {
        return None;
    }

    let resolved_icon = icon.as_ref().and_then(|i| resolve_icon_path(i));

    Some(AppInfo {
        name: name.unwrap_or_else(|| {
            path.file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        }),
        path: cleaned_exec,
        icon: resolved_icon,
    })
}

fn get_desktop_field(path: &Path, field: &str) -> Option<String> {
    let content = read_to_string(path).ok()?;
    let mut in_desktop_entry = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[Desktop Entry]" {
            in_desktop_entry = true;
            continue;
        } else if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_desktop_entry = false;
            continue;
        }
        if in_desktop_entry {
            if let Some((key, value)) = trimmed.split_once('=') {
                if key.trim() == field {
                    return Some(value.trim().to_string());
                }
            }
        }
    }
    None
}

fn strip_field_codes(exec: &str) -> String {
    let mut result = String::with_capacity(exec.len());
    let mut chars = exec.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '%' {
            if let Some(next) = chars.next() {
                if next == '%' {
                    result.push('%');
                }
            } else {
                result.push('%');
            }
        } else {
            result.push(ch);
        }
    }
    result.trim().to_string()
}

fn resolve_icon_path(icon_name: &str) -> Option<String> {
    let path = Path::new(icon_name);
    if path.is_absolute() && path.exists() {
        return Some(icon_name.to_string());
    }

    let candidates: Vec<PathBuf> = vec![
        PathBuf::from(format!(
            "/usr/share/icons/hicolor/48x48/apps/{}.png",
            icon_name
        )),
        PathBuf::from(format!(
            "/usr/share/icons/hicolor/64x64/apps/{}.png",
            icon_name
        )),
        PathBuf::from(format!(
            "/usr/share/icons/hicolor/128x128/apps/{}.png",
            icon_name
        )),
        PathBuf::from(format!(
            "/usr/share/icons/hicolor/256x256/apps/{}.png",
            icon_name
        )),
        PathBuf::from(format!(
            "/usr/share/icons/hicolor/scalable/apps/{}.svg",
            icon_name
        )),
        PathBuf::from(format!(
            "/usr/share/icons/hicolor/48x48/apps/{}.svg",
            icon_name
        )),
        PathBuf::from(format!("/usr/share/pixmaps/{}.png", icon_name)),
        PathBuf::from(format!("/usr/share/pixmaps/{}.svg", icon_name)),
        PathBuf::from(format!("/usr/share/pixmaps/{}.xpm", icon_name)),
    ];

    for p in &candidates {
        if p.exists() {
            return Some(p.to_string_lossy().to_string());
        }
    }

    if let Ok(entries) = read_dir("/usr/share/icons/") {
        for entry in entries.flatten() {
            let theme_path = entry.path();
            if !theme_path.is_dir() {
                continue;
            }
            let theme_name = theme_path.file_name().and_then(|s| s.to_str())?;
            if theme_name == "hicolor" {
                continue;
            }
            let sizes = vec!["48x48", "64x64", "128x128", "256x256", "scalable"];
            for size in sizes {
                let ext = if size == "scalable" { "svg" } else { "png" };
                let candidate = theme_path.join(format!("{}/apps/{}.{}", size, icon_name, ext));
                if candidate.exists() {
                    return Some(candidate.to_string_lossy().to_string());
                }
            }
        }
    }

    None
}

fn map_to_mime(content_type: &str) -> String {
    let ct = content_type.trim_start_matches('.').to_lowercase();
    match ct.as_str() {
        "text" | "txt" | "plain" => "text/plain".to_string(),
        "image" | "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" => "image/png".to_string(),
        "html" | "htm" => "text/html".to_string(),
        "video" | "mp4" | "mkv" | "avi" | "mov" | "webm" => "video/mp4".to_string(),
        _ => ct,
    }
}
