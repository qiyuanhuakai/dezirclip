#[cfg(target_os = "windows")]
use crate::global_state::TASKBAR_CREATED_MSG;
#[cfg(target_os = "windows")]
use std::sync::atomic::Ordering;
#[cfg(target_os = "windows")]
use tauri::Manager;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
#[cfg(target_os = "windows")]
use windows::Win32::UI::Shell::DefSubclassProc;

/// Window subclass procedure to handle taskbar recreation (explorer restart)
#[cfg(target_os = "windows")]
pub unsafe extern "system" fn tray_subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    let taskbar_msg = TASKBAR_CREATED_MSG.load(Ordering::Relaxed);
    if msg != 0 && msg == taskbar_msg {
        if let Some(app_handle) = crate::GLOBAL_APP_HANDLE.get() {
            let handle = app_handle.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(1500));
                if let Some(settings) = handle.try_state::<crate::app_state::SettingsState>() {
                    if settings.hide_tray_icon.load(Ordering::Relaxed) {
                        if let Some(tray) = handle.tray_by_id("main_tray") {
                            let _ = tray.set_visible(false);
                            println!(">>> [TRAY] Explorer restart detected, re-hiding tray icon per user setting.");
                        }
                    }
                }
            });
        }
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}
