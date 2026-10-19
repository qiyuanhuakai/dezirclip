use super::*;
use ::windows::Win32::{
    Foundation::CloseHandle,
    System::{
        DataExchange::{CloseClipboard, GetClipboardOwner, OpenClipboard},
        ProcessStatus::GetModuleFileNameExW,
        Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ},
    },
    UI::{
        Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
            KEYEVENTF_KEYUP, VIRTUAL_KEY,
        },
        WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
    },
};

pub fn clipboard_probe() -> CapabilityState {
    unsafe {
        if OpenClipboard(None).is_err() {
            return CapabilityState::unsupported(
                "win32-clipboard",
                UnsupportedReason::ConnectionUnavailable,
            )
            .with_probe("open_clipboard");
        }
        let _ = CloseClipboard();
    }
    CapabilityState::available("win32-clipboard", false).with_probe("open_close_clipboard")
}
pub fn source_probe() -> CapabilityState {
    if unsafe { GetForegroundWindow() }.0.is_null() {
        CapabilityState::unsupported("win32-window-source", UnsupportedReason::NoGraphicalSession)
            .with_probe("foreground_window")
    } else {
        CapabilityState::available("win32-window-source", false).with_probe("foreground_window")
    }
}
pub fn source() -> PlatformResult<SourceIdentity> {
    unsafe {
        let owner = GetClipboardOwner()
            .ok()
            .filter(|window| !window.0.is_null());
        let (window, resolution) = if let Some(owner) = owner {
            (owner, SourceResolution::ClipboardOwner)
        } else {
            (GetForegroundWindow(), SourceResolution::ActiveWindow)
        };
        if window.0.is_null() {
            return Err(PlatformError::unsupported(
                Capability::ResolveSource,
                UnsupportedReason::NoGraphicalSession,
            ));
        }
        let mut pid = 0;
        GetWindowThreadProcessId(window, Some(&mut pid));
        let mut identity = SourceIdentity {
            resolution,
            ..Default::default()
        };
        if let Ok(process) = OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
            false,
            pid,
        ) {
            let mut path = [0u16; 32768];
            let len = GetModuleFileNameExW(Some(process), None, &mut path) as usize;
            let _ = CloseHandle(process);
            if len > 0 && len < path.len() {
                let path = String::from_utf16_lossy(&path[..len]);
                identity.app_name = std::path::Path::new(&path)
                    .file_stem()
                    .map(|part| part.to_string_lossy().into_owned());
                identity.process_path = Some(path);
            }
        }
        Ok(identity)
    }
}
pub fn paste_probe() -> CapabilityState {
    let mut state = source_probe();
    state.backend = "win32-sendinput".into();
    state.probe = "interactive_foreground_target".into();
    state
}
pub fn paste() -> PlatformResult<()> {
    if !paste_probe().is_available() {
        return Err(PlatformError::unsupported(
            Capability::AutoPaste,
            UnsupportedReason::NoGraphicalSession,
        ));
    }
    let key = |code, up| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(code),
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                ..Default::default()
            },
        },
    };
    let inputs = [
        key(0x11, false),
        key(0x56, false),
        key(0x56, true),
        key(0x11, true),
    ];
    if unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) } != inputs.len() as u32 {
        return Err(PlatformError::failed(Capability::AutoPaste));
    }
    Ok(())
}
