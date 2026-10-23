// Content handler module for opening various content types
use crate::database::DbState;
use crate::error::AppError;
use crate::infrastructure::repository::settings_repo::SettingsRepository;
#[cfg(target_os = "windows")]
use crate::infrastructure::windows_api::apps::launch_uwp_with_file;
use base64::{engine::general_purpose, Engine as _};
use std::collections::HashMap;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::{mpsc, OnceLock};
use tauri::{Emitter, Manager, State};

fn edit_watch_ticket(
    history:&dzc_core::api::HistoryService,
    permit:&dzc_core::api::PrivacyReadPermit,
    id:i64,
    content_type:&str,
    now:i64,
)->Result<Option<dzc_core::api::CaptureTicket>,dzc_core::api::PrivacyError> {
    history.privacy().check_read(permit,now)?;
    if id<=0 || !matches!(content_type,"image"|"text"|"rich_text"|"code") {return Ok(None);}
    match history.privacy().capture_ticket(now) {
        Ok(ticket) if ticket.epoch()==permit.epoch()=>Ok(Some(ticket)),
        Ok(_)=>Err(dzc_core::api::PrivacyError::PrivacyEpochChanged),
        Err(dzc_core::api::PrivacyError::PrivacySessionActive)=>Ok(None),
        Err(error)=>Err(error),
    }
}

fn complete_external_open(
    history:&dzc_core::api::HistoryService,
    id:i64,
    permit:&dzc_core::api::PrivacyReadPermit,
    launched:Result<(),AppError>,
    now:i64,
)->Result<(),AppError> {
    launched?;
    history.privacy().check_read(permit,now).map_err(|error|AppError::Validation(error.to_string()))?;
    if id>0 {history.increment_use_count(id).map_err(|error|AppError::Database(error.to_string()))?;}
    Ok(())
}

