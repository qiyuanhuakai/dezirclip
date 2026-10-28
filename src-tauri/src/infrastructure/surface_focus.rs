//! Request-bound native paste targets. No target handle crosses the child IPC.
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
enum Foreground {
    Managed,
    Target,
    Owned,
    Absent,
    External,
}
fn focused(original_valid: bool, child_valid: bool, foreground: Foreground) -> Result<bool, String> {
    if !original_valid { return Err("SurfaceTargetChanged".into()); }
    if !child_valid { return Err("SurfaceProcessUnavailable".into()); }
    match foreground {
        Foreground::Managed => Ok(true),
        Foreground::Target | Foreground::Owned | Foreground::Absent => Ok(false),
        Foreground::External => Err("SurfaceFocusChanged".into()),
    }
}
#[cfg(any(target_os = "linux", test))]
pub(crate) fn input_focus_belongs_to(window: u32, focus: u32, mut parent: impl FnMut(u32) -> Option<u32>) -> bool {
    if window <= 1 || focus <= 1 { return false; }
    let mut seen = std::collections::HashSet::new();
    let mut current = focus;
    for _ in 0..32 {
        if current == window { return true; }
        if current <= 1 || !seen.insert(current) { return false; }
        let Some(next) = parent(current) else { return false; };
        current = next;
    }
    false
}
fn authorize_if(
    pid: u32,
    current: impl Fn() -> bool,
    child_valid: impl Fn() -> bool,
    context_current: impl Fn() -> bool,
    grant: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    if pid == 0 || pid == u32::MAX { return Err("SurfaceProcessUnavailable".into()); }
    if !current() { return Err("SurfaceDisplayCancelled".into()); }
    if !child_valid() { return Err("SurfaceProcessUnavailable".into()); }
    if !context_current() { return Err("SurfaceFocusChanged".into()); }
    grant()?;
    if !current() { return Err("SurfaceDisplayCancelled".into()); }
    if !child_valid() { return Err("SurfaceProcessUnavailable".into()); }
    if !context_current() { return Err("SurfaceFocusChanged".into()); }
    Ok(())
}
fn wait_for_focus(
    deadline: Instant,
    current: impl Fn() -> bool,
    mut observe: impl FnMut() -> Result<bool, String>,
    now: impl Fn() -> Instant,
    sleep: impl Fn(Duration),
) -> Result<(), String> {
    loop {
        if !current() { return Err("SurfaceDisplayCancelled".into()); }
        if now() >= deadline { return Err("SurfaceFocusUnavailable".into()); }
        if observe()? {
            if !current() { return Err("SurfaceDisplayCancelled".into()); }
            if now() >= deadline { return Err("SurfaceFocusUnavailable".into()); }
            return Ok(());
        }
        let remaining = deadline.saturating_duration_since(now());
        if remaining.is_zero() { return Err("SurfaceFocusUnavailable".into()); }
        sleep(remaining.min(Duration::from_millis(20)));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestoreForeground { None, Owned, Target, External }
impl RestoreForeground {
    pub fn label(self) -> &'static str {
        match self { Self::None => "none", Self::Owned => "owned", Self::Target => "target", Self::External => "external" }
    }
}
fn restore_until(
    deadline: Instant,
    current: impl Fn() -> bool,
    valid: impl Fn() -> bool,
    mut observe: impl FnMut() -> Result<RestoreForeground, String>,
    mut activate: impl FnMut() -> Result<bool, String>,
    mut diagnostic: impl FnMut(RestoreForeground),
    now: impl Fn() -> Instant,
    sleep: impl Fn(Duration),
) -> Result<(), String> {
    let check = || {
        if !current() { return Err("SurfaceDisplayCancelled".to_owned()); }
        if now() >= deadline { return Err("SurfaceFocusRestoreFailed".to_owned()); }
        if !valid() { return Err("SurfaceTargetChanged".to_owned()); }
        if !current() { return Err("SurfaceDisplayCancelled".to_owned()); }
        if now() >= deadline { return Err("SurfaceFocusRestoreFailed".to_owned()); }
        Ok(())
    };
    let mut activated = false;
    let mut previous = None;
    let mut diagnostics = 0;
    loop {
        check()?;
        let foreground = observe()?;
        check()?;
        if previous != Some(foreground) && diagnostics < 16 {
            diagnostic(foreground);
            previous = Some(foreground);
            diagnostics += 1;
        }
        check()?;
        match foreground {
            RestoreForeground::Target => return Ok(()),
            RestoreForeground::External => return Err("SurfaceFocusChanged".into()),
            RestoreForeground::Owned if !activated => {
                check()?;
                activated = activate()?;
                check()?;
            }
            RestoreForeground::None | RestoreForeground::Owned => {}
        }
        let remaining = deadline.saturating_duration_since(now());
        if remaining.is_zero() { return Err("SurfaceFocusRestoreFailed".into()); }
        sleep(remaining.min(Duration::from_millis(20)));
    }
}

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
fn presentation_budget(budget: Duration, capture_elapsed: Duration) -> Result<Duration, String> {
    budget.checked_sub(capture_elapsed).filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| "SurfaceFocusUnavailable".into())
}
#[cfg(any(target_os = "linux", test))]
pub(crate) fn application_activation_data(request_time: u32) -> Result<[u32; 5], String> {
    if request_time == 0 { return Err("SurfaceActivationUnavailable".into()); }
    Ok([1, request_time, 0, 0, 0])
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
struct RequestClock { timestamp: u32, elapsed: Duration }
pub struct ManagedActivation {
    #[cfg(target_os = "linux")]
    prepared: Option<crate::infrastructure::linux_api::surface_activation::Prepared>,
}
#[derive(Debug)]
pub struct Target {
    window: usize,
    process: Process,
    thread: u32,
    request_clock: std::sync::OnceLock<RequestClock>,
}
/// Presentation-only context: it cannot be passed to the paste/restore APIs.
#[cfg(target_os = "linux")]
pub(crate) struct PresentationTarget {
    origin: Option<Target>,
    previous_external: Option<Target>,
}
#[cfg(target_os = "linux")]
impl PresentationTarget {
    pub(crate) fn capture() -> Result<Self, String> { native::capture_presentation() }
    pub(crate) fn current(&self) -> bool { native::presentation_context_current(self) }
}
#[cfg(any(target_os = "linux", test))]
fn presentation_context_allowed(origin_valid: bool, origin_current: bool, owned_current: bool,
    absent: bool, previous_external_current: bool) -> bool {
    origin_valid && (origin_current || owned_current || absent || previous_external_current)
}
#[cfg(any(target_os = "linux", test))]
fn present_once(timestamp: u32, deadline: Instant, current: impl Fn() -> bool,
    now: impl Fn() -> Instant, present: impl FnOnce(u32) -> Result<(), String>) -> Result<(), String> {
    if timestamp == 0 { return Err("SurfaceActivationUnavailable".into()); }
    if !current() || now() >= deadline { return Err("SurfaceDisplayCancelled".into()); }
    present(timestamp)
}
#[cfg(target_os = "linux")]
pub(crate) fn present_tauri_with_time(window: &tauri::WebviewWindow, timestamp: u32,
    deadline: Instant, current: impl Fn() -> bool) -> Result<(), String> {
    use gtk::prelude::GtkWindowExt;
    if window.label() != "quick-paste" || !gtk::is_initialized_main_thread() {
        return Err("QuickPasteUiThreadRequired".into());
    }
    let gtk_window = window.gtk_window().map_err(|error| error.to_string())?;
    present_once(timestamp, deadline, current, Instant::now, |timestamp| {
        gtk_window.present_with_time(timestamp);
        Ok(())
    })
}
impl Target {
    pub fn capture() -> Result<Self, String> {
        native::capture()
    }
    pub async fn prepare_request_time(self: &std::sync::Arc<Self>, current: impl Fn() -> bool + Send + Sync + 'static) -> Result<(), String> {
        if !current() { return Err("SurfaceDisplayCancelled".into()); }
        #[cfg(target_os = "windows")]
        { Ok(()) }
        #[cfg(target_os = "linux")]
        {
            let started = Instant::now();
            let deadline = started + Duration::from_millis(250);
            let current: std::sync::Arc<dyn Fn() -> bool + Send + Sync> = std::sync::Arc::new(current);
            let worker_current = current.clone();
            let target = self.clone();
            let worker = tauri::async_runtime::spawn_blocking(move || -> Result<u32, String> {
                if !worker_current() { return Err("SurfaceDisplayCancelled".into()); }
                if Instant::now() >= deadline { return Err("SurfaceActivationUnavailable".into()); }
                let timestamp = crate::infrastructure::linux_api::surface_activation::request_time(deadline, worker_current.clone())?;
                if !worker_current() { return Err("SurfaceDisplayCancelled".into()); }
                if Instant::now() >= deadline { return Err("SurfaceActivationUnavailable".into()); }
                if !target.presentation_current() { return Err("SurfaceFocusChanged".into()); }
                Ok(timestamp)
            });
            let timestamp = tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), worker).await
                .map_err(|_| "SurfaceActivationUnavailable".to_owned())?
                .map_err(|_| "SurfaceActivationUnavailable".to_owned())??;
            if !current() { return Err("SurfaceDisplayCancelled".into()); }
            if Instant::now() >= deadline { return Err("SurfaceActivationUnavailable".into()); }
            let elapsed = started.elapsed();
            if elapsed >= Duration::from_millis(250) { return Err("SurfaceActivationUnavailable".into()); }
            self.request_clock.set(RequestClock { timestamp, elapsed }).map_err(|_| "SurfaceActivationUnavailable".to_owned())?;
            Ok(())
        }
    }
    pub fn presentation_deadline(&self, budget: Duration) -> Result<Instant, String> {
        #[cfg(target_os = "linux")]
        let elapsed = self.request_clock.get().ok_or("SurfaceActivationUnavailable")?.elapsed;
        #[cfg(target_os = "windows")]
        let elapsed = Duration::ZERO;
        Ok(Instant::now() + presentation_budget(budget, elapsed)?)
    }
    #[cfg(target_os = "linux")]
    pub(crate) fn request_timestamp(&self) -> Result<u32, String> {
        self.request_clock.get().map(|clock| clock.timestamp).filter(|value| *value != 0)
            .ok_or_else(|| "SurfaceActivationUnavailable".into())
    }
    pub fn restore_if(&self, deadline: Instant, current: impl Fn() -> bool + Send + Sync + 'static,
        diagnostic: impl FnMut(RestoreForeground)) -> Result<(), String> {
        #[cfg(target_os = "windows")]
        { native::restore(self, deadline, &current, diagnostic) }
        #[cfg(target_os = "linux")]
        { native::restore(self, deadline, std::sync::Arc::new(current), diagnostic) }
    }
    pub fn is_current(&self) -> bool {
        native::is_current(self)
    }
    pub fn context_current(&self) -> bool {
        native::context_current(self)
    }
    pub fn presentation_current(&self) -> bool {
        native::presentation_current(self)
    }
    pub fn authorize_managed(&self, pid: u32, current: impl Fn() -> bool) -> Result<(), String> {
        authorize_if(pid, current, || is_managed_child(pid), || self.presentation_current(), || native::authorize(pid))
    }
    pub fn wait_managed_focus(&self, pid: u32, deadline: Instant, current: impl Fn() -> bool) -> Result<(), String> {
        wait_for_focus(deadline, current, || native::managed_focus(self, pid), Instant::now, std::thread::sleep)
    }
    pub fn prepare_managed_activation(&self, pid: u32, deadline: Instant, current: impl Fn() -> bool + Send + Sync + 'static) -> Result<ManagedActivation, String> {
        #[cfg(target_os = "windows")]
        { let _ = (pid, deadline, current); Ok(ManagedActivation {}) }
        #[cfg(target_os = "linux")]
        {
            let current: std::sync::Arc<dyn Fn() -> bool + Send + Sync> = std::sync::Arc::new(current);
            authorize_if(pid, || current(), || is_managed_child(pid), || self.presentation_current(), || Ok(()))?;
            if native::managed_focus(self, pid)? { return Ok(ManagedActivation { prepared: None }); }
            if Instant::now() >= deadline { return Err("SurfaceFocusUnavailable".into()); }
            let timestamp = self.request_clock.get().ok_or("SurfaceActivationUnavailable")?.timestamp;
            let prepared = crate::infrastructure::linux_api::surface_activation::prepare(pid, timestamp, deadline, current)?;
            Ok(ManagedActivation { prepared: Some(prepared) })
        }
    }
    pub fn activate_managed_once(&self, activation: ManagedActivation, pid: u32, deadline: Instant, current: impl Fn() -> bool) -> Result<(), String> {
        #[cfg(target_os = "windows")]
        { let _ = (activation, pid, deadline, current); Ok(()) }
        #[cfg(target_os = "linux")]
        {
            if let Some(prepared) = activation.prepared {
                if Instant::now() >= deadline { return Err("SurfaceFocusUnavailable".into()); }
                prepared.send(|| authorize_if(pid, &current, || is_managed_child(pid), || self.presentation_current(), || Ok(())))?;
            }
            Ok(())
        }
    }
}
#[cfg(target_os = "windows")]
fn process(pid: u32) -> Result<Process, String> {
    use windows::Win32::{
        Foundation::{CloseHandle, FILETIME},
        System::Threading::{GetProcessTimes, OpenProcess, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE},
    };
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE, false, pid)
            .map_err(|_| "SurfaceProcessUnavailable")?;
        let mut created = FILETIME::default();
        let mut exited = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let result = GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user);
        let running = WaitForSingleObject(handle, 0) == windows::Win32::Foundation::WAIT_TIMEOUT;
        let _ = CloseHandle(handle);
        result.map_err(|_| "SurfaceProcessUnavailable")?;
        if !running { return Err("SurfaceProcessUnavailable".into()); }
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
    if matches!(fields.split_whitespace().next(), Some("Z" | "X")) {
        return Err("SurfaceProcessUnavailable".into());
    }
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
            AllowSetForegroundWindow, SetForegroundWindow,
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
            request_clock: std::sync::OnceLock::new(),
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
    pub(super) fn presentation_current(target: &Target) -> bool {
        valid(target) && (current() == 0 || context_current(target))
    }
    pub(super) fn authorize(pid: u32) -> Result<(), String> {
        unsafe { AllowSetForegroundWindow(pid) }.map_err(|_| "SurfaceFocusGrantDenied".into())
    }
    pub(super) fn managed_focus(target: &Target, pid: u32) -> Result<bool, String> {
        let window = current();
        let mut actual = 0;
        unsafe { GetWindowThreadProcessId(hwnd(window), Some(&mut actual)); }
        let foreground = if window == 0 { Foreground::Absent }
            else if actual == pid { Foreground::Managed }
            else if window == target.window { Foreground::Target }
            else if ours(actual) { Foreground::Owned }
            else { Foreground::External };
        focused(valid(target), is_managed_child(pid), foreground)
    }
    fn restore_foreground(target: &Target) -> RestoreForeground {
        let window = current();
        if window == 0 { return RestoreForeground::None; }
        if window == target.window { return RestoreForeground::Target; }
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(hwnd(window), Some(&mut pid)); }
        if ours(pid) { RestoreForeground::Owned } else { RestoreForeground::External }
    }
    pub(super) fn restore(target: &Target, deadline: Instant, allowed: &dyn Fn() -> bool,
        diagnostic: impl FnMut(RestoreForeground)) -> Result<(), String> {
        restore_until(deadline, allowed, || valid(target), || Ok(restore_foreground(target)), || {
            // Re-read immediately before the sole activation attempt.
            match restore_foreground(target) {
                RestoreForeground::External => return Err("SurfaceFocusChanged".into()),
                RestoreForeground::None | RestoreForeground::Target => return Ok(false),
                RestoreForeground::Owned => {}
            }
            if !allowed() { return Err("SurfaceDisplayCancelled".into()); }
            if !valid(target) { return Err("SurfaceTargetChanged".into()); }
            if Instant::now() >= deadline { return Err("SurfaceFocusRestoreFailed".into()); }
            if !unsafe { SetForegroundWindow(hwnd(target.window)).as_bool() } {
                return Err("SurfaceFocusRestoreFailed".into());
            }
            Ok(true)
        }, diagnostic, Instant::now, std::thread::sleep)
    }
}
#[cfg(target_os = "linux")]
mod native {
    use super::*;
    use crate::infrastructure::linux_api::window_tracker as tracker;
    fn presentation_identity(window: usize) -> Result<Process, String> {
        process(tracker::surface_window_pid(window).map_err(|_| "SurfaceTargetUnavailable")?)
    }
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
            request_clock: std::sync::OnceLock::new(),
        })
    }
    pub(super) fn capture_presentation() -> Result<PresentationTarget, String> {
        // Snapshot the already remembered target before reading this request's
        // foreground. capture() would reread foreground and could adopt a newly
        // selected third-party window as a permitted handoff target.
        let remembered = tracker::surface_last_target();
        let window = tracker::surface_active_window().map_err(|_| "SurfaceTargetUnavailable")?;
        if window == 0 { return Ok(PresentationTarget { origin: None, previous_external: None }); }
        let process = presentation_identity(window)?;
        // Only an owned-origin handoff may also return to the already recorded
        // external target. Never learn a new external window while waiting.
        let previous_external = if ours(process.pid) {
            identity(remembered).ok().map(|process| Target { window: remembered, process, thread: 0,
                request_clock: std::sync::OnceLock::new() })
        } else { None };
        if tracker::surface_active_window().ok() != Some(window)
            || presentation_identity(window).ok() != Some(process) {
            return Err("SurfaceFocusChanged".into());
        }
        Ok(PresentationTarget { origin: Some(Target { window, process, thread: 0,
            request_clock: std::sync::OnceLock::new() }), previous_external })
    }
    pub(super) fn presentation_context_current(context: &PresentationTarget) -> bool {
        let Ok(window) = tracker::surface_active_window() else { return false; };
        presentation_context_allowed(context.origin.as_ref().map_or(true,
            |origin| presentation_identity(origin.window).ok() == Some(origin.process)),
            context.origin.as_ref().is_some_and(|origin| window == origin.window),
            tracker::surface_window_pid(window).is_ok_and(ours), window == 0,
            context.previous_external.as_ref().is_some_and(is_current))
    }
    pub(super) fn is_current(target: &Target) -> bool {
        identity(target.window).ok() == Some(target.process)
            && tracker::surface_active_window().ok() == Some(target.window)
            && tracker::surface_has_input_focus(target.window, target.process.pid).unwrap_or(false)
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
    pub(super) fn presentation_current(target: &Target) -> bool {
        identity(target.window).ok() == Some(target.process)
            && (tracker::surface_active_window().ok() == Some(0) || context_current(target))
    }
    pub(super) fn authorize(_pid: u32) -> Result<(), String> { Ok(()) }
    pub(super) fn managed_focus(target: &Target, pid: u32) -> Result<bool, String> {
        let window = tracker::surface_active_window().unwrap_or(0);
        let actual = tracker::surface_window_pid(window).ok();
        let foreground = if window == 0 { Foreground::Absent }
            else if actual == Some(pid) && tracker::surface_has_input_focus(window, pid).unwrap_or(false) { Foreground::Managed }
            else if actual == Some(pid) { Foreground::Owned }
            else if window == target.window { Foreground::Target }
            else if actual.is_some_and(ours) { Foreground::Owned }
            else { Foreground::External };
        focused(identity(target.window).ok() == Some(target.process), is_managed_child(pid), foreground)
    }
    pub(super) fn restore(target: &Target, deadline: Instant, allowed: std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
        mut diagnostic: impl FnMut(RestoreForeground)) -> Result<(), String> {
        let window = u32::try_from(target.window).map_err(|_| "SurfaceTargetUnavailable")?;
        crate::infrastructure::linux_api::surface_activation::restore_once(window, deadline, allowed.clone(), || {
            if !allowed() { return Err("SurfaceDisplayCancelled".into()); }
            if Instant::now() >= deadline { return Err("SurfaceFocusRestoreFailed".into()); }
            if identity(target.window).ok() != Some(target.process) { return Err("SurfaceTargetChanged".into()); }
            let current = tracker::surface_active_window().map_err(|_| "SurfaceTargetUnavailable")?;
            diagnostic(if current == 0 { RestoreForeground::None } else if current == target.window {
                RestoreForeground::Target
            } else if tracker::surface_window_pid(current).is_ok_and(ours) { RestoreForeground::Owned }
            else { RestoreForeground::External });
            if current != target.window {
                let pid = tracker::surface_window_pid(current).map_err(|_| "SurfaceTargetUnavailable")?;
                if !ours(pid) { return Err("SurfaceFocusChanged".into()); }
            }
            Ok(())
        })?;
        if !allowed() { return Err("SurfaceDisplayCancelled".into()); }
        if Instant::now() >= deadline || !is_current(target) {
            return Err("SurfaceFocusRestoreFailed".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gtk_presentation_uses_the_original_server_timestamp_once() {
        let start = Instant::now();
        let calls = std::cell::Cell::new(0);
        present_once(17, start + Duration::from_secs(1), || true, || start, |timestamp| {
            assert_eq!(timestamp, 17);
            calls.set(calls.get() + 1);
            Ok(())
        }).unwrap();
        assert_eq!(calls.get(), 1);
    }
    #[test]
    fn gtk_presentation_rejects_missing_expired_or_cancelled_requests_without_activation() {
        let start = Instant::now();
        for (timestamp, now, current) in [(0, start, true), (17, start + Duration::from_secs(1), true), (17, start, false)] {
            let calls = std::cell::Cell::new(0);
            assert!(present_once(timestamp, start + Duration::from_secs(1), || current, || now, |_| {
                calls.set(calls.get() + 1);
                Ok(())
            }).is_err());
            assert_eq!(calls.get(), 0);
        }
    }
    #[test]
    fn gtk_presentation_checks_expiry_after_context_observation() {
        let start = Instant::now();
        let clock = std::cell::Cell::new(start);
        let calls = std::cell::Cell::new(0);
        assert!(present_once(17, start + Duration::from_secs(1), || {
            clock.set(start + Duration::from_secs(1));
            true
        }, || clock.get(), |_| {
            calls.set(calls.get() + 1);
            Ok(())
        }).is_err());
        assert_eq!(calls.get(), 0);
    }
    #[test]
    fn gtk_presentation_failure_never_retries_or_replaces_the_captured_time() {
        let start = Instant::now();
        let calls = std::cell::Cell::new(0);
        let result = present_once(17, start + Duration::from_secs(1), || true, || start, |timestamp| {
            assert_eq!(timestamp, 17);
            calls.set(calls.get() + 1);
            Err("NativePresentationFailed".to_owned())
        });
        assert_eq!(result.unwrap_err(), "NativePresentationFailed");
        assert_eq!(calls.get(), 1);
    }
    #[test]
    fn presentation_context_keeps_owned_origin_and_rejects_unknown_external_focus() {
        assert!(presentation_context_allowed(true, true, false, false, false));
        assert!(presentation_context_allowed(true, false, true, false, false));
        assert!(presentation_context_allowed(true, false, false, true, false));
        assert!(presentation_context_allowed(true, false, false, false, true));
        assert!(!presentation_context_allowed(true, false, false, false, false));
        assert!(!presentation_context_allowed(false, true, true, true, true));
    }
    #[test]
    fn restore_waits_through_absence_without_activating() {
        let start = Instant::now();
        let clock = std::cell::Cell::new(start);
        let mut states = [RestoreForeground::None, RestoreForeground::Target].into_iter();
        let mut observed = Vec::new();
        restore_until(start + Duration::from_secs(5), || true, || true,
            || Ok(states.next().unwrap()), || panic!("empty foreground cannot authorize activation"),
            |state| observed.push(state), || clock.get(), |wait| clock.set(clock.get() + wait)).unwrap();
        assert_eq!(observed, [RestoreForeground::None, RestoreForeground::Target]);
    }
    #[test]
    fn restore_activates_once_then_waits_for_asynchronous_target_confirmation() {
        let start = Instant::now();
        let clock = std::cell::Cell::new(start);
        let mut states = [RestoreForeground::Owned, RestoreForeground::None,
            RestoreForeground::Owned, RestoreForeground::Target].into_iter();
        let calls = std::cell::Cell::new(0);
        restore_until(start + Duration::from_secs(5), || true, || true,
            || Ok(states.next().unwrap()), || { calls.set(calls.get() + 1); Ok(true) }, |_| {},
            || clock.get(), |wait| clock.set(clock.get() + wait)).unwrap();
        assert_eq!(calls.get(), 1);
    }
    #[test]
    fn an_activation_recheck_that_makes_no_write_does_not_consume_the_single_attempt() {
        let start = Instant::now();
        let clock = std::cell::Cell::new(start);
        let calls = std::cell::Cell::new(0);
        let mut states = [RestoreForeground::Owned, RestoreForeground::None,
            RestoreForeground::Owned, RestoreForeground::Target].into_iter();
        restore_until(start + Duration::from_secs(5), || true, || true,
            || Ok(states.next().unwrap()), || {
                calls.set(calls.get() + 1);
                Ok(calls.get() == 2)
            }, |_| {}, || clock.get(), |wait| clock.set(clock.get() + wait)).unwrap();
        assert_eq!(calls.get(), 2);
    }
    #[test]
    fn restore_rejects_external_or_changed_targets_without_activation() {
        let start = Instant::now();
        assert_eq!(restore_until(start + Duration::from_secs(5), || true, || true,
            || Ok(RestoreForeground::External), || panic!("external focus is never replaced"), |_| {},
            || start, |_| panic!("external focus cancels immediately")).unwrap_err(), "SurfaceFocusChanged");
        assert_eq!(restore_until(start + Duration::from_secs(5), || true, || false,
            || panic!("changed identity is rejected before observation"), || panic!("changed identity cannot activate"), |_| {},
            || start, |_| panic!("changed identity cannot wait")).unwrap_err(), "SurfaceTargetChanged");
    }
    #[test]
    fn restore_deadline_and_cancellation_prevent_late_activation_or_success() {
        let start = Instant::now();
        let deadline = start + Duration::from_millis(40);
        for state in [RestoreForeground::Target, RestoreForeground::Owned] {
            let clock = std::cell::Cell::new(start);
            assert_eq!(restore_until(deadline, || true, || true, || {
                clock.set(deadline); Ok(state)
            }, || panic!("observation consumed the budget"), |_| {}, || clock.get(), |_| panic!("expired"))
                .unwrap_err(), "SurfaceFocusRestoreFailed");
        }
        let cancelled = std::cell::Cell::new(false);
        assert_eq!(restore_until(deadline, || !cancelled.get(), || true,
            || Ok(RestoreForeground::Owned), || { cancelled.set(true); Ok(true) }, |_| {},
            || start, |_| panic!("cancelled immediately after activation")).unwrap_err(), "SurfaceDisplayCancelled");
        let clock = std::cell::Cell::new(start);
        assert_eq!(restore_until(deadline, || true, || true,
            || Ok(RestoreForeground::Owned), || { clock.set(deadline); Ok(true) }, |_| {},
            || clock.get(), |_| panic!("activation consumed the budget")).unwrap_err(), "SurfaceFocusRestoreFailed");
        assert_eq!(restore_until(deadline, || false, || true,
            || panic!("cancelled before observation"), || panic!("cancelled before activation"), |_| {},
            || start, |_| panic!("cancelled")).unwrap_err(), "SurfaceDisplayCancelled");
    }
    #[test]
    fn absent_restore_is_bounded_and_diagnostics_only_record_limited_changes() {
        let start = Instant::now();
        let clock = std::cell::Cell::new(start);
        let observations = std::cell::Cell::new(0);
        let diagnostics = std::cell::Cell::new(0);
        assert_eq!(restore_until(start + Duration::from_secs(1), || true, || true, || {
            observations.set(observations.get() + 1);
            Ok(if observations.get() % 2 == 0 { RestoreForeground::Owned } else { RestoreForeground::None })
        }, || Ok(true), |_| diagnostics.set(diagnostics.get() + 1), || clock.get(), |wait| clock.set(clock.get() + wait))
            .unwrap_err(), "SurfaceFocusRestoreFailed");
        assert_eq!(diagnostics.get(), 16);
        assert!(observations.get() > 16);
    }
    #[test]
    fn timestamp_capture_consumes_the_existing_presentation_budget() {
        let budget = Duration::from_secs(5);
        assert_eq!(presentation_budget(budget, Duration::from_millis(250)).unwrap(), Duration::from_millis(4750));
        assert_eq!(presentation_budget(budget, Duration::ZERO).unwrap(), budget);
        assert!(presentation_budget(budget, budget).is_err());
        assert!(presentation_budget(budget, budget + Duration::from_nanos(1)).is_err());
    }
    #[test]
    fn activation_is_an_application_request_with_only_the_captured_server_time() {
        assert_eq!(application_activation_data(12345).unwrap(), [1, 12345, 0, 0, 0]);
        assert_eq!(application_activation_data(u32::MAX).unwrap(), [1, u32::MAX, 0, 0, 0]);
        assert_eq!(application_activation_data(0).unwrap_err(), "SurfaceActivationUnavailable");
    }
    #[test]
    fn only_a_current_exact_child_and_original_context_receive_one_foreground_grant() {
        for invalid in 0..6 {
            let grants = std::cell::Cell::new(0);
            let result = authorize_if(if invalid == 0 { 0 } else if invalid == 1 { u32::MAX } else { 10 },
                || invalid != 2, || invalid != 3, || invalid != 4,
                || { grants.set(grants.get() + 1); Ok(()) });
            assert_eq!(result.is_ok(), invalid == 5);
            assert_eq!(grants.get(), if invalid == 5 { 1 } else { 0 });
        }
    }
    #[test]
    fn grant_failure_or_a_post_grant_owner_change_never_retries_or_becomes_success() {
        let current = std::cell::Cell::new(true);
        let grants = std::cell::Cell::new(0);
        assert_eq!(authorize_if(10, || current.get(), || true, || true, || {
            grants.set(grants.get() + 1); current.set(false); Ok(())
        }).unwrap_err(), "SurfaceDisplayCancelled");
        assert_eq!(grants.get(), 1);
        assert_eq!(authorize_if(10, || true, || true, || true,
            || Err("SurfaceFocusGrantDenied".into())).unwrap_err(), "SurfaceFocusGrantDenied");
    }
    #[test]
    fn native_visibility_or_transient_absence_is_not_a_focus_acknowledgement() {
        for state in [Foreground::Target, Foreground::Owned, Foreground::Absent] {
            assert!(!focused(true, true, state).unwrap());
        }
        assert!(focused(true, true, Foreground::Managed).unwrap());
        assert_eq!(focused(true, true, Foreground::External).unwrap_err(), "SurfaceFocusChanged");
        assert!(focused(false, true, Foreground::Managed).is_err());
        assert!(focused(true, false, Foreground::Managed).is_err());
    }
    #[test]
    fn a_cancelled_or_expired_focus_wait_never_queries_native_state() {
        let target = Target { window: 1, process: Process { pid: 2, started: 3 }, thread: 4, request_clock: std::sync::OnceLock::new() };
        assert_eq!(target.wait_managed_focus(10, Instant::now() + Duration::from_secs(5), || false).unwrap_err(), "SurfaceDisplayCancelled");
        assert_eq!(target.wait_managed_focus(10, Instant::now(), || true).unwrap_err(), "SurfaceFocusUnavailable");
    }
    #[test]
    fn focus_observed_at_or_after_the_original_deadline_is_rejected() {
        let start = Instant::now();
        let deadline = start + Duration::from_secs(5);
        let clock = std::cell::Cell::new(start);
        let calls = std::cell::Cell::new(0);
        let result = wait_for_focus(deadline, || true, || {
            calls.set(calls.get() + 1);
            clock.set(deadline);
            Ok(true)
        }, || clock.get(), |_| panic!("a completed observation does not wait"));
        assert_eq!(result.unwrap_err(), "SurfaceFocusUnavailable");
        assert_eq!(calls.get(), 1);
        assert!(wait_for_focus(deadline, || true, || Ok(true), || start, |_| panic!("already focused")).is_ok());
    }
    #[test]
    fn input_focus_requires_a_bounded_real_descendant_not_an_active_window_hint() {
        let parents = |window| match window { 8 => Some(7), 7 => Some(6), 6 => Some(0), _ => None };
        assert!(input_focus_belongs_to(6, 6, |_| panic!("same window needs no parent read")));
        assert!(input_focus_belongs_to(6, 8, parents));
        assert!(!input_focus_belongs_to(6, 0, parents));
        assert!(!input_focus_belongs_to(6, 1, parents));
        assert!(!input_focus_belongs_to(6, 9, parents));
        assert!(!input_focus_belongs_to(6, 8, |window| Some(if window == 8 { 7 } else { 8 })));
        let calls = std::cell::Cell::new(0);
        assert!(!input_focus_belongs_to(6, 100, |window| { calls.set(calls.get() + 1); Some(window + 1) }));
        assert_eq!(calls.get(), 32);
    }
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
