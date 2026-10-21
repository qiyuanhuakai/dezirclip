// Content handler module for opening various content types
use crate::database::DbState;
use crate::error::AppError;
use crate::infrastructure::repository::settings_repo::SettingsRepository;
#[cfg(target_os = "windows")]
use crate::infrastructure::windows_api::apps::launch_uwp_with_file;
use base64::{engine::general_purpose, Engine as _};
use std::collections::HashMap;
use std::io::Read;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::{mpsc, OnceLock};
use tauri::{Emitter, Manager, State};

#[tauri::command]
pub async fn open_content(
    app_handle: tauri::AppHandle,
    state: State<'_, DbState>,
    id: i64,
    content: String,
    content_type: String,
    expected_privacy_epoch: Option<u64>,
) -> Result<(), AppError> {
    use dzc_core::api::{PrivacyAccessIntent, SensitiveAccess};
    let history = state.history_service();
    let now = || chrono::Utc::now().timestamp_millis();
    let ticket = history
        .privacy()
        .capture_ticket(now())
        .map_err(|error| AppError::Validation(error.to_string()))?;
    let permit = history
        .privacy()
        .authorize_read(
            PrivacyAccessIntent::SelectedCopy,
            if expected_privacy_epoch.is_some() {
                SensitiveAccess::ExplicitUserAction
            } else {
                SensitiveAccess::Denied
            },
            Some(expected_privacy_epoch.unwrap_or(ticket.epoch())),
            now(),
        )
        .map_err(|error| AppError::Validation(error.to_string()))?;
    let entry = if id > 0 {
        history
            .get_entry_authorized(id, &permit, now())
            .map_err(|error| AppError::Validation(error.to_string()))?
            .ok_or_else(|| AppError::Validation("SensitiveAccessDenied".into()))?
    } else if id < 0 {
        let entry = app_handle
            .state::<crate::app_state::SessionHistory>()
            .0
            .lock()
            .map_err(|_| AppError::Internal("PrivacyStateUnavailable".into()))?
            .iter()
            .find(|entry| entry.id == id)
            .cloned()
            .ok_or_else(|| AppError::Validation("ClipboardRecordUnavailable".into()))?;
        history
            .authorize_session_entry(&entry, &permit, now())
            .map_err(|error| AppError::Validation(error.to_string()))?
    } else {
        serde_json::from_value(serde_json::json!({"id":-1,"content_type":content_type,"content":content,"preview":"","source_app":"Manual","timestamp":now(),"is_pinned":false})).map_err(|_|AppError::Validation("PrivateExportRejected".into()))?
    };
    let content_type = entry.content_type.clone();
    let app_path = get_app_path_for_content_type(&state, &content_type)?;
    if matches!(content_type.as_str(), "link" | "url") {
        history
            .privacy()
            .check_read(&permit, now())
            .map_err(|error| AppError::Validation(error.to_string()))?;
        return handle_url_content(&app_path, &entry.content).await;
    }
    let export = if id > 0 {
        history.external_files_authorized(id, &permit, now())
    } else {
        history.session_external_files_authorized(&entry, &permit, now())
    }
    .map_err(|error| AppError::Validation(error.to_string()))?;
    let lease = dzc_platform::runtime::private_file_export(export)
        .map_err(|error| AppError::Internal(error.to_string()))?;
    let path = lease
        .paths()
        .map_err(|error| AppError::Internal(error.to_string()))?
        .into_iter()
        .next()
        .ok_or_else(|| AppError::Validation("PrivateExportRejected".into()))?;
    let file_path = std::path::PathBuf::from(&path);
    history
        .privacy()
        .check_read(&permit, now())
        .map_err(|error| AppError::Validation(error.to_string()))?;
    launch_file_with_app(&app_path, &file_path, &path, &content_type, false).await?;
    history
        .privacy()
        .check_read(&permit, now())
        .map_err(|error| AppError::Validation(error.to_string()))?;
    lease.retain();
    if id != 0
        && matches!(
            content_type.as_str(),
            "image" | "text" | "rich_text" | "code"
        )
    {
        start_file_watcher(app_handle, file_path, content_type, id, ticket);
    }
    Ok(())
}

fn get_app_path_for_content_type(
    state: &State<'_, DbState>,
    content_type: &str,
) -> Result<Option<String>, AppError> {
    let setting_key = format!("app.{}", content_type);
    let mut val = state
        .settings_repo
        .get(&setting_key)
        .map_err(AppError::from)?
        .filter(|s| !s.trim().is_empty());

    // Fallback: If 'code' app is not set, use 'text' app
    if val.is_none() && content_type == "code" {
        val = state
            .settings_repo
            .get("app.text")
            .map_err(AppError::from)?
            .filter(|s| !s.trim().is_empty());
    }
    Ok(val)
}

