use crate::app_state::AppDataDir;
use crate::database::DbState;
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
    let mut conn = db.conn.lock().map_err(|_| AppError::Internal("Database lock poisoned".into()))?;
    migrate_with_connection(&mut conn, &old_path_buf, &new_data_path, &redirect_file)
}

fn migrate_with_connection(
    conn: &mut Connection,
    old_base: &std::path::Path,
    new_base: &std::path::Path,
    redirect_file: &std::path::Path,
) -> AppResult<()> {
    if conn.pragma_query_value(None, "query_only", |row| row.get::<_, bool>(0)).map_err(AppError::from)? {
        return Err(AppError::Validation("Data migration is complete; restart the application".into()));
    }
    let transaction = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(AppError::from)?;
    transaction.pragma_update(None, "query_only", true).map_err(AppError::from)?;
    let result = migrate_data_directory(old_base, new_base, redirect_file);
    transaction.rollback().map_err(AppError::from)?;
    if result.is_err() {
        conn.pragma_update(None, "query_only", false).map_err(AppError::from)?;
    }
    result
}

fn migrate_data_directory(
    old_base: &std::path::Path,
    new_base: &std::path::Path,
    redirect_file: &std::path::Path,
) -> AppResult<()> {
    let old_canonical = old_base.canonicalize().map_err(AppError::from)?;
    let new_canonical = new_base.canonicalize().map_err(AppError::from)?;
    if new_canonical.starts_with(&old_canonical) || old_canonical.starts_with(&new_canonical) {
        return Err(AppError::Validation("Data directories must not contain one another".into()));
    }
    let names = ["clipboard.db", "attachments", "emoji_favorites"];
    for name in names.into_iter().chain(["clipboard.db-wal", "clipboard.db-shm"]) {
        if new_base.join(name).symlink_metadata().is_ok() {
            return Err(AppError::Validation(format!("Destination already contains {name}; choose an empty data directory")));
        }
    }
    let stage = new_base.join(format!(".dezirclip-migration-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&stage).map_err(AppError::from)?;
    let result = (|| {
        // SQLite reads committed WAL pages into a standalone snapshot. Never copy
        // a live DB/WAL/SHM file set with filesystem operations.
        let source = Connection::open_with_flags(old_base.join("clipboard.db"), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(AppError::from)?;
        crate::infrastructure::repository::migrations::register_privacy_functions(&source).map_err(AppError::from)?;
        let staged_db = stage.join("clipboard.db");
        source.execute("VACUUM main INTO ?1", [staged_db.to_string_lossy().as_ref()]).map_err(AppError::from)?;
        drop(source);
        let updates = prepare_attachment_path_rewrites(&staged_db, old_base, new_base)?;
        apply_attachment_path_rewrites(&staged_db, &updates)?;
        rewrite_emoji_favorites_in_db(&staged_db, old_base, new_base)?;
        // Close every staging connection and consolidate its own WAL before
        // publishing the DB. The source remains untouched on every error path.
        let snapshot = Connection::open(&staged_db).map_err(AppError::from)?;
        let mode: String = snapshot.query_row("PRAGMA journal_mode=DELETE", [], |row| row.get(0)).map_err(AppError::from)?;
        if !mode.eq_ignore_ascii_case("delete") {
            return Err(AppError::Internal("Could not finalize migration snapshot".into()));
        }
        drop(snapshot);
        for folder in ["attachments", "emoji_favorites"] {
            let source = old_base.join(folder);
            if source.exists() {
                copy_dir_recursive(&source, &stage.join(folder)).map_err(AppError::from)?;
            }
        }
        for name in names {
            let source = stage.join(name);
            if source.is_dir() {
                copy_dir_recursive(&source, &new_base.join(name)).map_err(AppError::from)?;
            } else if source.is_file() {
                copy_file_new(&source, &new_base.join(name)).map_err(AppError::from)?;
            }
        }
        crate::infrastructure::attachments::write_backup(redirect_file, new_base.to_string_lossy().as_bytes())
            .map_err(AppError::Internal)
    })();
    // This is the unique staging folder created above, never a user path.
    let _ = std::fs::remove_dir_all(&stage);
    result
}

fn copy_file_new(source: &std::path::Path, destination: &std::path::Path) -> std::io::Result<()> {
    let mut input = crate::infrastructure::local_file::open_regular_nofollow(source)?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut output = options.open(destination)?;
    std::io::copy(&mut input, &mut output)?;
    output.sync_all()
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
    crate::infrastructure::repository::migrations::configure_privacy_writer(&conn).map_err(AppError::from)?;
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
    if src.symlink_metadata()?.is_symlink() {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Data migration does not follow symbolic links"));
    }
    std::fs::create_dir(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if ty.is_symlink() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Data migration does not follow symbolic links"));
        }
        if ty.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if ty.is_file() {
            copy_file_new(&src_path, &dst_path)?;
        } else {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Data migration only copies regular files"));
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
        conn.execute_batch("CREATE TABLE clipboard_history(id INTEGER PRIMARY KEY, content TEXT NOT NULL, html_content TEXT, is_external INTEGER NOT NULL); CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT);").unwrap();
        (directory, path)
    }

    fn migration_fixture() -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf, Connection) {
        let (root, _) = database();
        let old = root.join("old");
        let new = root.join("new");
        std::fs::create_dir(&old).unwrap();
        std::fs::create_dir(&new).unwrap();
        std::fs::rename(root.join("clipboard.db"), old.join("clipboard.db")).unwrap();
        let conn = Connection::open(old.join("clipboard.db")).unwrap();
        conn.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
        std::fs::create_dir(old.join("attachments")).unwrap();
        std::fs::write(old.join("attachments/image.png"), b"fixture image").unwrap();
        let image = old.join("attachments/image.png").to_string_lossy().to_string();
        conn.execute("INSERT INTO clipboard_history VALUES(1,?1,NULL,1)", [&image]).unwrap();
        (root, old, new, conn)
    }

    #[test]
    fn directory_migration_preserves_schema19_filtered_fts_and_rewrites_html() {
        let (root, old, new, mut conn) = migration_fixture();
        conn.execute_batch("DROP TABLE clipboard_history; DROP TABLE settings;").unwrap();
        crate::infrastructure::repository::migrations::run_migrations(&mut conn).unwrap();
        let original = old.join("attachments/image.png").to_string_lossy().to_string();
        let html = format!("<p>ordinaryprobe</p><img src='{original}'>");
        conn.execute("INSERT INTO clipboard_history(id,content_type,content,html_content,source_app,timestamp,preview,tags) VALUES(1,'rich_text','ordinaryprobe',?1,'test',1,'ordinaryprobe','[]')", [&html]).unwrap();
        migrate_with_connection(&mut conn, &old, &new, &root.join("datapath.txt")).unwrap();
        let migrated = crate::database::init_db(new.join("clipboard.db").to_str().unwrap()).unwrap();
        let html: String = migrated.query_row("SELECT html_content FROM clipboard_history WHERE id=1", [], |row| row.get(0)).unwrap();
        assert!(html.contains(new.join("attachments/image.png").to_string_lossy().as_ref()));
        assert_eq!(migrated.query_row("SELECT count(*) FROM clipboard_fts WHERE clipboard_fts MATCH 'ordinaryprobe'", [], |row| row.get::<_, i64>(0)).unwrap(), 1);
        migrated.execute("INSERT INTO clipboard_fts(clipboard_fts,rank) VALUES('integrity-check',1)", []).unwrap();
        assert!(old.join("attachments/image.png").is_file());
        drop(migrated); drop(conn);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn directory_migration_sql_rejection_keeps_live_source_files_and_redirect_usable() {
        let (root, old, new, mut conn) = migration_fixture();
        conn.execute_batch("CREATE TRIGGER reject_rewrite BEFORE UPDATE ON clipboard_history BEGIN SELECT RAISE(ABORT,'fixture rejection'); END;").unwrap();
        let redirect = root.join("datapath.txt");
        std::fs::write(&redirect, old.to_string_lossy().as_bytes()).unwrap();
        assert!(migrate_with_connection(&mut conn, &old, &new, &redirect).is_err());
        assert_eq!(std::fs::read_to_string(&redirect).unwrap(), old.to_string_lossy());
        assert_eq!(std::fs::read(old.join("attachments/image.png")).unwrap(), b"fixture image");
        assert!(old.join("clipboard.db").exists());
        assert!(!new.join("clipboard.db").exists());
        conn.execute("INSERT INTO clipboard_history VALUES(2,'still writable',NULL,0)", []).unwrap();
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM clipboard_history", [], |row| row.get::<_, i64>(0)).unwrap(), 2);
        drop(conn);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn directory_migration_snapshots_committed_wal_and_preserves_source_until_restart() {
        let (root, old, new, mut conn) = migration_fixture();
        let second = Connection::open(old.join("clipboard.db")).unwrap();
        second.execute("INSERT INTO clipboard_history VALUES(2,'concurrent committed row',NULL,0)", []).unwrap();
        let redirect = root.join("datapath.txt");
        migrate_with_connection(&mut conn, &old, &new, &redirect).unwrap();
        assert_eq!(std::fs::read_to_string(&redirect).unwrap(), new.to_string_lossy());
        let migrated = Connection::open(new.join("clipboard.db")).unwrap();
        let expected_new = new.join("attachments/image.png").to_string_lossy().to_string();
        let expected_old = old.join("attachments/image.png").to_string_lossy().to_string();
        assert_eq!(migrated.query_row("SELECT content FROM clipboard_history WHERE id=1", [], |row| row.get::<_, String>(0)).unwrap(), expected_new);
        assert_eq!(conn.query_row("SELECT content FROM clipboard_history WHERE id=1", [], |row| row.get::<_, String>(0)).unwrap(), expected_old);
        assert_eq!(migrated.query_row("SELECT content FROM clipboard_history WHERE id=2", [], |row| row.get::<_, String>(0)).unwrap(), "concurrent committed row");
        assert_eq!(std::fs::read(old.join("attachments/image.png")).unwrap(), std::fs::read(new.join("attachments/image.png")).unwrap());
        assert!(conn.execute("INSERT INTO clipboard_history VALUES(3,'blocked until relaunch',NULL,0)", []).is_err());
        assert!(migrate_with_connection(&mut conn, &old, &new, &redirect).is_err());
        assert!(conn.pragma_query_value(None, "query_only", |row| row.get::<_, bool>(0)).unwrap());
        drop(migrated);
        drop(second);
        drop(conn);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn directory_migration_redirect_failure_retains_source_and_restores_writes() {
        let (root, old, new, mut conn) = migration_fixture();
        let redirect = root.join("datapath.txt");
        std::fs::create_dir(&redirect).unwrap();
        std::fs::write(redirect.join("keep"), b"existing configuration obstacle").unwrap();
        assert!(migrate_with_connection(&mut conn, &old, &new, &redirect).is_err());
        assert_eq!(std::fs::read(redirect.join("keep")).unwrap(), b"existing configuration obstacle");
        assert!(old.join("clipboard.db").exists());
        assert!(old.join("attachments/image.png").exists());
        conn.execute("INSERT INTO clipboard_history VALUES(2,'still writable',NULL,0)", []).unwrap();
        drop(conn);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn directory_migration_never_overwrites_existing_destination_data() {
        let (root, old, new, mut conn) = migration_fixture();
        std::fs::write(new.join("clipboard.db"), b"existing target history").unwrap();
        let redirect = root.join("datapath.txt");
        std::fs::write(&redirect, old.to_string_lossy().as_bytes()).unwrap();
        assert!(migrate_with_connection(&mut conn, &old, &new, &redirect).is_err());
        assert_eq!(std::fs::read(new.join("clipboard.db")).unwrap(), b"existing target history");
        assert_eq!(std::fs::read_to_string(&redirect).unwrap(), old.to_string_lossy());
        assert!(old.join("clipboard.db").exists());
        drop(conn);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn directory_migration_source_reservation_blocks_other_writers_through_snapshot() {
        let (root, old, new, mut conn) = migration_fixture();
        let writer = Connection::open(old.join("clipboard.db")).unwrap();
        writer.busy_timeout(std::time::Duration::ZERO).unwrap();
        let transaction = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).unwrap();
        assert!(writer.execute("INSERT INTO clipboard_history VALUES(2,'blocked',NULL,0)", []).is_err());
        migrate_data_directory(&old, &new, &root.join("datapath.txt")).unwrap();
        assert!(writer.execute("INSERT INTO clipboard_history VALUES(2,'blocked',NULL,0)", []).is_err());
        transaction.rollback().unwrap();
        drop(writer);
        drop(conn);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn directory_migration_fifo_rejection_returns_promptly_and_releases_source_write_lock() {
        let (root, old, new, mut conn) = migration_fixture();
        assert!(std::process::Command::new("mkfifo").arg(old.join("attachments/blocked.png")).status().unwrap().success());
        let redirect = root.join("datapath.txt");
        std::fs::write(&redirect, old.to_string_lossy().as_bytes()).unwrap();
        let writer = Connection::open(old.join("clipboard.db")).unwrap();
        writer.busy_timeout(std::time::Duration::ZERO).unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        let old_thread = old.clone();
        let redirect_thread = redirect.clone();
        let worker = std::thread::spawn(move || {
            assert!(migrate_with_connection(&mut conn, &old_thread, &new, &redirect_thread).is_err());
            assert!(conn.is_autocommit());
            assert!(!conn.pragma_query_value(None, "query_only", |row| row.get::<_, bool>(0)).unwrap());
            sender.send(()).unwrap();
        });
        receiver.recv_timeout(std::time::Duration::from_secs(5)).expect("FIFO input must be rejected without waiting for a writer");
        worker.join().unwrap();
        writer.execute("INSERT INTO clipboard_history VALUES(2,'source remains writable',NULL,0)", []).unwrap();
        assert_eq!(std::fs::read_to_string(&redirect).unwrap(), old.to_string_lossy());
        assert_eq!(std::fs::read(old.join("attachments/image.png")).unwrap(), b"fixture image");
        drop(writer);
        std::fs::remove_dir_all(root).unwrap();
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