#[tauri::command]
pub async fn open_content(
    app_handle: tauri::AppHandle,
    state: State<'_, DbState>,
    id: i64,
    content: String,
    content_type: String,
    expected_privacy_epoch: Option<u64>,
    expected_host_instance_id:Option<String>,
) -> Result<(), AppError> {
    if id<0 {
        app_handle.state::<crate::app::ipc_host::DesktopHost>().check_instance(id,expected_host_instance_id.as_deref()).map_err(AppError::Validation)?;
        return Err(AppError::Validation("SessionExternalOpenUnavailable".into()));
    }
    use dzc_core::api::{PrivacyAccessIntent, SensitiveAccess};
    let history = state.history_service();
    let now = || chrono::Utc::now().timestamp_millis();
    let sensitive = if expected_privacy_epoch.is_some() { SensitiveAccess::ExplicitUserAction } else { SensitiveAccess::Denied };
    let permit = if id > 0 {
        history.privacy().authorize_selected_copy(id, None, sensitive, expected_privacy_epoch, now())
    } else {
        history.privacy().authorize_read(PrivacyAccessIntent::GeneratedCopy, SensitiveAccess::Denied, expected_privacy_epoch, now())
    }
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
        serde_json::from_value(serde_json::json!({"id":0,"content_type":content_type,"content":content,"preview":"","source_app":"Manual","timestamp":now(),"is_pinned":false})).map_err(|_|AppError::Validation("PrivateExportRejected".into()))?
    };
    let content_type = entry.content_type.clone();
    let app_path = get_app_path_for_content_type(&state, &content_type)?;
    if matches!(content_type.as_str(), "link" | "url") {
        history
            .privacy()
            .check_read(&permit, now())
            .map_err(|error| AppError::Validation(error.to_string()))?;
        let launched=handle_url_content(&app_path, &entry.content).await;
        return complete_external_open(&history,id,&permit,launched,now());
    }
    let ticket=edit_watch_ticket(&history,&permit,id,&content_type,now())
        .map_err(|error|AppError::Validation(error.to_string()))?;
    let export = if id > 0 {
        history.external_files_authorized(id, &permit, now())
    } else {
        history.generated_external_files_authorized(&entry, &permit, now())
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
    let launched=launch_file_with_app(&app_path, &file_path, &path, &content_type, false).await;
    complete_external_open(&history,id,&permit,launched,now())?;
    if let Some(ticket)=ticket {
        start_file_watcher(app_handle, file_path, lease, content_type, id, ticket);
    } else {
        lease.retain();
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
    lease: dzc_platform::runtime::PrivateFileExport,
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
        lease: dzc_platform::runtime::PrivateFileExport,
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
                lease,
                content_type,
                id,
                ticket,
            }) => {
                let key = file_path.to_string_lossy().to_string();
                let last_mtime = lease.modified(0).ok();
                watchers.insert(
                    key,
                    WatchEntry {
                        app_handle,
                        lease,
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
            if entry.start_time.elapsed().as_secs() >= 300 {
                remove_keys.push(key.clone());
                continue;
            }
            if !entry.lease.is_current() {
                remove_keys.push(key.clone());
                continue;
            }
            let current_mtime = entry.lease.modified(0).ok();
            if current_mtime != entry.last_mtime {
                entry.last_mtime = current_mtime;
                if let Some((new_content, preview)) = entry
                    .lease
                    .read(0)
                    .ok()
                    .and_then(|bytes| changed_bytes(&bytes, &entry.content_type))
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
    lease: dzc_platform::runtime::PrivateFileExport,
    content_type: String,
    id: i64,
    ticket: dzc_core::api::CaptureTicket,
) {
    let _ = watcher_sender().send(FileWatcherCommand::Watch {
        app_handle,
        file_path,
        lease,
        content_type,
        id,
        ticket,
    });
}

fn changed_bytes(bytes: &[u8], content_type: &str) -> Option<(String, String)> {
    let content = if content_type == "image" {
        if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
            format!(
                "data:image/gif;base64,{}",
                general_purpose::STANDARD.encode(bytes)
            )
        } else {
            let png = crate::services::image_png::as_png_bytes(bytes).ok()?;
            format!(
                "data:image/png;base64,{}",
                general_purpose::STANDARD.encode(png)
            )
        }
    } else {
        String::from_utf8(bytes.to_vec()).ok()?
    };
    let preview = generate_preview(&content);
    Some((content, preview))
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
        let mut updated = false;
        let mut update = || {
            let session = app_handle.state::<SessionHistory>();
            let Ok(mut entries) = session.0.lock() else {
                return;
            };
            let Some(entry) = entries.iter_mut().find(|entry| entry.id == id) else {
                return;
            };
            entry.content = new_content.to_owned();
            entry.preview = preview.to_owned();
            entry.html_content = None;
            updated = true;
        };
        if store
            .with_capture_guard(Some(ticket), now, &mut update)
            .is_err()
            || !updated
        {
            return;
        }
    }
    let _ = app_handle.emit("clipboard-changed", id);
}

#[cfg(test)]
mod authorization_tests {
    use super::*;
    use std::sync::{Arc,Mutex};

    fn fixture()->(dzc_core::api::HistoryService,Arc<Mutex<rusqlite::Connection>>) {
        let connection=crate::database::init_db(":memory:").unwrap();
        connection.execute("INSERT INTO clipboard_history(id,content_type,content,preview,tags,source_app,timestamp,is_pinned) VALUES(1,'url','https://example.invalid/','synthetic','[]','Synthetic',1,0)",[]).unwrap();
        let connection=Arc::new(Mutex::new(connection));
        (dzc_core::api::HistoryService::from_legacy_connection(connection.clone()),connection)
    }

    fn permit(history:&dzc_core::api::HistoryService,now:i64)->dzc_core::api::PrivacyReadPermit {
        history.privacy().authorize_selected_copy(1,None,dzc_core::api::SensitiveAccess::Denied,None,now).unwrap()
    }

    fn uses(connection:&Arc<Mutex<rusqlite::Connection>>)->i32 {
        connection.lock().unwrap().query_row("SELECT use_count FROM clipboard_history WHERE id=1",[],|row|row.get(0)).unwrap()
    }

    #[test]
    fn paused_recording_allows_read_only_open_without_an_edit_write_ticket() {
        let (history,_)=fixture();
        history.privacy().toggle(10).unwrap();
        let permit=permit(&history,11);
        assert!(history.get_entry_authorized(1,&permit,11).unwrap().is_some());
        for kind in ["url","files","video","image","text","rich_text","code"] {
            assert!(edit_watch_ticket(&history,&permit,1,kind,11).unwrap().is_none(),"{kind}");
        }
        history.privacy().toggle(12).unwrap();
        assert_eq!(edit_watch_ticket(&history,&permit,1,"text",13).unwrap_err(),dzc_core::api::PrivacyError::PrivacyEpochChanged);
        let current=self::permit(&history,13);
        assert_eq!(edit_watch_ticket(&history,&current,1,"text",13).unwrap().unwrap().epoch(),current.epoch());
        assert!(edit_watch_ticket(&history,&current,1,"files",13).unwrap().is_none());
    }

    #[test]
    fn only_successful_current_persistent_opens_increment_use_count_once() {
        let (history,connection)=fixture();
        let permit=permit(&history,10);
        assert!(complete_external_open(&history,1,&permit,Err(AppError::IO("synthetic launch failure".into())),10).is_err());
        assert_eq!(uses(&connection),0);
        complete_external_open(&history,0,&permit,Ok(()),10).unwrap();
        assert_eq!(uses(&connection),0);
        complete_external_open(&history,1,&permit,Ok(()),10).unwrap();
        assert_eq!(uses(&connection),1);
        history.privacy().toggle(11).unwrap();
        assert!(complete_external_open(&history,1,&permit,Ok(()),12).is_err());
        assert_eq!(uses(&connection),1);
    }
}