async fn handle_url_content(app_path: &Option<String>, content: &str) -> Result<(), AppError> {
    if let Some(app) = app_path {
        if std::path::Path::new(app).exists() {
            Command::new(app)
                .arg(content)
                .spawn()
                .map_err(|e| AppError::Internal(format!("启动程序失败: {}", e)))?;
            return Ok(());
        } else {
            // Check for macOS-style paths on Windows to avoid invalid Start-Process calls
            #[cfg(target_os = "windows")]
            if app.starts_with("/Applications/") || app.contains(".app") {
                return launch_default_handler(content).await;
            }

            println!("Attempting to launch URL handler: {}", app);

            #[cfg(target_os = "windows")]
            {
                let ps_script = format!(
                    "Start-Process -FilePath 'shell:AppsFolder\\{}' -ArgumentList '{}'",
                    app,
                    content.replace("'", "''")
                );
                let mut cmd = Command::new("powershell");
                cmd.args(["-NoProfile", "-Command", &ps_script])
                    .creation_flags(0x08000000);

                match cmd.spawn() {
                    Ok(_) => return Ok(()),
                    Err(e) => {
                        println!(
                            "Failed to launch via powershell: {}, falling back to default",
                            e
                        );
                        return launch_default_handler(content).await;
                    }
                }
            }

            #[cfg(not(target_os = "windows"))]
            {
                return launch_default_handler(content).await;
            }
        }
    } else {
        return launch_default_handler(content).await;
    }
}

async fn launch_default_handler(content: &str) -> Result<(), AppError> {
    #[cfg(target_os = "windows")]
    {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "start", "", content])
            .creation_flags(0x08000000);
        cmd.spawn()
            .map_err(|e| AppError::Internal(format!("启动默认浏览器失败: {}", e)))?;
    }

    #[cfg(not(target_os = "windows"))]
    {
        Command::new("xdg-open")
            .arg(content)
            .spawn()
            .map_err(|e| AppError::Internal(format!("启动默认浏览器失败: {}", e)))?;
    }
    Ok(())
}

fn is_file_type(content_type: &str) -> bool {
    matches!(content_type, "file" | "video" | "image")
}

fn get_existing_file_path(content: &str) -> Option<std::path::PathBuf> {
    let first_line = content.lines().next().unwrap_or(content).trim();
    let path = std::path::Path::new(first_line);
    if path.exists() {
        Some(path.to_path_buf())
    } else {
        None
    }
}

fn create_temp_file(
    content: &str,
    content_type: &str,
    filename: &str,
    mut temp_path: std::path::PathBuf,
) -> Result<std::path::PathBuf, AppError> {
    match content_type {
        "image" => {
            let is_gif = content.contains("image/gif");
            let extension = if is_gif { "gif" } else { "png" };
            temp_path.push(format!("{}.{}", filename, extension));

            let b64_data = if content.starts_with("data:image") {
                content.split(',').nth(1).unwrap_or(content)
            } else {
                content
            };
            let bytes = general_purpose::STANDARD
                .decode(b64_data)
                .map_err(|e| e.to_string())?;

            if is_gif {
                // For GIFs, write raw bytes directly to preserve animation
                std::fs::write(&temp_path, &bytes).map_err(AppError::from)?;
            } else {
                // For other images, use image crate to ensure standard peak format
                let img = image::load_from_memory(&bytes).map_err(AppError::from)?;
                img.save(&temp_path).map_err(AppError::from)?;
            }
        }
        _ => {
            temp_path.push(format!("{}.txt", filename));
            let mut file = std::fs::File::create(&temp_path).map_err(AppError::from)?;
            use std::io::Write;
            file.write_all(content.as_bytes())
                .map_err(|e| AppError::IO(e.to_string()))?;
        }
    }
    Ok(temp_path)
}

