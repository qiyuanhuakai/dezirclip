// Global state module
use std::ptr::null_mut;
use std::sync::atomic::{
    AtomicBool, AtomicI32, AtomicPtr, AtomicU32, AtomicU64, AtomicU8, AtomicUsize,
};

pub static GLOBAL_APP_HANDLE: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();
pub static HOOK_HANDLE: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(null_mut());
pub static HOOK_MOUSE_HANDLE: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(null_mut());
pub static HOTKEY_STRING: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

#[derive(Clone, Debug)]
pub struct HookHotkey {
    pub vk: u32,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub win: bool,
}

pub static TARGET_HOTKEY: std::sync::Mutex<Option<HookHotkey>> = std::sync::Mutex::new(None);

// Win+ hotkeys are now handled via tauri-plugin-global-shortcut.

pub static IS_RECORDING: AtomicBool = AtomicBool::new(false);
pub static IGNORE_BLUR: AtomicBool = AtomicBool::new(false);
pub static WINDOW_PINNED: AtomicBool = AtomicBool::new(false);
pub static CLIPBOARD_MONITOR_PAUSED: AtomicBool = AtomicBool::new(false);
pub static LAST_ACTIVE_HWND: AtomicUsize = AtomicUsize::new(0);
pub static LAST_APP_SET_HASH: AtomicU64 = AtomicU64::new(0);
pub static LAST_APP_SET_HASH_ALT: AtomicU64 = AtomicU64::new(0);
pub static LAST_APP_SET_TIMESTAMP: AtomicU64 = AtomicU64::new(0);
pub static LAST_TOGGLE_TIMESTAMP: AtomicU64 = AtomicU64::new(0);
pub static LAST_SHOW_TIMESTAMP: AtomicU64 = AtomicU64::new(0);
pub static HOOK_THREAD_ID: AtomicU32 = AtomicU32::new(0);
pub static TASKBAR_CREATED_MSG: AtomicU32 = AtomicU32::new(0);

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DockPosition {
    None,
    Top,
    Left,
    Right,
}

pub static CURRENT_DOCK: AtomicI32 = AtomicI32::new(0); // 0: None, 1: Top, 2: Left, 3: Right
                                                        // Tracks whether the main window is hidden specifically by edge docking.
                                                        // Other hide paths should clear it so the next toggle is treated as a normal show.
pub static IS_HIDDEN: AtomicBool = AtomicBool::new(false);
pub static IS_MOUSE_BUTTON_DOWN: AtomicBool = AtomicBool::new(false);
pub static NAVIGATION_ENABLED: AtomicBool = AtomicBool::new(false);
pub static NAVIGATION_MODE_ACTIVE: AtomicBool = AtomicBool::new(false);
pub static IS_MAIN_WINDOW_FOCUSED: AtomicBool = AtomicBool::new(false);

/// Timestamp (ms since UNIX_EPOCH) when the main window last became hidden.
/// `0` means "currently visible / never hidden".
/// Used by the idle destroyer to decide when to release the WebView2 / WebKitGTK process.
pub static LAST_HIDDEN_TIMESTAMP: AtomicU64 = AtomicU64::new(0);

/// Window lifecycle for the main window. Encoded as u8 for atomic use:
/// 0 = Open (normal, webview alive)
/// 1 = Closing (destroy requested, in-flight)
/// 2 = Closed (webview torn down, awaiting recreate)
/// 3 = Opening (recreate requested, in-flight)
pub static WINDOW_LIFECYCLE: AtomicU8 = AtomicU8::new(0);

/// `true` if the main window has been destroyed by the idle destroyer and is awaiting recreate.
/// Kept separate from `WINDOW_LIFECYCLE` for fast checks in code paths that don't need full state.
pub static IS_DESTROYED: AtomicBool = AtomicBool::new(false);

/// A recreate request was queued while a destroy was in-flight.
/// The destroy path consumes this flag and immediately recreates after tearing down.
pub static RECREATE_PENDING: AtomicBool = AtomicBool::new(false);

/// `true` if a clipboard capture was written to the database while the main
/// window was off screen, so the frontend never heard about it.
///
/// The main window is hidden for almost its entire life, and delivering a
/// capture into a hidden renderer undoes the low memory target that the hide
/// just applied. The entry is already durable, so the capture is not lost — the
/// next show converts this flag into a single `clipboard-changed` refresh.
pub static FRONTEND_CATCHUP_PENDING: AtomicBool = AtomicBool::new(false);

/// Timestamp (ms since UNIX_EPOCH) when the main window entered the `Closing`
/// lifecycle state. `0` means "not closing".
/// Bounded by `idle_destroyer::CLOSING_WATCHDOG_MS` so a teardown the runtime
/// never confirms cannot wedge the lifecycle in `Closing` forever.
pub static CLOSING_SINCE_MS: AtomicU64 = AtomicU64::new(0);
