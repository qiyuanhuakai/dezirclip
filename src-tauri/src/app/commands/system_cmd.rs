use crate::app_state::AppDataDir;
use crate::infrastructure::encryption::{rewrite_stored_text, EncryptionError};
use crate::error::{AppError, AppResult};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json;
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
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::*;
        use winreg::RegKey;

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = hkcu
            .open_subkey_with_flags(
                "Software\\Microsoft\\Windows\\CurrentVersion\\Run",
                KEY_WRITE | KEY_READ,
            )
            .map_err(|e| AppError::Internal(e.to_string()))?;
        let app_path = std::env::current_exe()
            .map_err(|e| AppError::Internal(e.to_string()))?
            .to_string_lossy()
            .to_string();
        let cmd = format!("\"{}\" --minimized", app_path);

        if enabled {
            key.set_value("DezirClip", &cmd)
                .map_err(|e| AppError::Internal(e.to_string()))?;
        } else {
            let _ = key.delete_value("DezirClip");
            let _ = key.delete_value("tie-z");
        }
    }

    #[cfg(target_os = "linux")]
    {
        let app_path = std::env::current_exe().map_err(|e| AppError::Internal(e.to_string()))?;
        let autostart_dir = dirs::config_dir()
            .ok_or_else(|| AppError::Internal("Failed to get config dir".to_string()))?
            .join("autostart");
        std::fs::create_dir_all(&autostart_dir).map_err(|e| AppError::Internal(e.to_string()))?;

        let desktop_file = autostart_dir.join("dezirclip.desktop");
        if enabled {
            let content = format!(
                "[Desktop Entry]\nType=Application\nName=DezirClip\nExec=\"{}\" --minimized\nHidden=false\nNoDisplay=false\nX-GNOME-Autostart-enabled=true\nStartupNotify=false\nStartupWMClass=DezirClip\n",
                app_path.to_string_lossy()
            );
            std::fs::write(&desktop_file, content)
                .map_err(|e| AppError::Internal(e.to_string()))?;
        } else {
            let _ = std::fs::remove_file(&desktop_file);
        }
    }

    let _ = enabled;
    Ok(())
}

#[tauri::command]
pub fn is_autostart_enabled() -> AppResult<bool> {
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::*;
        use winreg::RegKey;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = hkcu
            .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
            .map_err(|e| AppError::Internal(e.to_string()))?;
        Ok(key.get_value::<String, _>("DezirClip").is_ok()
            || key.get_value::<String, _>("tie-z").is_ok())
    }

    #[cfg(target_os = "linux")]
    {
        let autostart_dir = dirs::config_dir()
            .ok_or_else(|| AppError::Internal("Failed to get config dir".to_string()))?
            .join("autostart");
        Ok(autostart_dir.join("dezirclip.desktop").exists())
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        Ok(false)
    }
}

#[tauri::command]
pub fn set_windows_clipboard_history(enabled: bool) -> AppResult<()> {
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::*;
        use winreg::RegKey;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let mut needs_restart = false;

        if let Ok((key, _)) = hkcu.create_subkey("Software\\Microsoft\\Clipboard") {
            let value: u32 = if enabled { 1 } else { 0 };
            let _ = key.set_value("EnableClipboardHistory", &value);
            let _ = key.set_value("EnableCloudClipboard", &value);
        }

        if let Ok((adv_key, _)) =
            hkcu.create_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced")
        {
            let current_disabled: String = adv_key.get_value("DisabledHotkeys").unwrap_or_default();
            if current_disabled.to_uppercase().contains('V') {
                let new_val = current_disabled.to_uppercase().replace('V', "");
                if new_val.is_empty() {
                    let _ = adv_key.delete_value("DisabledHotkeys");
                } else {
                    let _ = adv_key.set_value("DisabledHotkeys", &new_val);
                }
                needs_restart = true;
            }
        }

        if let Ok((policy_key, _)) =
            hkcu.create_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Policies\\Explorer")
        {
            if policy_key
                .get_value::<u32, _>("DisallowClipboardHistory")
                .unwrap_or(0)
                != 0
            {
                let _ = policy_key.delete_value("DisallowClipboardHistory");
                needs_restart = true;
            }
        }

        if enabled {
            if let Ok((sys_policy, _)) =
                hkcu.create_subkey("Software\\Policies\\Microsoft\\Windows\\System")
            {
                if sys_policy
                    .get_value::<u32, _>("AllowClipboardHistory")
                    .unwrap_or(1)
                    == 0
                {
                    let _ = sys_policy.delete_value("AllowClipboardHistory");
                    needs_restart = true;
                }
                if sys_policy
                    .get_value::<u32, _>("AllowCrossDeviceClipboard")
                    .unwrap_or(1)
                    == 0
                {
                    let _ = sys_policy.delete_value("AllowCrossDeviceClipboard");
                    needs_restart = true;
                }
            }
        }

        if needs_restart {
            restart_explorer().ok();
        }
    }

    let _ = enabled;
    Ok(())
}