async fn launch_file_with_app(
    app_path: &Option<String>,
    temp_path: &std::path::Path,
    path_str: &str,
    content_type: &str,
    use_direct_path: bool,
) -> Result<(), AppError> {
    if let Some(app) = app_path {
        if std::path::Path::new(app).exists() {
            Command::new(app)
                .arg(temp_path)
                .spawn()
                .map_err(|e| format!("启动程序失败: {}", e))?;
        } else {
            // macOS style apps on Windows should fall back to default
            #[cfg(target_os = "windows")]
            if app.starts_with("/Applications/") || app.contains(".app") {
                return launch_with_default_app(path_str, content_type, use_direct_path);
            }

            let safe_path = path_str.replace("'", "''");
            // Try UWP launch on Windows
            #[cfg(target_os = "windows")]
            {
                println!(
                    "Attempting to launch UWP app: {} for file: {}",
                    app, path_str
                );
                if let Err(e) = launch_uwp_with_file(app, path_str).await {
                    println!("WinRT launch failed: {}, falling back to old method", e);
                    let ps_script = format!(
                        "Start-Process -FilePath 'shell:AppsFolder\\{}' -ArgumentList '{}'",
                        app, safe_path
                    );
                    let mut cmd = Command::new("powershell");
                    cmd.args(["-NoProfile", "-Command", &ps_script])
                        .creation_flags(0x08000000);
                    match cmd.spawn() {
                        Ok(_) => return Ok(()),
                        Err(err) => {
                            println!("Fallback launch failed: {}, using system default", err);
                            return launch_with_default_app(
                                path_str,
                                content_type,
                                use_direct_path,
                            );
                        }
                    }
                }
            }
            #[cfg(not(target_os = "windows"))]
            {
                Command::new("xdg-open")
                    .arg(safe_path)
                    .spawn()
                    .map_err(|e| {
                        AppError::Internal(format!("启动 UWP 程序失败 (Fallback): {}", e))
                    })?;
            }
        }
    } else {
        launch_with_default_app(path_str, content_type, use_direct_path)?;
    }
    Ok(())
}

fn launch_with_default_app(
    path_str: &str,
    _content_type: &str,
    _use_direct_path: bool,
) -> Result<(), AppError> {
    #[cfg(target_os = "windows")]
    {
        use windows::core::{HSTRING, PCWSTR};
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOW;

        let operation = HSTRING::from("open");
        let file = HSTRING::from(path_str);

        let ret = unsafe {
            ShellExecuteW(
                None,
                PCWSTR::from_raw(operation.as_ptr()),
                PCWSTR::from_raw(file.as_ptr()),
                None,
                None,
                SW_SHOW,
            )
        };

        if (ret.0 as isize) <= 32 {
            return Err(AppError::Internal(format!(
                "Failed to open file (ShellExecute error code: {})",
                ret.0 as isize
            )));
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        Command::new("xdg-open")
            .arg(path_str)
            .spawn()
            .map_err(|e| AppError::Internal(format!("Failed to open file: {}", e)))?;
    }

    Ok(())
}

struct WatchEntry {
    app_handle: tauri::AppHandle,
    file_path: std::path::PathBuf,
    content_type: String,
    id: i64,
    ticket: dzc_core::api::CaptureTicket,
    last_mtime: Option<std::time::SystemTime>,
    start_time: std::time::Instant,
}

enum FileWatcherCommand {
    Watch {
        app_handle: tauri::AppHandle,
        file_path: std::path::PathBuf,
        content_type: String,
        id: i64,
        ticket: dzc_core::api::CaptureTicket,
    },
}

fn watcher_sender() -> &'static mpsc::Sender<FileWatcherCommand> {
    static WATCHER_SENDER: OnceLock<mpsc::Sender<FileWatcherCommand>> = OnceLock::new();
    WATCHER_SENDER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<FileWatcherCommand>();
        let _ = std::thread::Builder::new()
            .name("content-file-watcher".to_string())
            .spawn(move || run_file_watcher_loop(rx));
        tx
    })
}

