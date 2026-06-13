use crate::app_state::SettingsState;
use crate::database::DbState;
#[cfg(target_os = "windows")]
use crate::error::AppError;
use crate::error::AppResult;
use crate::infrastructure::repository::settings_repo::SettingsRepository;
use serde::Serialize;
use std::process::Command;
use tauri::{Emitter, State, Theme, WebviewWindow};

#[derive(Debug, Serialize)]
pub struct PlatformInfo {
    pub platform: String,
    pub is_windows_10: bool,
    pub is_windows_11: bool,
    pub is_linux: bool,
}

#[tauri::command]
pub fn get_platform_info() -> PlatformInfo {
    #[cfg(target_os = "windows")]
    {
        let build = windows_version::OsVersion::current().build;
        let is_windows_11 = build >= 22000;
        let is_windows_10 = build >= 10240 && build < 22000;
        PlatformInfo {
            platform: "windows".to_string(),
            is_windows_10,
            is_windows_11,
            is_linux: false,
        }
    }

    #[cfg(target_os = "macos")]
    {
        PlatformInfo {
            platform: "macos".to_string(),
            is_windows_10: false,
            is_windows_11: false,
            is_linux: false,
        }
    }

    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        PlatformInfo {
            platform: "other".to_string(),
            is_windows_10: false,
            is_windows_11: false,
            is_linux: true,
        }
    }
}

#[tauri::command]
pub fn get_system_theme_mode() -> String {
    #[cfg(target_os = "linux")]
    {
        if let Ok(output) = Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "color-scheme"])
            .output()
        {
            let value = String::from_utf8_lossy(&output.stdout).to_lowercase();
            if value.contains("dark") {
                return "dark".to_string();
            }
            if value.contains("light") {
                return "light".to_string();
            }
        }

        if let Ok(output) = Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "gtk-theme"])
            .output()
        {
            let value = String::from_utf8_lossy(&output.stdout).to_lowercase();
            if value.contains("dark") {
                return "dark".to_string();
            }
        }
    }

    "system".to_string()
}

#[tauri::command]
pub fn set_theme(
    window: WebviewWindow,
    state: State<'_, SettingsState>,
    db_state: State<'_, DbState>,
    theme: String,
    color_mode: Option<String>,
    show_app_border: Option<bool>,
) -> AppResult<()> {
    let mut effective_color_mode = color_mode.clone();
    if effective_color_mode
        .as_deref()
        .map(|v| v.trim().is_empty())
        .unwrap_or(true)
    {
        effective_color_mode = db_state
            .settings_repo
            .get("app.color_mode")
            .unwrap_or(Some("system".to_string()));
    }
    let mut effective_show_app_border = show_app_border;
    if effective_show_app_border.is_none() {
        effective_show_app_border = db_state
            .settings_repo
            .get("app.show_app_border")
            .unwrap_or(Some("true".to_string()))
            .map(|v| v != "false");
    }
    let show_border = effective_show_app_border.unwrap_or(true);

    if let Ok(mut guard) = state.theme.lock() {
        *guard = theme.clone();
    }

    #[cfg(target_os = "windows")]
    use windows::core::BOOL;
    #[cfg(target_os = "windows")]
    use windows::Win32::Foundation::HWND;
    #[cfg(target_os = "windows")]
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE,
        DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DWM_WINDOW_CORNER_PREFERENCE,
    };

    #[cfg(target_os = "windows")]
    {
        let hwnd = window
            .hwnd()
            .map_err(|e| AppError::Internal(e.to_string()))?;
        let hwnd = HWND(hwnd.0 as _);
        let _ = window_vibrancy::clear_vibrancy(&window);

        let is_dark = match effective_color_mode.as_deref() {
            Some("light") => false,
            Some("dark") => true,
            _ => window.theme().unwrap_or(Theme::Dark) == Theme::Dark,
        };

        let dark_mode = BOOL::from(is_dark);
        unsafe {
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &dark_mode as *const _ as _,
                std::mem::size_of::<BOOL>() as u32,
            );
            // Toggle native DWM border visibility while preserving the window frame/corners.
            const DWMWA_COLOR_DEFAULT: u32 = 0xFFFFFFFF;
            const DWMWA_COLOR_NONE: u32 = 0xFFFFFFFE;
            let border_color: u32 = if show_border {
                DWMWA_COLOR_DEFAULT
            } else {
                DWMWA_COLOR_NONE
            };
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_BORDER_COLOR,
                &border_color as *const _ as _,
                std::mem::size_of::<u32>() as u32,
            );
            // Keep rounded corners even when border/shadow are disabled.
            let corner_pref = DWM_WINDOW_CORNER_PREFERENCE(DWMWCP_ROUND.0);
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &corner_pref as *const _ as _,
                std::mem::size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
            );
        }

        let build = windows_version::OsVersion::current().build;
        let is_win11 = build >= 22000;
        let is_win10_1803 = build >= 17134;

        match theme.as_str() {
            "mica" if is_win11 => {
                let _ = window_vibrancy::apply_mica(&window, Some(is_dark));
                let _ = window.set_shadow(show_border);
            }
            "acrylic" if is_win10_1803 => {
                let _ = window_vibrancy::apply_acrylic(
                    &window,
                    Some(if is_dark {
                        (30, 30, 30, 40)
                    } else {
                        (240, 240, 240, 40)
                    }),
                );
                let _ = window.set_shadow(show_border);
            }
            _ => {
                let _ = window
                    .set_shadow(show_border && (theme != "mica" && theme != "acrylic" || is_win11));
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _is_dark = match effective_color_mode.as_deref() {
            Some("light") => false,
            Some("dark") => true,
            _ => window.theme().unwrap_or(Theme::Dark) == Theme::Dark,
        };

        let _ = window_vibrancy::clear_vibrancy(&window);
        let material = match theme.as_str() {
            "mica" => window_vibrancy::NSVisualEffectMaterial::HudWindow,
            "acrylic" => window_vibrancy::NSVisualEffectMaterial::UnderWindowBackground,
            _ => window_vibrancy::NSVisualEffectMaterial::HudWindow,
        };
        if theme == "mica" || theme == "acrylic" {
            let _ = window_vibrancy::apply_vibrancy(&window, material, None, None);
        }
    }

    let _ = window.emit("theme-changed", theme);
    Ok(())
}
