use crate::app_state::AppDataDir;
use crate::database::DbState;
use crate::error::{AppError, AppResult};
use tauri::{AppHandle, Manager, State};

#[tauri::command]
pub fn get_data_path(state: State<'_, AppDataDir>) -> AppResult<String> {
    let path = state.0.lock().unwrap();
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn open_data_folder(state: State<'_, AppDataDir>) -> AppResult<()> {
    let path = state.0.lock().unwrap();
    let path_str = path.to_string_lossy().to_string();

    use std::process::Command;

    #[cfg(target_os = "windows")]
    {
        Command::new("explorer")
            .arg(path_str)
            .spawn()
            .map_err(|e| AppError::Internal(format!("Failed to open data folder: {}", e)))?;
    }

    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open")
            .arg(path_str)
            .spawn()
            .map_err(|e| AppError::Internal(format!("Failed to open data folder: {}", e)))?;
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = path_str;
    }

    Ok(())
}

#[tauri::command]
pub fn toggle_autostart(enabled: bool) -> AppResult<()> {
    Ok(dezirclip::infrastructure::login_start::set(&std::env::current_exe()?, enabled)?)
}

#[tauri::command]
pub fn is_autostart_enabled() -> AppResult<bool> {
    Ok(dezirclip::infrastructure::login_start::enabled(&std::env::current_exe()?)?)
}

#[tauri::command]
pub fn set_windows_clipboard_history(enabled: bool) -> AppResult<()> {
    #[cfg(target_os = "windows")]
    crate::infrastructure::windows_api::system_settings::set_clipboard_history(enabled)
        .map_err(|error| {
            AppError::Internal(format!("Could not update Windows clipboard settings: {}", error))
        })?;

    let _ = enabled;
    Ok(())
}

#[tauri::command]
pub fn trigger_registry_win_v_optimization(enable: bool) -> AppResult<bool> {
    #[cfg(target_os = "windows")]
    {
        crate::infrastructure::windows_api::system_settings::set_win_v_optimization(enable)
            .map_err(|error| {
                AppError::Internal(format!("Could not update Windows shortcut settings: {}", error))
            })
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = enable;
        Ok(false)
    }
}

#[tauri::command]
pub fn begin_registry_win_v_change(enable: bool) -> AppResult<(bool, String)> {
    #[cfg(target_os = "windows")]
    {
        crate::infrastructure::windows_api::system_settings::begin_win_v_change(enable)
            .map_err(|error| AppError::Internal(format!("Could not change Windows shortcut settings: {}", error)))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = enable;
        Ok((false, String::new()))
    }
}

#[tauri::command]
pub fn finish_registry_win_v_change(token: String, rollback: bool) -> AppResult<bool> {
    #[cfg(target_os = "windows")]
    {
        crate::infrastructure::windows_api::system_settings::finish_win_v_change(&token, rollback)
            .map_err(|error| AppError::Internal(format!("Could not finish Windows shortcut settings change: {}", error)))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (token, rollback);
        Ok(false)
    }
}

#[tauri::command]
pub fn is_registry_win_v_optimized() -> AppResult<bool> {
    #[cfg(target_os = "windows")]
    {
        crate::infrastructure::windows_api::system_settings::win_v_optimized()
            .map_err(|error| {
                AppError::Internal(format!("Could not read Windows shortcut settings: {}", error))
            })
    }

    #[cfg(not(target_os = "windows"))]
    {
        Ok(false)
    }
}

#[cfg(target_os = "windows")]
pub fn get_registry_win_v_optimized_status() -> bool {
    crate::infrastructure::windows_api::system_settings::win_v_optimized().unwrap_or(false)
}

#[tauri::command]
pub fn restart_explorer() -> AppResult<()> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Command;
        let _ = Command::new("cmd")
            .args(["/C", "taskkill /F /IM explorer.exe & start explorer.exe"])
            .creation_flags(0x08000000)
            .spawn();
    }
    Ok(())
}

#[tauri::command]
pub fn relaunch(app: AppHandle) {
    use std::process::Command;
    if let Ok(exe) = std::env::current_exe() {
        let _ = Command::new(exe).spawn();
    }
    crate::app::app_exit::request_app_exit();
    app.exit(0);
}