#[tauri::command]
pub fn trigger_registry_win_v_optimization(enable: bool) -> AppResult<bool> {
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::*;
        use winreg::RegKey;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let mut changed = false;

        if let Ok((adv_key, _)) =
            hkcu.create_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced")
        {
            let current: String = adv_key.get_value("DisabledHotkeys").unwrap_or_default();
            if enable && !current.to_uppercase().contains('V') {
                let _ = adv_key.set_value("DisabledHotkeys", &format!("{}V", current));
                changed = true;
            } else if !enable && current.to_uppercase().contains('V') {
                let clean = current.to_uppercase().replace('V', "");
                if clean.is_empty() {
                    let _ = adv_key.delete_value("DisabledHotkeys");
                } else {
                    let _ = adv_key.set_value("DisabledHotkeys", &clean);
                }
                changed = true;
            }
        }

        if let Ok((cb_key, _)) = hkcu.create_subkey("Software\\Microsoft\\Clipboard") {
            let val: u32 = if enable { 0 } else { 1 };
            let prev_history = cb_key.get_value::<u32, _>("EnableClipboardHistory").ok();
            let prev_cloud = cb_key.get_value::<u32, _>("EnableCloudClipboard").ok();
            let _ = cb_key.set_value("EnableClipboardHistory", &val);
            let _ = cb_key.set_value("EnableCloudClipboard", &val);
            if prev_history != Some(val) || prev_cloud != Some(val) {
                changed = true;
            }
        }

        if !enable {
            if let Ok((policy_key, _)) = hkcu
                .create_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Policies\\Explorer")
            {
                if policy_key
                    .get_value::<u32, _>("DisallowClipboardHistory")
                    .unwrap_or(0)
                    != 0
                {
                    let _ = policy_key.delete_value("DisallowClipboardHistory");
                    changed = true;
                }
            }

            if let Ok((sys_policy, _)) =
                hkcu.create_subkey("Software\\Policies\\Microsoft\\Windows\\System")
            {
                if sys_policy
                    .get_value::<u32, _>("AllowClipboardHistory")
                    .unwrap_or(1)
                    == 0
                {
                    let _ = sys_policy.delete_value("AllowClipboardHistory");
                    changed = true;
                }
                if sys_policy
                    .get_value::<u32, _>("AllowCrossDeviceClipboard")
                    .unwrap_or(1)
                    == 0
                {
                    let _ = sys_policy.delete_value("AllowCrossDeviceClipboard");
                    changed = true;
                }
            }
        }
        Ok(changed)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = enable;
        Ok(false)
    }
}

#[tauri::command]
pub fn is_registry_win_v_optimized() -> AppResult<bool> {
    #[cfg(target_os = "windows")]
    {
        Ok(get_registry_win_v_optimized_status())
    }

    #[cfg(not(target_os = "windows"))]
    {
        Ok(false)
    }
}

