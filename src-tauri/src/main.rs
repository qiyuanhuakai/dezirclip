#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

pub mod app;
pub mod database;
pub mod domain;
pub mod error;
pub mod infrastructure;
pub mod services;
pub mod app_state;
pub mod global_state;
pub mod logger;
pub mod migration;

#[cfg(target_os = "windows")]
use std::sync::atomic::{Ordering};
use crate::global_state::*;
use crate::app::setup;

const APP_IDENTIFIER: &str = "com.tiez.clipboard";
const GPU_SETTING_KEY: &str = "app.disable_webview_gpu";

fn resolve_default_app_data_dir() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA")
            .map(std::path::PathBuf::from)
            .map(|path| path.join(APP_IDENTIFIER))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .map(|path| path.join("Library").join("Application Support").join(APP_IDENTIFIER))
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
            Some(std::path::PathBuf::from(xdg).join(APP_IDENTIFIER))
        } else {
            std::env::var_os("HOME")
                .map(std::path::PathBuf::from)
                .map(|path| path.join(".local").join("share").join(APP_IDENTIFIER))
        }
    }
}

fn resolve_startup_db_path() -> Option<std::path::PathBuf> {
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let portable_db = exe_dir.join("data").join("clipboard.db");
            if portable_db.exists() {
                return Some(portable_db);
            }
        }
    }
    let default_dir = resolve_default_app_data_dir()?;
    let redirect_file = default_dir.join("datapath.txt");
    let app_dir = if redirect_file.exists() {
        if let Ok(content) = std::fs::read_to_string(&redirect_file) {
            let custom = content.trim();
            if !custom.is_empty() {
                let custom_path = std::path::PathBuf::from(custom);
                if custom_path.exists() {
                    custom_path
                } else {
                    default_dir.clone()
                }
            } else {
                default_dir.clone()
            }
        } else {
            default_dir.clone()
        }
    } else {
        default_dir
    };
    Some(app_dir.join("clipboard.db"))
}

fn read_disable_webview_gpu_setting() -> bool {
    let db_path = match resolve_startup_db_path() {
        Some(path) if path.exists() => path,
        _ => return false,
    };
    let conn = match rusqlite::Connection::open(db_path) {
        Ok(conn) => conn,
        Err(_) => return false,
    };
    let mut stmt = match conn.prepare("SELECT value FROM settings WHERE key = ?1 LIMIT 1") {
        Ok(stmt) => stmt,
        Err(_) => return false,
    };
    let value = stmt
        .query_row([GPU_SETTING_KEY], |row| row.get::<_, String>(0))
        .ok();
    matches!(value.as_deref(), Some("true") | Some("1") | Some("TRUE") | Some("True"))
}

fn should_disable_webview_gpu() -> bool {
    read_disable_webview_gpu_setting()
}

fn apply_webview2_gpu_switch() {
    if !should_disable_webview_gpu() {
        return;
    }
    let key = "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS";
    let extra = "--disable-gpu";
    let merged = match std::env::var(key) {
        Ok(existing) if !existing.trim().is_empty() => {
            if existing.contains(extra) {
                existing
            } else {
                format!("{existing} {extra}")
            }
        }
        _ => extra.to_string(),
    };
    std::env::set_var(key, merged);
}

