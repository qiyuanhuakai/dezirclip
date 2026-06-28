use serde_json::Value;
use tauri::AppHandle;
use x11rb::connection::Connection;
use x11rb::protocol::xproto;
use x11rb::protocol::xproto::ConnectionExt;
use x11rb::protocol::Event;

use crate::error::{AppError, AppResult};

static LAST_ACTIVE_HWND: std::sync::Mutex<usize> = std::sync::Mutex::new(0);

#[derive(Debug, Clone, Default)]
pub struct ActiveAppInfo {
    pub app_name: String,
    pub process_path: Option<String>,
}

fn unknown_app_info() -> ActiveAppInfo {
    ActiveAppInfo {
        app_name: "Unknown".to_string(),
        process_path: None,
    }
}

pub fn init_window_tracking(_app_handle: AppHandle) -> AppResult<()> {
    std::thread::spawn(move || {
        if let Err(e) = run_window_tracking_loop() {
            eprintln!("[window_tracker] Tracking loop error: {}", e);
        }
    });
    Ok(())
}

fn run_window_tracking_loop() -> Result<(), Box<dyn std::error::Error>> {
    let (conn, screen_num) = x11rb::connect(None)?;
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;

    let net_active_window_atom = conn
        .intern_atom(false, b"_NET_ACTIVE_WINDOW")?
        .reply()?
        .atom;

    let net_wm_pid_atom = conn.intern_atom(false, b"_NET_WM_PID")?.reply()?.atom;

    conn.change_window_attributes(
        root,
        &xproto::ChangeWindowAttributesAux::new().event_mask(xproto::EventMask::PROPERTY_CHANGE),
    )?
    .check()?;

    let our_pid = std::process::id() as u32;

    loop {
        match conn.wait_for_event() {
            Ok(event) => {
                if let Event::PropertyNotify(property_notify) = event {
                    if property_notify.atom == net_active_window_atom {
                        if let Ok(active_window) =
                            get_active_window_id(&conn, root, net_active_window_atom)
                        {
                            if active_window != 0 {
                                if let Ok(window_pid) =
                                    get_window_pid(&conn, active_window, net_wm_pid_atom)
                                {
                                    if window_pid != 0 && window_pid != our_pid {
                                        if let Ok(mut hwnd) = LAST_ACTIVE_HWND.lock() {
                                            *hwnd = active_window as usize;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("[window_tracker] X11 event error: {}", e);
            }
        }
    }
}

fn get_active_window_id(
    conn: &impl Connection,
    root: xproto::Window,
    net_active_window_atom: xproto::Atom,
) -> Result<u32, Box<dyn std::error::Error>> {
    let reply = conn
        .get_property(
            false,
            root,
            net_active_window_atom,
            xproto::AtomEnum::WINDOW,
            0,
            1,
        )?
        .reply()?;

    if reply.format == 32 && !reply.value.is_empty() {
        let window_id = u32::from_ne_bytes([
            reply.value[0],
            reply.value[1],
            reply.value[2],
            reply.value[3],
        ]);
        Ok(window_id)
    } else {
        Ok(0)
    }
}

fn get_window_pid(
    conn: &impl Connection,
    window: xproto::Window,
    net_wm_pid_atom: xproto::Atom,
) -> Result<u32, Box<dyn std::error::Error>> {
    let reply = conn
        .get_property(
            false,
            window,
            net_wm_pid_atom,
            xproto::AtomEnum::CARDINAL,
            0,
            1,
        )?
        .reply()?;

    if reply.format == 32 && !reply.value.is_empty() {
        let pid = u32::from_ne_bytes([
            reply.value[0],
            reply.value[1],
            reply.value[2],
            reply.value[3],
        ]);
        Ok(pid)
    } else {
        Ok(0)
    }
}

pub fn get_foreground_window_info() -> AppResult<ActiveAppInfo> {
    let (conn, screen_num) = x11rb::connect(None).map_err(|e| AppError::Internal(e.to_string()))?;
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;

    let net_active_window_atom = conn
        .intern_atom(false, b"_NET_ACTIVE_WINDOW")
        .map_err(|e| AppError::Internal(e.to_string()))?
        .reply()
        .map_err(|e| AppError::Internal(e.to_string()))?
        .atom;

    let net_wm_pid_atom = conn
        .intern_atom(false, b"_NET_WM_PID")
        .map_err(|e| AppError::Internal(e.to_string()))?
        .reply()
        .map_err(|e| AppError::Internal(e.to_string()))?
        .atom;

    let active_window = get_active_window_id(&conn, root, net_active_window_atom)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if active_window == 0 {
        return Ok(unknown_app_info());
    }

    let pid = get_window_pid(&conn, active_window, net_wm_pid_atom)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if pid == 0 {
        return Ok(unknown_app_info());
    }

    resolve_app_info_from_pid(pid)
}

fn resolve_app_info_from_pid(pid: u32) -> AppResult<ActiveAppInfo> {
    let comm_path = format!("/proc/{}/comm", pid);
    let app_name = std::fs::read_to_string(&comm_path)
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "Unknown".to_string());

    let exe_path = format!("/proc/{}/exe", pid);
    let process_path = std::fs::read_link(&exe_path)
        .ok()
        .map(|p| p.to_string_lossy().to_string());

    Ok(ActiveAppInfo {
        app_name,
        process_path,
    })
}

fn send_net_active_window(
    conn: &impl Connection,
    root: xproto::Window,
    window_id: u32,
) -> AppResult<()> {
    let net_active_window_atom = conn
        .intern_atom(false, b"_NET_ACTIVE_WINDOW")
        .map_err(|e| AppError::Internal(e.to_string()))?
        .reply()
        .map_err(|e| AppError::Internal(e.to_string()))?
        .atom;

    let data: [u32; 5] = [2, x11rb::CURRENT_TIME as u32, window_id, 0, 0];

    let event = xproto::ClientMessageEvent::new(32, window_id, net_active_window_atom, data);

    conn.send_event(
        false,
        root,
        xproto::EventMask::SUBSTRUCTURE_REDIRECT | xproto::EventMask::SUBSTRUCTURE_NOTIFY,
        event,
    )
    .map_err(|e| AppError::Internal(e.to_string()))?
    .check()
    .map_err(|e| AppError::Internal(format!("Failed to send _NET_ACTIVE_WINDOW: {:?}", e)))?;

    Ok(())
}

pub fn activate_window_focus(window_handle: usize) -> AppResult<()> {
    let window_id = window_handle as u32;

    if window_id == 0 {
        return Ok(());
    }

    let (conn, screen_num) = x11rb::connect(None).map_err(|e| AppError::Internal(e.to_string()))?;
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;

    let _ = send_net_active_window(&conn, root, window_id);

    let _ = conn.configure_window(
        window_id,
        &xproto::ConfigureWindowAux::new().stack_mode(xproto::StackMode::ABOVE),
    );

    conn.set_input_focus(
        xproto::InputFocus::POINTER_ROOT,
        window_id,
        x11rb::CURRENT_TIME,
    )
    .map_err(|e| AppError::Internal(e.to_string()))?
    .check()
    .map_err(|e| AppError::Internal(format!("Failed to set input focus: {:?}", e)))?;

    let _ = conn.flush();

    Ok(())
}

fn find_window_by_pid(conn: &impl Connection, screen_num: usize, target_pid: u32) -> Option<u32> {
    let root = conn.setup().roots[screen_num].root;

    let net_wm_pid = conn
        .intern_atom(false, b"_NET_WM_PID")
        .ok()?
        .reply()
        .ok()?
        .atom;

    // Try _NET_CLIENT_LIST first (standard way to list top-level windows)
    let net_client_list = conn
        .intern_atom(false, b"_NET_CLIENT_LIST")
        .ok()?
        .reply()
        .ok()?
        .atom;

    let reply = conn
        .get_property(
            false,
            root,
            net_client_list,
            xproto::AtomEnum::WINDOW,
            0,
            u32::MAX,
        )
        .ok()?
        .reply()
        .ok()?;

    if reply.format == 32 {
        for chunk in reply.value.chunks(4) {
            if chunk.len() != 4 {
                continue;
            }
            let window_id = u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            if window_id == 0 {
                continue;
            }
            let pid_reply = conn
                .get_property(
                    false,
                    window_id,
                    net_wm_pid,
                    xproto::AtomEnum::CARDINAL,
                    0,
                    1,
                )
                .ok()?
                .reply()
                .ok()?;
            if pid_reply.format == 32 && pid_reply.value.len() >= 4 {
                let pid = u32::from_ne_bytes([
                    pid_reply.value[0],
                    pid_reply.value[1],
                    pid_reply.value[2],
                    pid_reply.value[3],
                ]);
                if pid == target_pid {
                    return Some(window_id);
                }
            }
        }
    }

    // Fallback: query tree from root
    if let Ok(tree) = conn.query_tree(root) {
        if let Ok(tree_reply) = tree.reply() {
            for window_id in tree_reply.children {
                let pid_reply = conn
                    .get_property(
                        false,
                        window_id,
                        net_wm_pid,
                        xproto::AtomEnum::CARDINAL,
                        0,
                        1,
                    )
                    .ok()?
                    .reply()
                    .ok()?;
                if pid_reply.format == 32 && pid_reply.value.len() >= 4 {
                    let pid = u32::from_ne_bytes([
                        pid_reply.value[0],
                        pid_reply.value[1],
                        pid_reply.value[2],
                        pid_reply.value[3],
                    ]);
                    if pid == target_pid {
                        return Some(window_id);
                    }
                }
            }
        }
    }

    None
}

pub fn raise_own_window() -> AppResult<()> {
    let (conn, screen_num) = x11rb::connect(None).map_err(|e| AppError::Internal(e.to_string()))?;
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;

    let target_pid = std::process::id() as u32;
    let window_id = find_window_by_pid(&conn, screen_num, target_pid)
        .ok_or_else(|| AppError::Internal("Failed to find DezirClip window on X11".to_string()))?;

    let _ = send_net_active_window(&conn, root, window_id);

    conn.configure_window(
        window_id,
        &xproto::ConfigureWindowAux::new().stack_mode(xproto::StackMode::ABOVE),
    )
    .map_err(|e| AppError::Internal(e.to_string()))?
    .check()
    .map_err(|e| AppError::Internal(format!("Failed to raise window: {:?}", e)))?;

    conn.set_input_focus(
        xproto::InputFocus::POINTER_ROOT,
        window_id,
        x11rb::CURRENT_TIME,
    )
    .map_err(|e| AppError::Internal(e.to_string()))?
    .check()
    .map_err(|e| AppError::Internal(format!("Failed to set input focus: {:?}", e)))?;

    let _ = conn.flush();

    Ok(())
}

pub fn restore_last_focus() -> AppResult<()> {
    let window_id = {
        let hwnd = LAST_ACTIVE_HWND
            .lock()
            .map_err(|e| AppError::Internal(e.to_string()))?;
        *hwnd as u32
    };

    if window_id == 0 {
        return Ok(());
    }

    activate_window_focus(window_id as usize)
}

pub fn get_clipboard_source_app_info() -> AppResult<ActiveAppInfo> {
    let (conn, screen_num) = x11rb::connect(None).map_err(|e| AppError::Internal(e.to_string()))?;
    let _screen = &conn.setup().roots[screen_num];

    let clipboard_atom = conn
        .intern_atom(false, b"CLIPBOARD")
        .map_err(|e| AppError::Internal(e.to_string()))?
        .reply()
        .map_err(|e| AppError::Internal(e.to_string()))?
        .atom;

    let owner_reply = conn
        .get_selection_owner(clipboard_atom)
        .map_err(|e| AppError::Internal(e.to_string()))?
        .reply()
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let owner_window = owner_reply.owner;

    if owner_window == 0 {
        return get_foreground_window_info();
    }

    let net_wm_pid_atom = conn
        .intern_atom(false, b"_NET_WM_PID")
        .map_err(|e| AppError::Internal(e.to_string()))?
        .reply()
        .map_err(|e| AppError::Internal(e.to_string()))?
        .atom;

    let pid = get_window_pid(&conn, owner_window, net_wm_pid_atom)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    if pid == 0 {
        return get_foreground_window_info();
    }

    let our_pid = std::process::id() as u32;
    if pid == our_pid {
        return get_foreground_window_info();
    }

    resolve_app_info_from_pid(pid)
}

pub fn start_window_tracking(app_handle: AppHandle) {
    let _ = init_window_tracking(app_handle);
}

pub fn get_active_app_info() -> ActiveAppInfo {
    get_foreground_window_info().unwrap_or_else(|_| unknown_app_info())
}

pub fn launch_uwp_with_file(_package: &str, _file: &str) -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

pub fn get_system_default_app(_ext: &str) -> String {
    String::new()
}

pub fn get_executable_icon(_executable_path: String) -> Result<Option<String>, String> {
    Ok(None)
}

pub fn scan_installed_apps() -> Vec<Value> {
    vec![]
}

pub fn get_associated_apps(_ext: &str) -> Vec<Value> {
    vec![]
}
