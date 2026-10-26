//! Request-bound native paste targets. No target handle crosses the child IPC.
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Process {
    pid: u32,
    started: u64,
}
static MANAGED: Mutex<Option<Process>> = Mutex::new(None);
fn register(current: &mut Option<Process>, identity: Process) -> Result<(), String> {
    if current.is_some() || identity.pid == 0 || identity.started == 0 {
        return Err("SurfaceProcessAlreadyManaged".into());
    }
    *current = Some(identity);
    Ok(())
}
fn matching(expected: Option<Process>, actual: Option<Process>) -> bool {
    expected.is_some() && expected == actual
}
pub struct ManagedChild(Process);
impl ManagedChild {
    pub fn register(pid: u32) -> Result<Self, String> {
        let identity = process(pid)?;
        let mut current = MANAGED
            .lock()
            .map_err(|_| "SurfaceProcessStateUnavailable")?;
        register(&mut current, identity)?;
        Ok(Self(identity))
    }
}
impl Drop for ManagedChild {
    fn drop(&mut self) {
        if let Ok(mut current) = MANAGED.lock() {
            if *current == Some(self.0) {
                *current = None;
            }
        }
    }
}
pub fn is_managed_child(pid: u32) -> bool {
    let expected = MANAGED.lock().ok().and_then(|current| *current);
    expected
        .is_some_and(|expected| expected.pid == pid && matching(Some(expected), process(pid).ok()))
}
fn ours(pid: u32) -> bool {
    pid == std::process::id() || is_managed_child(pid)
}
pub fn configure_spawn(command: &mut std::process::Command) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = command;
    }
}