fn main() {
    let _ = dotenvy::dotenv();
    apply_webview2_gpu_switch();

    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().with_handler(|app, shortcut, event| {
            if event.state() == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                crate::info!("[global-shortcut] Handler invoked: {:?}", shortcut);
                setup::handle_global_shortcut(app, shortcut);
            }
        }).build())
        .plugin(tauri_plugin_window_state::Builder::default().build());

    if !cfg!(debug_assertions) {
        builder = builder.plugin(tauri_plugin_single_instance::init(|_app, _args, _cwd| {}));
    }

    let app = builder
        .setup(|app| {
            setup::init(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app::window_manager::toggle_window_cmd,
            app::window_manager::hide_window_cmd,
            app::window_manager::activate_window_focus,
            app::window_manager::focus_clipboard_window,
            app::window_manager::set_navigation_enabled,
            app::window_manager::set_navigation_mode,
            app::hooks::set_recording_mode,
            
            services::content_handler::open_content,
            services::clipboard_ops::copy_to_clipboard,
            services::clipboard_ops::paste_latest_rich,
            
            app::commands::get_clipboard_history,
            app::commands::search_clipboard_history,
            app::commands::delete_clipboard_entry,
            app::commands::clear_clipboard_history,
            app::commands::get_tag_items,
            app::commands::get_all_tags_info,
            app::commands::rename_tag_globally,
            app::commands::delete_tag_from_all,
            app::commands::create_new_tag,
            app::commands::update_pinned_order,
            app::commands::get_db_count,
            app::commands::get_clipboard_content,
            
            app::commands::set_sequential_mode,
            app::commands::set_sequential_hotkey,
            app::commands::set_rich_paste_hotkey,
            app::commands::set_search_hotkey,
            app::commands::set_deduplication,
            app::commands::save_setting,
            app::commands::set_ignore_blur,
            app::commands::set_window_pinned,
            app::commands::get_settings,
            app::commands::set_persistence,
            app::commands::set_capture_files,
            app::commands::set_capture_rich_text,
            app::commands::set_silent_start,
            app::commands::set_delete_after_paste,
            app::commands::set_privacy_protection,
            app::commands::set_privacy_protection_kinds,
            app::commands::set_privacy_protection_custom_rules,
            app::commands::reset_settings,
            app::commands::set_sound_enabled,
            app::commands::set_arrow_key_selection,
            app::commands::set_tray_visible,
            app::commands::set_edge_docking,
            app::commands::set_follow_mouse,

            app::commands::get_data_path,
            app::commands::open_data_folder,
            app::commands::set_data_path,
            app::commands::toggle_autostart,
            app::commands::is_autostart_enabled,
            app::commands::set_windows_clipboard_history,
            app::commands::trigger_registry_win_v_optimization,
            app::commands::is_registry_win_v_optimized,
            app::commands::restart_explorer,
            app::commands::restart_as_admin,
            app::commands::check_is_admin,
            app::commands::relaunch,

            app::commands::set_theme,
            app::commands::get_platform_info,
            app::commands::register_hotkey,
            app::commands::test_hotkey_available,

            app::commands::toggle_clipboard_pin,
            app::commands::update_tags,
            app::commands::add_manual_item,
            app::commands::update_item_content,

            app::commands::save_emoji_favorite,
            app::commands::remove_emoji_favorite,
            app::commands::list_emoji_favorites,
            app::commands::save_emoji_favorite_data_url,
            app::commands::get_file_size,
            
            services::paste_queue::get_paste_queue,
            services::paste_queue::set_paste_queue,
            services::paste_queue::paste_next_step,
            
            app::commands::get_tag_colors,
            app::commands::set_tag_color,
            
            #[cfg(target_os = "windows")]
            infrastructure::windows_api::apps::get_system_default_app,
            #[cfg(target_os = "windows")]
            infrastructure::windows_api::apps::get_executable_icon,
            #[cfg(target_os = "windows")]
            infrastructure::windows_api::apps::scan_installed_apps,
            #[cfg(target_os = "windows")]
            infrastructure::windows_api::apps::get_associated_apps
        ])
        .on_window_event(|window, event| {
            setup::handle_window_event(window, event);
        })
        .build(tauri::generate_context!());

    match app {
        Ok(app) => {
            info!(">>> [STARTUP] Tauri app built successfully.");
            app.run(|_app_handle, _event| {});
        },
        Err(e) => {
             error!(">>> [STARTUP] Failed to build tauri app: {}", e);
        }
    }

    // Cleanup Hooks on exit
    #[cfg(target_os = "windows")]
    unsafe {
        let h_hook = HOOK_HANDLE.swap(std::ptr::null_mut(), Ordering::SeqCst);
        if !h_hook.is_null() {
            let _ = windows::Win32::UI::WindowsAndMessaging::UnhookWindowsHookEx(windows::Win32::UI::WindowsAndMessaging::HHOOK(h_hook as _));
        }
        let h_mouse = HOOK_MOUSE_HANDLE.swap(std::ptr::null_mut(), Ordering::SeqCst);
        if !h_mouse.is_null() {
            let _ = windows::Win32::UI::WindowsAndMessaging::UnhookWindowsHookEx(windows::Win32::UI::WindowsAndMessaging::HHOOK(h_mouse as _));
        }
    }
}
