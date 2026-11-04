use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::read_dir;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppInfo {
    pub name: String,
    pub path: String,
    pub icon: Option<String>,
}

fn read_desktop_metadata(path: &Path) -> Option<String> {
    let bytes = dzc_core::filesystem::read_bounded_regular(path, 1024 * 1024).ok()?;
    String::from_utf8(bytes).ok()
}

fn application_dirs() -> Vec<PathBuf> {
    let system = std::env::var_os("XDG_DATA_DIRS")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    dirs::data_dir()
        .into_iter()
        .chain(std::env::split_paths(&system))
        .filter(|base| base.is_absolute())
        .map(|base| base.join("applications"))
        .collect()
}

pub enum Handler { System, Desktop(PathBuf), Executable(PathBuf) }

pub fn resolve_handler(handler: Option<&str>) -> Handler {
    // GIO expands the field codes in the installed desktop entry and preserves
    // its argument boundaries, quoting, working directory and activation rules.
    if let Some(handler) = handler {
        let desktop = application_dirs().into_iter()
            .filter_map(|directory| read_dir(directory).ok())
            .flatten().flatten().map(|entry| entry.path())
            .find(|path| path.extension().is_some_and(|ext| ext == "desktop")
                && parse_desktop_file(path).is_some_and(|app| app.path == handler));
        if let Some(desktop) = desktop {
            return Handler::Desktop(desktop);
        } else if Path::new(handler).is_absolute() && Path::new(handler).is_file() {
            return Handler::Executable(PathBuf::from(handler));
        }
    }
    Handler::System
}

pub(super) fn launch(handler: &Handler, target: &str, _url: bool) -> io::Result<()> {
    let mut command = Command::new("gio");
    match handler {
        Handler::Desktop(desktop) => { command.arg("launch").arg(desktop); }
        Handler::Executable(exe) => return Command::new(exe).arg(target).stdin(Stdio::null())
            .stdout(Stdio::null()).stderr(Stdio::null()).spawn().map(|_| ()),
        Handler::System => { command.arg("open"); }
    }
    let status = match command.arg(target).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status() {
        Err(error) if error.kind() == io::ErrorKind::NotFound && matches!(handler, Handler::System) => {
            return Command::new("xdg-open").arg(target).stdin(Stdio::null()).stdout(Stdio::null())
                .stderr(Stdio::null()).spawn().map(|_| ());
        }
        result => result?,
    };
    if status.success() { Ok(()) } else { Err(io::Error::other("DesktopLaunchFailed")) }
}

pub fn scan_installed_apps() -> io::Result<Vec<AppInfo>> {
    let mut apps = Vec::new();
    let mut seen = HashSet::new();

    let dirs = application_dirs();

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

pub fn get_system_default_app(content_type: String) -> io::Result<String> {
    let mime = map_to_mime(&content_type);
    let output = Command::new("xdg-mime")
        .args(["query", "default", &mime])
        .output()
        .map_err(|e| io::Error::other(format!("xdg-mime failed: {}", e)))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Ok(String::new())
    }
}

pub fn get_associated_apps(extension: String) -> io::Result<Vec<AppInfo>> {
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
        if let Some(content) = read_desktop_metadata(&mimeapps_path) {
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
        let paths = application_dirs()
            .into_iter()
            .map(|dir| dir.join(&desktop_name));

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

pub fn get_executable_icon(executable_path: String) -> io::Result<Option<String>> {
    let dirs = application_dirs();

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

fn parse_desktop_file(path: &Path) -> Option<AppInfo> {
    let content = read_desktop_metadata(path)?;
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
    let content = read_desktop_metadata(path)?;
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
        "code" | "text" | "rich_text" | "txt" | "plain" => "text/plain".to_string(),
        "image" | "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" => "image/png".to_string(),
        "url" | "link" => "x-scheme-handler/http".to_string(),
        "html" | "htm" => "text/html".to_string(),
        "video" | "mp4" | "mkv" | "avi" | "mov" | "webm" => "video/mp4".to_string(),
        _ => ct,
    }
}