#[cfg(target_os = "windows")]
pub fn get_registry_win_v_optimized_status() -> bool {
    use winreg::enums::*;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(key) =
        hkcu.open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced")
    {
        return key
            .get_value::<String, _>("DisabledHotkeys")
            .unwrap_or_default()
            .to_uppercase()
            .contains('V');
    }
    false
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
    let clean_path = new_path.trim().to_string();
    let new_data_path = std::path::Path::new(&clean_path);
    if !new_data_path.exists() {
        return Err(AppError::Validation("Directory does not exist".to_string()));
    }

    let old_path_buf = app_handle.state::<AppDataDir>().0.lock().unwrap().clone();
    if old_path_buf == new_data_path {
        return Ok(());
    }
    // Complete protected-field rewrites before moving any source data.
    let old_db_path = old_path_buf.join("clipboard.db");
    let existing_new_db = new_data_path.join("clipboard.db");
    let rewrite_source = if old_db_path.exists() {
        &old_db_path
    } else {
        &existing_new_db
    };
    let path_rewrites = if rewrite_source.exists() {
        prepare_attachment_path_rewrites(rewrite_source, &old_path_buf, new_data_path)?
    } else {
        Vec::new()
    };

    // 1. Migrate data folders if they exist in the OLD path
    {
        for folder in ["attachments", "emoji_favorites"] {
            let old_folder = old_path_buf.join(folder);
            let new_folder = new_data_path.join(folder);

            if old_folder.exists() && old_folder.is_dir() {
                if let Err(_) = std::fs::rename(&old_folder, &new_folder) {
                    if let Err(copy_err) = copy_dir_recursive(&old_folder, &new_folder) {
                        return Err(AppError::Internal(format!(
                            "Failed to copy {}: {}",
                            folder, copy_err
                        )));
                    } else {
                        let _ = std::fs::remove_dir_all(&old_folder);
                    }
                }
            }
        }

        // 1.2 Migrate database files (main + WAL/SHM)
        let db_files = ["clipboard.db", "clipboard.db-wal", "clipboard.db-shm"];
        for name in db_files {
            let old_db = old_path_buf.join(name);
            if !old_db.exists() {
                continue;
            }
            let new_db = new_data_path.join(name);
            if new_db.exists() {
                let backup = new_data_path.join(format!("{}.backup", name));
                if backup.exists() {
                    let _ = std::fs::remove_file(&backup);
                }
                let _ = std::fs::rename(&new_db, &backup);
            }
            if let Err(_) = std::fs::rename(&old_db, &new_db) {
                if let Err(copy_err) = std::fs::copy(&old_db, &new_db) {
                    return Err(AppError::Internal(format!(
                        "Failed to copy {}: {}",
                        name, copy_err
                    )));
                } else {
                    let _ = std::fs::remove_file(&old_db);
                }
            }
        }
    }

    // 1.3 Rewrite internal attachment paths inside DB (if DB exists in new path)
    let new_db_path = new_data_path.join("clipboard.db");
    if new_db_path.exists() {
        apply_attachment_path_rewrites(&new_db_path, &path_rewrites)?;
        rewrite_emoji_favorites_in_db(&new_db_path, &old_path_buf, new_data_path)?;
    }

    // 2. Save new path to a persistent config file
    let config_dir = app_handle.path().app_data_dir().map_err(AppError::from)?;
    if !config_dir.exists() {
        std::fs::create_dir_all(&config_dir).map_err(AppError::from)?;
    }

    let redirect_file = config_dir.join("datapath.txt");
    std::fs::write(&redirect_file, &clean_path).map_err(AppError::from)?;

    Ok(())
}

#[derive(Debug)]
struct AttachmentPathRewrite {
    id: i64,
    original_content: String,
    original_html: Option<String>,
    content: String,
    html: Option<String>,
}

