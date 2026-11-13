//! Preserve the last editor across Explorer tray activation. No content is read.
use super::{PlatformError, PlatformResult, Capability, WindowsBoundPaste};
use crate::ipc::ProcessExit;
use dzc_protocol::wire::WindowsPasteTarget;
use std::{collections::VecDeque, sync::{Mutex, OnceLock}};
use windows::Win32::{
    Foundation::HWND,
    UI::{Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK},
        WindowsAndMessaging::{DispatchMessageW, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId,
            TranslateMessage, MSG, EVENT_SYSTEM_FOREGROUND, WINEVENT_OUTOFCONTEXT}},
};

struct Observed { target: WindowsPasteTarget, identity: WindowsBoundPaste }
static RECENT: Mutex<VecDeque<Observed>> = Mutex::new(VecDeque::new());

fn failed() -> PlatformError { PlatformError::failed(Capability::AutoPaste) }

fn observe(window: HWND) {
    if window.0.is_null() || crate::windows_input::is_system_focus_window(window) { return; }
    let observed = WindowsBoundPaste::describe(window).and_then(|target|
        WindowsBoundPaste::remember(&target).map(|identity| Observed { target, identity }));
    if let Ok(mut recent) = RECENT.lock() {
        match observed {
            Ok(value) => {
                recent.retain(|old| old.target.hwnd != value.target.hwnd);
                recent.push_front(value);
                recent.truncate(8);
            }
            // Never substitute an older editor when the new target cannot be identified.
            Err(_) => recent.clear(),
        }
    }
}

unsafe extern "system" fn foreground(_: HWINEVENTHOOK, _: u32, window: HWND, _: i32, _: i32, _: u32, _: u32) {
    if unsafe { GetForegroundWindow() } == window { observe(window); }
}

pub(super) fn start() -> PlatformResult<()> {
    static STARTED: OnceLock<bool> = OnceLock::new();
    if *STARTED.get_or_init(|| {
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::Builder::new().name("desktop-focus".into()).spawn(move || unsafe {
            let hook = SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, None,
                Some(foreground), 0, 0, WINEVENT_OUTOFCONTEXT);
            if hook.0.is_null() { let _ = send.send(false); return; }
            observe(GetForegroundWindow());
            let _ = send.send(true);
            let mut message = MSG::default();
            while GetMessageW(&mut message, None, 0, 0).0 > 0 {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            let _ = UnhookWinEvent(hook);
        });
        worker.is_ok() && receive.recv().unwrap_or(false)
    }) { Ok(()) } else { Err(failed()) }
}

pub(super) fn previous(peer_pid: u32, peer: &ProcessExit) -> PlatformResult<(WindowsPasteTarget, WindowsBoundPaste)> {
    let current = unsafe { GetForegroundWindow() };
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(current, Some(&mut pid)); }
    // The fallback applies only to our own foreground or the shell. An unrelated
    // foreground app must use the normal, strict foreground capture instead.
    if !crate::windows_input::is_system_focus_window(current) && pid != peer_pid
        && !peer.shares_executable(pid).map_err(|_| failed())? { return Err(failed()); }
    let recent = RECENT.lock().map_err(|_| failed())?;
    for value in recent.iter() {
        if value.target.pid == peer_pid || peer.shares_executable(value.target.pid).map_err(|_| failed())? { continue; }
        value.identity.check_identity()?;
        let bound = WindowsBoundPaste::remember(&value.target)?;
        // Focus may change while we validate process objects; do not cross that boundary.
        if unsafe { GetForegroundWindow() } != current { return Err(failed()); }
        return Ok((value.target.clone(), bound));
    }
    Err(failed())
}
