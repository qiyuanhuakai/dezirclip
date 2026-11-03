//! A Desktop-selected target is validated, never replaced by the current foreground.
use super::*;
use dzc_protocol::wire::WindowsPasteTarget;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use ::windows::Win32::{
    Foundation::{FILETIME, HANDLE, HWND, WAIT_TIMEOUT},
    System::Threading::{GetProcessId, GetProcessTimes, OpenProcess, WaitForSingleObject,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE},
    UI::WindowsAndMessaging::{GetAncestor, GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId,
        IsWindow, IsWindowVisible, GUITHREADINFO, GA_ROOT},
};

fn positive_decimal(text: &str) -> Option<u64> {
    if text.len() > 20 { return None; }
    let value = text.parse::<u64>().ok()?;
    (value > 0 && value.to_string() == text).then_some(value)
}
fn failed() -> PlatformError { PlatformError::failed(Capability::AutoPaste) }
fn focus_context(window: usize, root: usize, foreground: usize, focused_root: Option<usize>) -> bool {
    window != 0 && root != 0 && foreground == window && focused_root == Some(root)
}

pub struct WindowsBoundPaste {
    process: OwnedHandle,
    window: HWND,
    root: HWND,
    pid: u32,
    thread: u32,
    created: u64,
}
// HWND values are only queried; the owned process object may move to a Host worker.
unsafe impl Send for WindowsBoundPaste {}