fn prepare_attachment_path_rewrites(
    db_path: &std::path::Path,
    old_base: &std::path::Path,
    new_base: &std::path::Path,
) -> AppResult<Vec<AttachmentPathRewrite>> {
    let old_attach = old_base.join("attachments");
    let new_attach = new_base.join("attachments");
    let old_prefix = old_attach.to_string_lossy().to_string();
    let new_prefix = new_attach.to_string_lossy().to_string();
    if old_prefix == new_prefix {
        return Ok(Vec::new());
    }

    let old_prefix_slash = old_prefix.replace('\\', "/");
    let new_prefix_slash = new_prefix.replace('\\', "/");

    let conn = Connection::open(db_path).map_err(AppError::from)?;

    let mut stmt = conn
        .prepare("SELECT id, content, html_content FROM clipboard_history WHERE is_external = 1 OR html_content IS NOT NULL")
        .map_err(AppError::from)?;

    let rows = stmt
        .query_map([], |row| {
            let id: i64 = row.get(0)?;
            let content: String = row.get(1)?;
            let html_content: Option<String> = row.get(2)?;
            Ok((id, content, html_content))
        })
        .map_err(AppError::from)?;

    let mut updates = Vec::new();
    for row in rows {
        let (id, content_raw, html_raw) = row.map_err(AppError::from)?;
        let mut content_new: Option<String> = None;
        let mut html_new: Option<String> = None;

        if let Some(updated) = rewrite_content_path(
            &content_raw,
            &old_prefix,
            &new_prefix,
            &old_prefix_slash,
            &new_prefix_slash,
        )
        .map_err(|error| AppError::Internal(error.to_string()))?
        {
            content_new = Some(updated);
        }

        if let Some(html) = html_raw.as_ref() {
            if let Some(updated) = rewrite_html_paths(
                html,
                &old_prefix,
                &new_prefix,
                &old_prefix_slash,
                &new_prefix_slash,
            )
            .map_err(|error| AppError::Internal(error.to_string()))?
            {
                html_new = Some(updated);
            }
        }

        {
            let content_final = content_new.as_ref().unwrap_or(&content_raw);
            let html_final = match html_new.as_ref() {
                Some(v) => Some(v.as_str()),
                None => html_raw.as_deref(),
            };
            let content = content_final.to_owned();
            let html = html_final.map(str::to_owned);
            updates.push(AttachmentPathRewrite {
                id,
                original_content: content_raw,
                original_html: html_raw,
                content,
                html,
            });
        }
    }

    Ok(updates)
}

fn apply_attachment_path_rewrites(
    db_path: &std::path::Path,
    updates: &[AttachmentPathRewrite],
) -> AppResult<()> {
    let mut conn = Connection::open(db_path).map_err(AppError::from)?;
    let transaction = conn.transaction().map_err(AppError::from)?;
    let candidates: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM clipboard_history WHERE is_external = 1 OR html_content IS NOT NULL", [],
        |row| row.get(0),
    ).map_err(AppError::from)?;
    if candidates
        != i64::try_from(updates.len()).map_err(|_| {
            AppError::Internal(EncryptionError::SensitivePersistenceRejected.to_string())
        })?
    {
        return Err(AppError::Internal(
            EncryptionError::SensitivePersistenceRejected.to_string(),
        ));
    }
    for update in updates {
        let changed = transaction.execute(
            "UPDATE clipboard_history SET content = ?1, html_content = ?2 WHERE id = ?3 AND content IS ?4 AND html_content IS ?5",
            params![update.content, update.html, update.id, update.original_content, update.original_html],
        ).map_err(AppError::from)?;
        if changed != 1 {
            return Err(AppError::Internal(
                EncryptionError::SensitivePersistenceRejected.to_string(),
            ));
        }
    }
    transaction.commit().map_err(AppError::from)
}