#[tauri::command]
pub fn restart_as_admin(app_handle: AppHandle) -> AppResult<()> {
    #[cfg(target_os = "windows")]
    {
        use std::env;
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        let exe_path = env::current_exe().map_err(AppError::from)?;

        let exe_wide: Vec<u16> = OsStr::new(&exe_path)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let runas: Vec<u16> = OsStr::new("runas")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        unsafe {
            let result = ShellExecuteW(
                None,
                PCWSTR::from_raw(runas.as_ptr()),
                PCWSTR::from_raw(exe_wide.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            );

            if result.0 as usize <= 32 {
                return Err(AppError::Internal(
                    "Failed to restart as administrator. User may have cancelled UAC prompt."
                        .to_string(),
                ));
            }
        }

        crate::app::app_exit::request_app_exit();
        app_handle.exit(0);
        Ok(())
    }

    #[cfg(target_os = "linux")]
    {
        use std::process::Command;

        let exe_path = std::env::current_exe().map_err(|e| AppError::Internal(e.to_string()))?;

        match Command::new("pkexec").arg(&exe_path).spawn() {
            Ok(_) => {
                crate::app::app_exit::request_app_exit();
                app_handle.exit(0);
                Ok(())
            }
            Err(_) => Err(AppError::Internal(
                "Elevation requires pkexec or manual launch from terminal".into(),
            )),
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = app_handle;
        Err(AppError::Internal(
            "Restart as admin not supported on this platform".to_string(),
        ))
    }
}

#[tauri::command]
pub fn check_is_admin() -> bool {
    #[cfg(target_os = "windows")]
    {
        use std::ffi::c_void;
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::Security::{
            GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
        };
        use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

        unsafe {
            let mut token_handle = HANDLE::default();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token_handle).is_ok() {
                let mut elevation = TOKEN_ELEVATION::default();
                let mut return_length = 0;
                let success = GetTokenInformation(
                    token_handle,
                    TokenElevation,
                    Some(&mut elevation as *mut _ as *mut c_void),
                    std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                    &mut return_length,
                );

                let _ = windows::Win32::Foundation::CloseHandle(token_handle);

                if success.is_ok() {
                    return elevation.TokenIsElevated != 0;
                }
            }
        }
    }
    false
}

#[tauri::command]
pub fn set_data_path(app_handle: AppHandle, new_path: String) -> AppResult<()> {
    let new_data_path = std::path::Path::new(new_path.trim());
    if !new_data_path.is_dir() {
        return Err(AppError::Validation("Directory does not exist".to_string()));
    }

    let new_data_path = std::path::absolute(new_data_path).map_err(AppError::from)?;
    let old_path_buf = app_handle.state::<AppDataDir>().0.lock().unwrap().clone();
    if old_path_buf.canonicalize().map_err(AppError::from)? == new_data_path.canonicalize().map_err(AppError::from)? {
        return Ok(());
    }
    let executable = std::env::current_exe().map_err(AppError::from)?;
    if cfg!(feature = "portable") || crate::infrastructure::data_paths::portable_data_dir(executable.parent()).is_some() {
        return Err(AppError::Validation("Portable storage uses the data folder beside the executable; move the complete portable directory instead".into()));
    }
    let config_dir = app_handle.path().app_data_dir().map_err(AppError::from)?;
    let redirect_file = config_dir.join("datapath.txt");
    // The live shared connection stays read-only after cutover until relaunch.
    // Other connections cannot commit while the source write reservation is held.
    let db = app_handle.state::<DbState>();
    dzc_core::api::HistoryService::from_legacy_connection(db.conn.clone())
        .migrate_profile(&old_path_buf, &new_data_path, &redirect_file)
        .map_err(AppError::from)
}

#[cfg(all(test, windows, not(feature = "portable")))]
mod tests {
    #[test]
    fn migration_preserves_native_encryption_for_content_and_html() {
        use crate::infrastructure::encryption::{decode_for_mutation, encrypt_sensitive, is_encrypted_value};
        use rusqlite::params;
        dzc_platform::install_default_key_provider();
        let root = std::env::temp_dir().join(format!("dzc-encrypted-migration-{}", uuid::Uuid::new_v4()));
        let old = root.join("old");
        let new = root.join("new");
        std::fs::create_dir_all(old.join("attachments")).unwrap();
        std::fs::create_dir(&new).unwrap();
        let source = old.join("attachments").join("image.png");
        std::fs::write(&source, b"fixture image").unwrap();
        let path = source.to_string_lossy();
        let html = format!("<img src='{path}'>");
        let conn = crate::database::init_db(old.join("clipboard.db").to_str().unwrap()).unwrap();
        conn.execute("INSERT INTO clipboard_history(content_type,content,html_content,source_app,timestamp,preview,is_external) VALUES('image',?1,?2,'fixture',1,'fixture',1)",
            params![encrypt_sensitive(&path).unwrap().into_string(), encrypt_sensitive(&html).unwrap().into_string()]).unwrap();
        drop(conn);
        let history = dzc_core::api::HistoryService::open(&old.join("clipboard.db")).unwrap();
        history.migrate_profile(&old, &new, &root.join("datapath.txt")).unwrap();
        let db = crate::database::init_db(new.join("clipboard.db").to_str().unwrap()).unwrap();
        let (content, html): (String,String) = db.query_row("SELECT content,html_content FROM clipboard_history", [], |r| Ok((r.get(0)?,r.get(1)?))).unwrap();
        assert!(is_encrypted_value(&content) && is_encrypted_value(&html));
        let expected = new.join("attachments").join("image.png");
        assert_eq!(decode_for_mutation(&content).unwrap(), expected.to_string_lossy());
        assert_eq!(decode_for_mutation(&html).unwrap(), format!("<img src='{}'>", expected.display()));
        drop(db);
        drop(history);
        std::fs::remove_dir_all(root).unwrap();
    }
}