impl WindowsBoundPaste {
    pub fn foreground() -> PlatformResult<WindowsPasteTarget> {
        unsafe {
            let hwnd = GetForegroundWindow();
            let mut pid = 0;
            let thread_id = GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if crate::windows_input::is_system_focus_window(hwnd) || thread_id == 0 || pid == std::process::id() { return Err(failed()); }
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).map_err(|_| failed())?;
            let process = OwnedHandle::from_raw_handle(handle.0);
            let (mut created, mut exit, mut kernel, mut user) = Default::default();
            GetProcessTimes(HANDLE(process.as_raw_handle()), &mut created, &mut exit, &mut kernel, &mut user).map_err(|_| failed())?;
            let target = WindowsPasteTarget {
                hwnd: (hwnd.0 as usize).to_string(), root_hwnd: (GetAncestor(hwnd,GA_ROOT).0 as usize).to_string(), pid, thread_id,
                creation_time_100ns: ((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime)).to_string(),
            };
            Self::prepare(&target)?;
            Ok(target)
        }
    }

    pub fn send_queue(&self, method: &str, text: Option<&str>, kind: &str, current: &dyn Fn() -> bool) -> PlatformResult<()> {
        struct Guarded<'a>(&'a WindowsBoundPaste, &'a dyn Fn() -> bool);
        impl crate::windows_input::InputTarget for Guarded<'_> {
            fn is_current(&self) -> bool { (self.1)() && self.0.check().is_ok() }
            fn windows_handle(&self) -> HWND { self.0.window }
            fn windows_identity_current(&self) -> bool { self.0.check_identity().is_ok() }
        }
        let target = Guarded(self, current);
        let alt = crate::windows_input::alt_is_down();
        crate::paste_input::send_queue_input(
            || if current() { Ok(()) } else { Err("PasteQueueChanged".into()) },
            || crate::windows_input::send_paste(&target, method, text, Some(kind)),
            || {
                std::thread::sleep(std::time::Duration::from_millis(20));
                crate::windows_input::restore_held_alt(&target, alt)
            }
        ).map_err(|_| failed())
    }

    // Preparation requires the restored target to be foreground with input focus in its original root.
    pub fn prepare(target: &WindowsPasteTarget) -> PlatformResult<Self> {
        let window = positive_decimal(&target.hwnd).and_then(|value| usize::try_from(value).ok()).ok_or_else(failed)?;
        let root = positive_decimal(&target.root_hwnd).and_then(|value| usize::try_from(value).ok()).ok_or_else(failed)?;
        let created = positive_decimal(&target.creation_time_100ns).ok_or_else(failed)?;
        if target.pid == 0 || target.thread_id == 0 { return Err(failed()); }
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE, false, target.pid) }
            .map_err(|_| failed())?;
        let process = unsafe { OwnedHandle::from_raw_handle(handle.0) };
        let value = Self { process, window: HWND(window as *mut _), root: HWND(root as *mut _),
            pid: target.pid, thread: target.thread_id, created };
        value.check()?;
        Ok(value)
    }
    fn check_identity(&self) -> PlatformResult<()> {
        let handle = HANDLE(self.process.as_raw_handle());
        let mut created=FILETIME::default();
        let mut exited=FILETIME::default();
        let mut kernel=FILETIME::default();
        let mut user=FILETIME::default();
        let mut pid=0;
        unsafe {
            if WaitForSingleObject(handle,0) != WAIT_TIMEOUT || GetProcessId(handle) != self.pid
                || GetProcessTimes(handle,&mut created,&mut exited,&mut kernel,&mut user).is_err() {
                return Err(failed());
            }
            let stamp=(u64::from(created.dwHighDateTime)<<32)|u64::from(created.dwLowDateTime);
            if stamp != self.created || !IsWindow(Some(self.window)).as_bool()
                || GetWindowThreadProcessId(self.window,Some(&mut pid)) != self.thread || pid != self.pid
                || GetAncestor(self.window,GA_ROOT) != self.root {
                return Err(failed());
            }
        }
        Ok(())
    }
    fn check(&self) -> PlatformResult<()> {
        self.check_identity()?;
        let handle = HANDLE(self.process.as_raw_handle());
        let mut pid = 0;
        unsafe {
            if !IsWindowVisible(self.window).as_bool() || GetForegroundWindow() != self.window { return Err(failed()); }
            let mut gui=GUITHREADINFO{cbSize:std::mem::size_of::<GUITHREADINFO>() as u32,..Default::default()};
            if GetGUIThreadInfo(self.thread,&mut gui).is_err() {return Err(failed());}
            let focused_root=if !gui.hwndFocus.0.is_null() && IsWindow(Some(gui.hwndFocus)).as_bool() {
                Some(GetAncestor(gui.hwndFocus,GA_ROOT).0 as usize)
            } else {None};
            if !focus_context(self.window.0 as usize,self.root.0 as usize,GetForegroundWindow().0 as usize,focused_root)
                || GetWindowThreadProcessId(self.window,Some(&mut pid)) != self.thread || pid != self.pid
                || GetAncestor(self.window,GA_ROOT) != self.root
                || WaitForSingleObject(handle,0) != WAIT_TIMEOUT || !IsWindowVisible(self.window).as_bool()
                || GetForegroundWindow() != self.window {
                return Err(failed());
            }
        }
        Ok(())
    }
    pub fn send(&self) -> PlatformResult<()> {
        super::windows::paste_bound(|| self.check())
    }
}

impl crate::windows_input::InputTarget for WindowsBoundPaste {
    fn is_current(&self) -> bool { self.check().is_ok() }
    fn windows_handle(&self) -> HWND { self.window }
    fn windows_identity_current(&self) -> bool { self.check_identity().is_ok() }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_or_foreign_input_focus_rejects_before_any_input() {
        for focused_root in [None,Some(99)] {
            let sends=std::cell::Cell::new(0);
            let result=super::super::windows::guarded_ctrl_v(
                ||if focus_context(10,10,10,focused_root){Ok(())}else{Err(failed())},
                ||{sends.set(sends.get()+1);4},|_|panic!("no prefix exists"));
            assert!(result.is_err());assert_eq!(sends.get(),0);
        }
        assert!(focus_context(10,10,10,Some(10)));
        assert!(!focus_context(10,10,99,Some(10)));
    }
    #[test]
    fn target_integer_strings_are_canonical_positive_and_bounded() {
        for value in ["", "0", "00", "01", "-1", "+1", " 1", "1 ", "1.0", "18446744073709551616"] {
            assert_eq!(positive_decimal(value),None);
        }
        assert_eq!(positive_decimal(&"1".repeat(21)),None);
        assert_eq!(positive_decimal("1"),Some(1));
        assert_eq!(positive_decimal("18446744073709551615"),Some(u64::MAX));
    }
}
