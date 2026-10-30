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


#[cfg(any(feature = "ipc", test))]
fn partial_ctrl_v_releases(accepted: usize) -> &'static [u16] {
    match accepted { 1 | 3 => &[0x11], 2 => &[0x56,0x11], _ => &[] }
}
#[cfg(any(feature = "ipc", test))]
pub(super) fn guarded_ctrl_v(mut current: impl FnMut() -> PlatformResult<()>, send: impl FnOnce() -> usize,
    mut release: impl FnMut(&[u16])) -> PlatformResult<()> {
    current()?;
    let accepted=send();
    let after=current();
    if accepted != 4 {
        let keys=partial_ctrl_v_releases(accepted);
        if !keys.is_empty() {release(keys);}
        return Err(PlatformError::failed(Capability::AutoPaste));
    }
    after
}
#[cfg(feature = "ipc")]
pub(super) fn paste_bound(mut current: impl FnMut() -> PlatformResult<()>) -> PlatformResult<()> {
    if !paste_probe().is_available() {
        return Err(PlatformError::unsupported(Capability::AutoPaste,UnsupportedReason::NoGraphicalSession));
    }
    let key = |code,up| INPUT { r#type:INPUT_KEYBOARD,Anonymous:INPUT_0 {ki:KEYBDINPUT {
        wVk:VIRTUAL_KEY(code),dwFlags:if up {KEYEVENTF_KEYUP}else{KEYBD_EVENT_FLAGS(0)},..Default::default()
    }}};
    let inputs=[key(0x11,false),key(0x56,false),key(0x56,true),key(0x11,true)];
    if [0x10,0x11,0x12,0x5b,0x5c,0x56].into_iter().any(|code|unsafe {::windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(code)<0}) {
        return Err(PlatformError::failed(Capability::AutoPaste));
    }
    guarded_ctrl_v(&mut current,
        ||unsafe {SendInput(&inputs,std::mem::size_of::<INPUT>() as i32)} as usize,
        |codes| {
            // Only this attempt's accepted prefix may need key-up cleanup.
            let releases:Vec<_>=codes.iter().map(|code|key(*code,true)).collect();
            unsafe {SendInput(&releases,std::mem::size_of::<INPUT>() as i32)};
        })
}
#[cfg(test)]
mod bound_input_tests {
    use super::*;
    #[test]
    fn pre_and_post_guards_never_recapture_or_retry_input() {
        use std::cell::Cell;
        for fail_at in [Some(0),Some(1),None] {
            let guards=Cell::new(0);let sends=Cell::new(0);let releases=Cell::new(0);
            let result=guarded_ctrl_v(|| {
                let index=guards.get();guards.set(index+1);
                if fail_at==Some(index) {Err(PlatformError::failed(Capability::AutoPaste))}else{Ok(())}
            },||{sends.set(sends.get()+1);4},|_|releases.set(releases.get()+1));
            assert_eq!(result.is_ok(),fail_at.is_none());
            assert_eq!(sends.get(),if fail_at==Some(0){0}else{1});
            assert_eq!(releases.get(),0);
        }
    }
    #[test]
    fn partial_batch_only_releases_owned_prefix_once_without_new_paste() {
        for accepted in 0..4 {
            let mut cleanup=Vec::new();let calls=std::cell::Cell::new(0);
            assert!(guarded_ctrl_v(||Ok(()),||{calls.set(calls.get()+1);accepted},|keys|cleanup.extend_from_slice(keys)).is_err());
            assert_eq!(calls.get(),1);
            assert_eq!(cleanup.as_slice(),partial_ctrl_v_releases(accepted));
        }
    }
    #[test]
    fn partial_ctrl_v_cleanup_contains_only_this_prefix_unreleased_keys() {
        assert!(partial_ctrl_v_releases(0).is_empty());
        assert_eq!(partial_ctrl_v_releases(1),&[0x11][..]);
        assert_eq!(partial_ctrl_v_releases(2),&[0x56,0x11][..]);
        assert_eq!(partial_ctrl_v_releases(3),&[0x11][..]);
        assert!(partial_ctrl_v_releases(4).is_empty());
        assert!(partial_ctrl_v_releases(5).is_empty());
    }
}