fn rewrite_emoji_favorites_in_db(
    db_path: &std::path::Path,
    old_base: &std::path::Path,
    new_base: &std::path::Path,
) -> AppResult<()> {
    let old_dir = old_base.join("emoji_favorites");
    let new_dir = new_base.join("emoji_favorites");
    let old_prefix = old_dir.to_string_lossy().to_string();
    let new_prefix = new_dir.to_string_lossy().to_string();
    if old_prefix == new_prefix {
        return Ok(());
    }

    let old_prefix_slash = old_prefix.replace('\\', "/");
    let new_prefix_slash = new_prefix.replace('\\', "/");

    let conn = Connection::open(db_path).map_err(AppError::from)?;
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'app.emoji_favorites'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(AppError::from)?;

    let Some(raw) = value else {
        return Ok(());
    };
    let parsed: Vec<String> = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };

    let mut changed = false;
    let mut updated: Vec<String> = Vec::with_capacity(parsed.len());
    for path in parsed {
        let mut next = path.clone();
        if next.starts_with(&old_prefix) {
            next = format!("{}{}", new_prefix, &next[old_prefix.len()..]);
        } else if next.starts_with(&old_prefix_slash) {
            next = format!("{}{}", new_prefix_slash, &next[old_prefix_slash.len()..]);
        }
        if next != path {
            changed = true;
        }
        updated.push(next);
    }

    if changed {
        let serialized = serde_json::to_string(&updated).unwrap_or(raw);
        conn.execute(
            "UPDATE settings SET value = ?1 WHERE key = 'app.emoji_favorites'",
            params![serialized],
        )
        .map_err(AppError::from)?;
    }

    Ok(())
}

fn rewrite_content_path(
    value: &str,
    old_prefix: &str,
    new_prefix: &str,
    old_prefix_slash: &str,
    new_prefix_slash: &str,
) -> Result<Option<String>, EncryptionError> {
    rewrite_stored_text(value, |plain| {
        if let Some(suffix) = plain.strip_prefix(old_prefix) {
            Some(format!("{new_prefix}{suffix}"))
        } else {
            plain
                .strip_prefix(old_prefix_slash)
                .map(|suffix| format!("{new_prefix_slash}{suffix}"))
        }
    })
}