#[derive(Debug)]
pub struct Target {
    window: usize,
    process: Process,
    thread: u32,
}
impl Target {
    pub fn capture() -> Result<Self, String> {
        native::capture()
    }
    pub fn restore_if(&self, current: impl Fn() -> bool) -> Result<(), String> {
        native::restore(self, &current)
    }
    pub fn is_current(&self) -> bool {
        native::is_current(self)
    }
    pub fn context_current(&self) -> bool {
        native::context_current(self)
    }
}
#[cfg(target_os = "windows")]
fn process(pid: u32) -> Result<Process, String> {
    use windows::Win32::{
        Foundation::{CloseHandle, FILETIME},
        System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    };
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
            .map_err(|_| "SurfaceProcessUnavailable")?;
        let mut created = FILETIME::default();
        let mut exited = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let result = GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user);
        let _ = CloseHandle(handle);
        result.map_err(|_| "SurfaceProcessUnavailable")?;
        Ok(Process {
            pid,
            started: (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime),
        })
    }
}
#[cfg(target_os = "linux")]
fn process(pid: u32) -> Result<Process, String> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .map_err(|_| "SurfaceProcessUnavailable")?;
    let fields = stat.rsplit_once(')').ok_or("SurfaceProcessUnavailable")?.1;
    let started = fields
        .split_whitespace()
        .nth(19)
        .ok_or("SurfaceProcessUnavailable")?
        .parse()
        .map_err(|_| "SurfaceProcessUnavailable")?;
    Ok(Process { pid, started })
}
#[cfg(target_os = "windows")]
mod native {
    use super::*;
    use windows::Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId, IsWindow, IsWindowVisible,
            SetForegroundWindow,
        },
    };
    fn hwnd(window: usize) -> HWND {
        HWND(window as *mut _)
    }
    fn identity(window: usize) -> Result<(Process, u32), String> {
        if window == 0
            || !unsafe {
                IsWindow(Some(hwnd(window))).as_bool() && IsWindowVisible(hwnd(window)).as_bool()
            }
        {
            return Err("SurfaceTargetUnavailable".into());
        }
        let mut pid = 0;
        let thread = unsafe { GetWindowThreadProcessId(hwnd(window), Some(&mut pid)) };
        if thread == 0 || ours(pid) {
            return Err("SurfaceTargetUnavailable".into());
        }
        Ok((process(pid)?, thread))
    }
    fn valid(target: &Target) -> bool {
        identity(target.window).ok() == Some((target.process, target.thread))
    }
    fn current() -> usize {
        unsafe { GetForegroundWindow().0 as usize }
    }
    pub(super) fn capture() -> Result<Target, String> {
        use std::sync::atomic::Ordering;
        let current = current();
        let window = if identity(current).is_ok() {
            current
        } else {
            crate::global_state::LAST_ACTIVE_HWND.load(Ordering::Acquire)
        };
        if crate::infrastructure::windows_api::window_tracker::is_system_focus_window(hwnd(window))
        {
            return Err("SurfaceTargetUnavailable".into());
        }
        let (process, thread) = identity(window)?;
        Ok(Target {
            window,
            process,
            thread,
        })
    }
    pub(super) fn is_current(target: &Target) -> bool {
        valid(target) && current() == target.window
    }
    pub(super) fn context_current(target: &Target) -> bool {
        if !valid(target) {
            return false;
        }
        let window = current();
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd(window), Some(&mut pid));
        }
        window == target.window || ours(pid)
    }
    pub(super) fn restore(target: &Target, allowed: &dyn Fn() -> bool) -> Result<(), String> {
        if !allowed() {
            return Err("SurfaceDisplayCancelled".into());
        }
        if !valid(target) {
            return Err("SurfaceTargetChanged".into());
        }
        let current = current();
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd(current), Some(&mut pid));
        }
        if current != target.window && !ours(pid) {
            return Err("SurfaceFocusChanged".into());
        }
        if !allowed() {
            return Err("SurfaceDisplayCancelled".into());
        }
        if current != target.window
            && !unsafe { SetForegroundWindow(hwnd(target.window)).as_bool() }
        {
            return Err("SurfaceFocusRestoreFailed".into());
        }
        if !is_current(target) {
            return Err("SurfaceFocusRestoreFailed".into());
        }
        Ok(())
    }
}
#[cfg(target_os = "linux")]
mod native {
    use super::*;
    use crate::infrastructure::linux_api::window_tracker as tracker;
    fn identity(window: usize) -> Result<Process, String> {
        let pid = tracker::surface_window_pid(window).map_err(|_| "SurfaceTargetUnavailable")?;
        if ours(pid) {
            return Err("SurfaceTargetUnavailable".into());
        }
        process(pid)
    }
    pub(super) fn capture() -> Result<Target, String> {
        if dzc_platform::runtime::is_native_wayland() {
            return Err("wayland_interaction_unavailable".into());
        }
        let current = tracker::surface_active_window().map_err(|_| "SurfaceTargetUnavailable")?;
        let window = if identity(current).is_ok() {
            current
        } else {
            tracker::surface_last_target()
        };
        Ok(Target {
            window,
            process: identity(window)?,
            thread: 0,
        })
    }
    pub(super) fn is_current(target: &Target) -> bool {
        identity(target.window).ok() == Some(target.process)
            && tracker::surface_active_window().ok() == Some(target.window)
    }
    pub(super) fn context_current(target: &Target) -> bool {
        if identity(target.window).ok() != Some(target.process) {
            return false;
        }
        let Ok(window) = tracker::surface_active_window() else {
            return false;
        };
        window == target.window || tracker::surface_window_pid(window).is_ok_and(ours)
    }
    pub(super) fn restore(target: &Target, allowed: &dyn Fn() -> bool) -> Result<(), String> {
        if !allowed() {
            return Err("SurfaceDisplayCancelled".into());
        }
        if identity(target.window).ok() != Some(target.process) {
            return Err("SurfaceTargetChanged".into());
        }
        let current = tracker::surface_active_window().map_err(|_| "SurfaceTargetUnavailable")?;
        if current != target.window {
            let pid =
                tracker::surface_window_pid(current).map_err(|_| "SurfaceTargetUnavailable")?;
            if !ours(pid) {
                return Err("SurfaceFocusChanged".into());
            }
        }
        if !allowed() {
            return Err("SurfaceDisplayCancelled".into());
        }
        tracker::activate_window_focus(target.window).map_err(|_| "SurfaceFocusRestoreFailed")?;
        if !is_current(target) {
            return Err("SurfaceFocusRestoreFailed".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_child_exemption_is_exact_and_scoped_to_its_live_process_identity() {
        let identity = Process {
            pid: 10,
            started: 12,
        };
        let mut current = None;
        register(&mut current, identity).unwrap();
        assert!(matching(current, Some(identity)));
        assert!(register(&mut current, identity).is_err());
        assert!(!matching(
            current,
            Some(Process {
                pid: 10,
                started: 13
            })
        ));
        assert!(!matching(
            current,
            Some(Process {
                pid: 11,
                started: 12
            })
        ));
        assert!(!matching(None, Some(identity)));
        assert!(!matching(current, None));
    }
}
