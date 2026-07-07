use crate::app::webview_memory;
use crate::app_state::SettingsState;
use crate::database::DbState;
#[cfg(target_os = "windows")]
use crate::error::AppError;
use crate::error::AppResult;
use crate::infrastructure::repository::settings_repo::SettingsRepository;
use serde::Serialize;
#[cfg(target_os = "linux")]
use std::process::Command;
use tauri::{AppHandle, Emitter, Manager, State, Theme, WebviewWindow};

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

    #[cfg(not(target_os = "windows"))]
    {
        PlatformInfo {
            platform: "linux".to_string(),
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

#[cfg(any(target_os = "windows", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowsBackdropEffect {
    None,
    Mica,
    Acrylic,
}

#[cfg(any(target_os = "windows", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WindowsThemePlan {
    effect: WindowsBackdropEffect,
    clear_native_backdrop: bool,
    force_transparent_webview: bool,
    shadow: bool,
}

#[cfg(any(target_os = "windows", test))]
fn windows_theme_plan(theme: &str, build: u32, show_border: bool) -> WindowsThemePlan {
    let is_win11 = build >= 22000;
    let is_win10_1803 = build >= 17134;
    let effect = match theme {
        "mica" if is_win11 => WindowsBackdropEffect::Mica,
        "acrylic" if is_win10_1803 => WindowsBackdropEffect::Acrylic,
        _ => WindowsBackdropEffect::None,
    };

    WindowsThemePlan {
        effect,
        clear_native_backdrop: true,
        force_transparent_webview: true,
        shadow: show_border && ((theme != "mica" && theme != "acrylic") || is_win11),
    }
}

#[cfg(target_os = "windows")]
fn clear_windows_backdrop(window: &WebviewWindow) {
    let _ = window_vibrancy::clear_mica(window);
    let _ = window_vibrancy::clear_acrylic(window);
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
    #[cfg(not(target_os = "windows"))]
    let _ = show_border;

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
        let plan = windows_theme_plan(&theme, build, show_border);

        if plan.clear_native_backdrop {
            clear_windows_backdrop(&window);
        }

        match plan.effect {
            WindowsBackdropEffect::Mica => {
                let _ = window_vibrancy::apply_mica(&window, Some(is_dark));
                let _ = window.set_shadow(plan.shadow);
            }
            WindowsBackdropEffect::Acrylic => {
                let _ = window_vibrancy::apply_acrylic(
                    &window,
                    Some(if is_dark {
                        (30, 30, 30, 40)
                    } else {
                        (240, 240, 240, 40)
                    }),
                );
                let _ = window.set_shadow(plan.shadow);
            }
            WindowsBackdropEffect::None => {
                let _ = window.set_shadow(plan.shadow);
            }
        }

        if plan.force_transparent_webview {
            webview_memory::force_transparent_background(&window);
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

    #[cfg(not(target_os = "windows"))]
    webview_memory::force_transparent_background(&window);

    Ok(())
}

/// Whether the Tauri window with the given label is currently the topmost
/// window of this application. Used by the React frontend to detect the
/// compact-preview overlay sitting over the cursor position so it can fall
/// back to the anchor rect for context menu placement. Bug: B6.
#[tauri::command]
pub fn is_window_topmost(app: AppHandle, label: String) -> bool {
    is_window_topmost_for_platform(&app, &label)
}

#[cfg(target_os = "linux")]
fn is_window_topmost_for_platform(app: &AppHandle, label: &str) -> bool {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto;
    use x11rb::protocol::xproto::ConnectionExt;

    let Ok((conn, screen_num)) = x11rb::connect(None) else {
        return false;
    };
    let root = conn.setup().roots[screen_num].root;

    let Ok(intern_reply) = conn.intern_atom(false, b"_NET_ACTIVE_WINDOW") else {
        return false;
    };
    let Ok(reply) = intern_reply.reply() else {
        return false;
    };
    let net_active_window_atom = reply.atom;

    let Ok(property_reply) = conn.get_property(
        false,
        root,
        net_active_window_atom,
        xproto::AtomEnum::WINDOW,
        0,
        1,
    ) else {
        return false;
    };
    let Ok(reply) = property_reply.reply() else {
        return false;
    };
    if reply.format != 32 || reply.value.len() < 4 {
        return false;
    }
    let active_wid = u32::from_ne_bytes([
        reply.value[0],
        reply.value[1],
        reply.value[2],
        reply.value[3],
    ]);
    if active_wid == 0 {
        return false;
    }

    let Ok(intern_reply) = conn.intern_atom(false, b"_NET_WM_PID") else {
        return false;
    };
    let Ok(reply) = intern_reply.reply() else {
        return false;
    };
    let net_wm_pid_atom = reply.atom;

    let Ok(property_reply) = conn.get_property(
        false,
        active_wid,
        net_wm_pid_atom,
        xproto::AtomEnum::CARDINAL,
        0,
        1,
    ) else {
        return false;
    };
    let Ok(reply) = property_reply.reply() else {
        return false;
    };
    let active_pid = if reply.format == 32 && reply.value.len() >= 4 {
        u32::from_ne_bytes([
            reply.value[0],
            reply.value[1],
            reply.value[2],
            reply.value[3],
        ])
    } else {
        0
    };

    if active_pid != std::process::id() {
        return false;
    }

    app.get_webview_window(label)
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(false)
}

#[cfg(target_os = "windows")]
fn is_window_topmost_for_platform(app: &AppHandle, label: &str) -> bool {
    app.get_webview_window(label)
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(false)
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn is_window_topmost_for_platform(_app: &AppHandle, _label: &str) -> bool {
    false
}

#[cfg(test)] 

mod tests {
    use super::*;

    #[test]
    fn windows_liquid_glass_clears_previous_native_backdrop() {
        let plan = windows_theme_plan("liquid-glass", 22631, true);

        assert_eq!(plan.effect, WindowsBackdropEffect::None);
        assert!(plan.clear_native_backdrop);
        assert!(plan.force_transparent_webview);
        assert!(plan.shadow);
    }

    #[test]
    fn windows_mica_on_win11_keeps_webview_transparent_for_material_pass_through() {
        let plan = windows_theme_plan("mica", 22631, true);

        assert_eq!(plan.effect, WindowsBackdropEffect::Mica);
        assert!(plan.clear_native_backdrop);
        assert!(plan.force_transparent_webview);
        assert!(plan.shadow);
    }

    #[test]
    fn windows_acrylic_on_win10_clears_existing_backdrop_before_applying_acrylic() {
        let plan = windows_theme_plan("acrylic", 19045, true);

        assert_eq!(plan.effect, WindowsBackdropEffect::Acrylic);
        assert!(plan.clear_native_backdrop);
        assert!(plan.force_transparent_webview);
        assert!(!plan.shadow);
    }
}