fn rewrite_html_paths(
    value: &str,
    old_prefix: &str,
    new_prefix: &str,
    old_prefix_slash: &str,
    new_prefix_slash: &str,
) -> Result<Option<String>, EncryptionError> {
    rewrite_stored_text(value, |plain| {
        let updated = plain
            .replace(old_prefix, new_prefix)
            .replace(old_prefix_slash, new_prefix_slash);
        (updated != plain).then_some(updated)
    })
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    if !dst.exists() {
        std::fs::create_dir_all(dst)?;
    }
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod path_rewrite_tests {
    use super::*;
    use base64::Engine;

    fn database() -> (std::path::PathBuf, std::path::PathBuf) {
        let directory =
            std::env::temp_dir().join(format!("dezirclip-path-rewrite-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("clipboard.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE clipboard_history(id INTEGER PRIMARY KEY, content TEXT NOT NULL, html_content TEXT, is_external INTEGER NOT NULL);").unwrap();
        (directory, path)
    }

    #[test]
    fn path_rewrite_preflight_rejects_unavailable_ciphertext_without_changing_source() {
        let (directory, path) = database();
        let cipher = format!(
            "linux:{}",
            base64::engine::general_purpose::STANDARD.encode([0u8; 28])
        );
        let conn = Connection::open(&path).unwrap();
        conn.execute(
            "INSERT INTO clipboard_history VALUES(1,?1,NULL,1)",
            [&cipher],
        )
        .unwrap();
        let before = std::fs::read(&path).unwrap();
        let error = prepare_attachment_path_rewrites(&path, &directory, &directory.join("new"))
            .unwrap_err();
        assert!(error.to_string().contains("ProtectedRecordUnavailable"));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        drop(conn);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn path_rewrite_sql_failure_rolls_back_every_updated_row() {
        let (directory, path) = database();
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("INSERT INTO clipboard_history VALUES(1,'original one',NULL,1),(2,'original two',NULL,1);
            CREATE TRIGGER reject_second BEFORE UPDATE ON clipboard_history WHEN new.id=2 BEGIN SELECT RAISE(ABORT,'rejected'); END;").unwrap();
        let updates: Vec<_> = [
            (1, "original one", "new one"),
            (2, "original two", "new two"),
        ]
        .into_iter()
        .map(|(id, original, updated)| AttachmentPathRewrite {
            id,
            original_content: original.to_string(),
            original_html: None,
            content: updated.to_string(),
            html: None,
        })
        .collect();
        assert!(apply_attachment_path_rewrites(&path, &updates).is_err());
        let values: Vec<String> = conn
            .prepare("SELECT content FROM clipboard_history ORDER BY id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(values, vec!["original one", "original two"]);
        drop(conn);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn path_rewrite_concurrent_edit_preserves_new_values_and_rolls_back_earlier_rows() {
        let (directory, path) = database();
        let first = directory
            .join("attachments")
            .join("one.png")
            .to_string_lossy()
            .to_string();
        let second = directory
            .join("attachments")
            .join("two.png")
            .to_string_lossy()
            .to_string();
        let reader = Connection::open(&path).unwrap();
        reader
            .execute(
                "INSERT INTO clipboard_history VALUES(1,?1,NULL,1),(2,?2,NULL,1)",
                params![first, second],
            )
            .unwrap();
        let updates =
            prepare_attachment_path_rewrites(&path, &directory, &directory.join("new")).unwrap();
        let writer = Connection::open(&path).unwrap();
        writer.execute("UPDATE clipboard_history SET content='concurrent edit', html_content='<p>new HTML</p>' WHERE id=2", []).unwrap();
        assert!(apply_attachment_path_rewrites(&path, &updates).is_err());
        let unchanged: String = reader
            .query_row(
                "SELECT content FROM clipboard_history WHERE id=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let edited: (String, String) = reader
            .query_row(
                "SELECT content,html_content FROM clipboard_history WHERE id=2",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(unchanged, first);
        assert_eq!(
            edited,
            ("concurrent edit".to_string(), "<p>new HTML</p>".to_string())
        );
        drop(writer);
        drop(reader);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn path_rewrite_concurrent_new_attachment_row_rejects_the_prepared_plan() {
        let (directory, path) = database();
        let original = directory
            .join("attachments")
            .join("one.png")
            .to_string_lossy()
            .to_string();
        let reader = Connection::open(&path).unwrap();
        reader
            .execute(
                "INSERT INTO clipboard_history VALUES(1,?1,NULL,1)",
                [&original],
            )
            .unwrap();
        let updates =
            prepare_attachment_path_rewrites(&path, &directory, &directory.join("new")).unwrap();
        let writer = Connection::open(&path).unwrap();
        writer
            .execute(
                "INSERT INTO clipboard_history VALUES(2,'concurrent attachment',NULL,1)",
                [],
            )
            .unwrap();
        assert!(apply_attachment_path_rewrites(&path, &updates).is_err());
        let unchanged: String = reader
            .query_row(
                "SELECT content FROM clipboard_history WHERE id=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(unchanged, original);
        let inserted: String = reader
            .query_row(
                "SELECT content FROM clipboard_history WHERE id=2",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(inserted, "concurrent attachment");
        drop(writer);
        drop(reader);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(all(windows, not(feature = "portable")))]
    #[test]
    fn path_rewrite_retains_encryption_for_content_and_html() {
        dzc_platform::install_default_key_provider();
        let content = crate::infrastructure::encryption::encrypt_sensitive("old/image.png")
            .unwrap()
            .into_string();
        let html =
            crate::infrastructure::encryption::encrypt_sensitive("<img src='old/image.png'>")
                .unwrap()
                .into_string();
        let updated = rewrite_content_path(&content, "old/", "new/", "old/", "new/")
            .unwrap()
            .unwrap();
        assert_eq!(
            crate::infrastructure::encryption::decode_for_mutation(&updated).unwrap(),
            "new/image.png"
        );
        let updated = rewrite_html_paths(&html, "old/", "new/", "old/", "new/")
            .unwrap()
            .unwrap();
        assert_eq!(
            crate::infrastructure::encryption::decode_for_mutation(&updated).unwrap(),
            "<img src='new/image.png'>"
        );
    }
}