fn run_file_watcher_loop(rx: mpsc::Receiver<FileWatcherCommand>) {
    let mut watchers: HashMap<String, WatchEntry> = HashMap::new();
    loop {
        match rx.recv_timeout(std::time::Duration::from_secs(2)) {
            Ok(FileWatcherCommand::Watch {
                app_handle,
                file_path,
                content_type,
                id,
                ticket,
            }) => {
                let key = file_path.to_string_lossy().to_string();
                let last_mtime = std::fs::metadata(&file_path)
                    .ok()
                    .and_then(|m| m.modified().ok());
                watchers.insert(
                    key,
                    WatchEntry {
                        app_handle,
                        file_path,
                        content_type,
                        id,
                        ticket,
                        last_mtime,
                        start_time: std::time::Instant::now(),
                    },
                );
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        if watchers.is_empty() {
            continue;
        }

        let mut remove_keys = Vec::new();
        for (key, entry) in watchers.iter_mut() {
            if entry.start_time.elapsed().as_secs() >= 3600 {
                remove_keys.push(key.clone());
                continue;
            }
            if !entry.file_path.exists() {
                remove_keys.push(key.clone());
                continue;
            }
            let current_mtime = std::fs::metadata(&entry.file_path)
                .ok()
                .and_then(|m| m.modified().ok());
            if current_mtime != entry.last_mtime {
                entry.last_mtime = current_mtime;
                if let Some((new_content, preview)) =
                    read_changed_file(&entry.file_path, &entry.content_type)
                {
                    update_database_with_changes(
                        &entry.app_handle,
                        entry.id,
                        &new_content,
                        &preview,
                        &entry.ticket,
                    );
                }
            }
        }
        for key in remove_keys {
            watchers.remove(&key);
        }
    }
}

fn start_file_watcher(
    app_handle: tauri::AppHandle,
    file_path: std::path::PathBuf,
    content_type: String,
    id: i64,
    ticket: dzc_core::api::CaptureTicket,
) {
    let _ = watcher_sender().send(FileWatcherCommand::Watch {
        app_handle,
        file_path,
        content_type,
        id,
        ticket,
    });
}

fn read_changed_file(file_path: &std::path::Path, content_type: &str) -> Option<(String, String)> {
    let mut new_content = String::new();
    let success = if content_type == "image" {
        read_image_file(file_path, &mut new_content)
    } else {
        read_text_file(file_path, &mut new_content)
    };

    if success {
        let preview = generate_preview(&new_content);
        Some((new_content, preview))
    } else {
        None
    }
}

fn read_image_file(file_path: &std::path::Path, new_content: &mut String) -> bool {
    if let Ok(mut f) = std::fs::File::open(file_path) {
        let mut buffer = Vec::new();
        if f.read_to_end(&mut buffer).is_ok() {
            // Preserve animated GIFs
            let is_gif_header = buffer.len() >= 6
                && (buffer.starts_with(b"GIF87a") || buffer.starts_with(b"GIF89a"));
            let is_gif_ext = file_path
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s.eq_ignore_ascii_case("gif"))
                .unwrap_or(false);

            if is_gif_header || is_gif_ext {
                let b64 = general_purpose::STANDARD.encode(&buffer);
                *new_content = format!("data:image/gif;base64,{}", b64);
                return true;
            }

            if let Ok(png) = crate::services::image_png::as_png_bytes(&buffer) {
                let b64 = general_purpose::STANDARD.encode(&png);
                *new_content = format!("data:image/png;base64,{}", b64);
                return true;
            }
        }
    }
    false
}

fn read_text_file(file_path: &std::path::Path, new_content: &mut String) -> bool {
    if let Ok(mut f) = std::fs::File::open(file_path) {
        f.read_to_string(new_content).is_ok()
    } else {
        false
    }
}

fn generate_preview(content: &str) -> String {
    if content.starts_with("data:image") {
        "[New Image]".to_string()
    } else if content.chars().count() > 500 {
        let preview_text: String = content.chars().take(497).collect();
        format!("{}...", preview_text.replace('\n', " "))
    } else {
        content.replace('\n', " ")
    }
}

fn update_database_with_changes(
    app_handle: &tauri::AppHandle,
    id: i64,
    new_content: &str,
    preview: &str,
    ticket: &dzc_core::api::CaptureTicket,
) {
    use crate::app_state::{AppDataDir, SessionHistory};
    use dzc_core::capture::CaptureStore;
    let history = app_handle.state::<DbState>().history_service();
    let now = chrono::Utc::now().timestamp_millis();
    if id > 0 {
        if history
            .update_content_authorized(id, new_content, preview, ticket, now)
            .is_err()
        {
            return;
        }
    } else {
        let directory = app_handle
            .state::<AppDataDir>()
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let store = history.capture_store(&directory);
        let mut updated=false;
        let mut update=|| {
            let session=app_handle.state::<SessionHistory>();
            let Ok(mut entries)=session.0.lock() else {return;};
            let Some(entry)=entries.iter_mut().find(|entry|entry.id==id) else {return;};
            entry.content=new_content.to_owned();
            entry.preview=preview.to_owned();
            entry.html_content=None;
            updated=true;
        };
        if store.with_capture_guard(Some(ticket),now,&mut update).is_err() || !updated {return;}

    }
    let _ = app_handle.emit("clipboard-changed", id);
}
