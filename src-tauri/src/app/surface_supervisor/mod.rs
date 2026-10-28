//! Experimental Quick Paste is a managed read-only child, never a second Host.
mod identity;
mod state;
mod diagnostics;
use super::ipc_host::{DesktopHost, SurfaceAccess};
use crate::infrastructure::repository::settings_repo::SettingsRepository;
use crate::infrastructure::surface_focus::{ManagedChild, RestoreForeground, Target};
use dzc_core::{
    ports::PasteController,
    protocol::{
        surface_control::{self, Direction},
        wire::*,
    },
};
use serde::{de::DeserializeOwned, Serialize};
use std::{
    collections::HashMap,
    path::PathBuf,
    process::Stdio,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, Weak,
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{Child, ChildStdin},
    sync::{oneshot, Mutex as AsyncMutex},
};
const TIMEOUT: Duration = Duration::from_secs(5);
const FOCUS_CANCEL_REASON: &str = "cancel";
fn action_budget(deadline: Instant) -> Result<(), String> {
    if Instant::now() >= deadline { Err("SurfaceActionTimeout".into()) } else { Ok(()) }
}
async fn action_until<T>(deadline: Instant, work: impl std::future::Future<Output = Result<T, String>>) -> Result<T, String> {
    action_budget(deadline)?;
    let result = tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), work)
        .await.map_err(|_| "SurfaceActionTimeout".to_owned())?;
    action_budget(deadline)?;
    result
}
const SURFACE_ENV_KEYS: &[&str] = &[
    "SystemRoot", "WINDIR", "PATH", "TEMP", "TMP", "HOME", "USERPROFILE",
    "LOCALAPPDATA", "APPDATA", "DISPLAY", "XAUTHORITY", "WAYLAND_DISPLAY",
    "XDG_RUNTIME_DIR", "DBUS_SESSION_BUS_ADDRESS", "LANG", "LC_ALL",
    "FONTCONFIG_FILE", "FONTCONFIG_PATH",
];
type Reply = oneshot::Sender<SurfaceControlMessage>;
struct PendingReply<'a> {
    replies: &'a Mutex<HashMap<String, Reply>>,
    request_id: &'a str,
}
impl Drop for PendingReply<'_> {
    fn drop(&mut self) {
        if let Ok(mut replies) = self.replies.lock() {
            replies.remove(self.request_id);
        }
    }
}

#[derive(Default)]
pub struct SurfaceSupervisor {
    serial: AsyncMutex<()>,
    // Only revision changes and the atomic main-show cancellation use this lock.
    // Never hold it over native window calls, another lock, or an await.
    route_commit: Mutex<()>,
    process: Mutex<Option<Arc<Managed>>>,
    revision: Arc<AtomicU64>,
    visible: AtomicU64,
    requested: AtomicU64,
    last_error: Mutex<Option<String>>,
    pending_notice: Mutex<Option<String>>,
}
struct Managed {
    id: String,
    pid: u32,
    access: SurfaceAccess,
    created: Instant,
    child: AsyncMutex<Child>,
    writer: AsyncMutex<Option<ChildStdin>>,
    replies: Mutex<HashMap<String, Reply>>,
    view: Mutex<state::View>,
    target: Mutex<Option<Arc<Target>>>,
    cancelled: AtomicBool,
    process_identity: Mutex<Option<ManagedChild>>,
    event_pending: AtomicBool,
    dismiss_pending: AtomicBool,
    request_revision: AtomicU64,
    diagnostic_lines: AtomicU64,
}
pub struct SurfaceRequest {
    revision: u64,
    owner: Option<Arc<Managed>>,
    display: Option<String>,
    target: Option<Arc<Target>>,
}
#[derive(Clone)]
pub struct SurfaceFallback {
    revision: u64,
    target: Option<Arc<Target>>,
    original_tauri: bool,
    deadline: Instant,
    #[cfg(target_os = "linux")]
    presentation: Option<Arc<crate::infrastructure::surface_focus::PresentationTarget>>,
}
impl SurfaceFallback {
    pub fn original_tauri(revision: u64) -> Result<Self, String> {
        let deadline = Instant::now() + TIMEOUT;
        #[cfg(target_os = "linux")]
        let presentation = if dzc_platform::runtime::is_native_wayland() { None }
            else { Some(Arc::new(crate::infrastructure::surface_focus::PresentationTarget::capture()?)) };
        Ok(Self { revision, target: None, original_tauri: true, deadline,
            #[cfg(target_os = "linux")] presentation })
    }
    fn context_current(&self) -> bool {
        #[cfg(target_os = "linux")]
        if let Some(context) = &self.presentation { return context.current(); }
        self.original_tauri || self.target.as_ref().is_some_and(|target| target.presentation_current())
    }
    pub fn current(&self, app: &AppHandle) -> bool {
        Instant::now() < self.deadline && route_current(app, self.revision) && self.context_current()
    }
    pub(crate) fn revision(&self) -> u64 { self.revision }
    pub(crate) fn deadline(&self) -> Instant { self.deadline }
}
impl SurfaceRequest {
    fn bind(&mut self, owner: Option<Arc<Managed>>) -> Result<(), String> {
        self.display = if let Some(process) = owner.as_ref() {
            process.view.lock().map_err(error)?.display.as_ref().map(|display| display.id.clone())
        } else {
            None
        };
        self.owner = owner;
        Ok(())
    }
}
fn diagnostic_code(value: &str) -> &str {
    match value {
        "SurfaceFocusChanged" | "SurfaceDisplayCancelled" | "SurfaceFocusRestoreFailed"
        | "SurfaceTargetChanged" | "SurfaceTargetUnavailable" | "SurfaceGeometryUnavailable"
        | "SurfaceTransportUnavailable" | "SurfaceAcknowledgementRejected"
        | "SurfaceAcknowledgementTimeout" | "SurfaceStaleAcknowledgement"
        | "SurfaceRequestsBusy" | "SurfaceWriteTimeout" | "SurfaceReadTimeout"
        | "SurfaceFrameTooLarge" | "SurfaceStartupUnavailable" | "SurfaceBuildMismatch"
        | "SurfaceProcessAlreadyManaged" | "SurfaceProcessUnavailable" | "SurfaceProcessStateUnavailable"
        | "SurfaceExitUnconfirmed" | "SurfaceHelperNotInstalled" | "SurfaceVerifiedBuildRequired"
        | "SurfaceShutdownUnacknowledged" | "SurfaceGrantRevocationUnconfirmed" | "SurfaceTransportClosed"
        | "SurfaceFocusGrantDenied" | "SurfaceFocusUnavailable" | "SurfaceActivationUnavailable"
        | "SurfaceSelectionChanged" | "SurfaceActionRepeated" | "SurfaceActionTimeout" | "SurfaceCopyFailed" | "SurfacePasteFailed"
        | "SurfaceHostDeadline" | "ClipboardHostInstanceChanged" | "ClipboardHostUnavailable"
        | "ClipboardHostPermissionDenied" | "ClipboardHostBusy" | "ClipboardRecordUnavailable"
        | "PrivacyStateUnavailable" | "PrivacyEpochChanged" | "PrivacySessionActive" | "SensitiveAccessDenied"
        | "host_unavailable" | "host_handshake_rejected" | "state_changed"
        | "show_failed" | "show_rejected" | "resync_required" | "initialize_rejected"
        | "hide_rejected" | "shutdown_rejected" | "action_timeout" => value,
        _ => "unclassified",
    }
}
#[derive(Serialize)]
pub struct SurfaceStatus {
    pub renderer: &'static str,
    pub managed_pid: Option<u32>,
    pub session_id: Option<String>,
    pub visible: bool,
    pub last_error: Option<String>,
}
fn error(_: impl std::fmt::Display) -> String {
    "SurfaceTransportUnavailable".into()
}
fn id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}
fn message(
    kind: SurfaceControlType,
    payload: impl Serialize,
) -> Result<SurfaceControlMessage, String> {
    Ok(SurfaceControlMessage {
        protocol_version: VERSION,
        request_id: id(),
        r#type: kind,
        payload: serde_json::to_value(payload).map_err(error)?,
    })
}
fn decode<T: DeserializeOwned>(value: serde_json::Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(error)
}
fn own_build() -> Result<SurfaceBuildIdentity, String> {
    let build = super::build_info::get_build_info();
    if build.source_state != "verified_checkout" {
        return Err("SurfaceVerifiedBuildRequired".into());
    }
    let identity = SurfaceBuildIdentity {
        component_version: build.component_version,
        source_revision: build
            .source_revision
            .ok_or("SurfaceVerifiedBuildRequired")?,
        build_id: build.build_id.ok_or("SurfaceVerifiedBuildRequired")?,
        target: build.target,
    };
    if !surface_control::same_build(&identity, &identity) {
        return Err("SurfaceBuildMismatch".into());
    }
    Ok(identity)
}
fn helper() -> Result<PathBuf, String> {
    let current = std::env::current_exe().map_err(error)?;
    let path = current.with_file_name(format!("dzc-surface{}", std::env::consts::EXE_SUFFIX));
    let metadata = std::fs::symlink_metadata(&path).map_err(|_| "SurfaceHelperNotInstalled")?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("SurfaceHelperNotInstalled".into());
    }
    Ok(path)
}
fn environment() -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
    SURFACE_ENV_KEYS.iter().copied()
    .filter_map(|key| std::env::var_os(key).map(|value| (key.into(), value)))
    .collect()
}
fn appearance(app: &AppHandle) -> (String, SurfaceTheme) {
    let settings = app.state::<crate::database::DbState>();
    let read = |key: &str| settings.settings_repo.get(key).ok().flatten();
    let locale = read("app.language")
        .filter(|value| value.len() <= 32)
        .unwrap_or_else(|| "zh".into());
    let mode = match read("app.color_mode").as_deref() {
        Some("dark") => "dark",
        Some("light") => "light",
        _ => {
            if super::commands::ui_cmd::get_system_theme_mode() == "dark" {
                "dark"
            } else {
                "light"
            }
        }
    };
    let opacity = read("app.surface_opacity")
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite())
        .map(|value| value.clamp(0.0, 100.0) / 100.0)
        .unwrap_or(0.65);
    (
        locale,
        SurfaceTheme {
            mode: mode.into(),
            opacity,
        },
    )
}
fn geometry(app: &AppHandle) -> Result<(SurfacePhysicalRect, f64), String> {
    let cursor = app.cursor_position().map_err(error)?;
    let monitor = app
        .monitor_from_point(cursor.x, cursor.y)
        .map_err(error)?
        .ok_or("SurfaceGeometryUnavailable")?;
    let scale = monitor.scale_factor();
    if !scale.is_finite() || scale <= 0.0 {
        return Err("SurfaceGeometryUnavailable".into());
    }
    let size = monitor.size();
    let origin = monitor.position();
    let width = (480.0 * scale)
        .round()
        .min(f64::from(size.width))
        .clamp(1.0, 32768.0) as u32;
    let height = (360.0 * scale)
        .round()
        .min(f64::from(size.height))
        .clamp(1.0, 32768.0) as u32;
    let x = (cursor.x - f64::from(width) / 2.0).round().clamp(
        f64::from(origin.x),
        f64::from(origin.x) + f64::from(size.width - width),
    );
    let y = (cursor.y + 12.0).round().clamp(
        f64::from(origin.y),
        f64::from(origin.y) + f64::from(size.height - height),
    );
    if x < f64::from(i32::MIN)
        || x > f64::from(i32::MAX)
        || y < f64::from(i32::MIN)
        || y > f64::from(i32::MAX)
    {
        return Err("SurfaceGeometryUnavailable".into());
    }
    Ok((
        SurfacePhysicalRect {
            x: x as i64,
            y: y as i64,
            width,
            height,
        },
        scale,
    ))
}
impl Managed {
    fn trace_restore(&self, foreground: RestoreForeground) {
        if self.diagnostic_lines.fetch_update(Ordering::Relaxed, Ordering::Relaxed,
            |count| (count < 256).then_some(count + 1)).is_ok() {
            eprintln!("[surface] stage=restore_foreground pid={} elapsed_ms={} foreground={}", self.pid,
                self.created.elapsed().as_millis(), foreground.label());
        }
    }
    fn trace(&self, stage: &'static str, outcome: Option<&str>) {
        if self.diagnostic_lines.fetch_update(Ordering::Relaxed, Ordering::Relaxed,
            |count| (count < 256).then_some(count + 1)).is_ok() {
            eprintln!("[surface] stage={} pid={} elapsed_ms={} code={}", stage, self.pid,
                self.created.elapsed().as_millis(), outcome.map(diagnostic_code).unwrap_or("ok"));
        }
    }
    async fn send(&self, message: &SurfaceControlMessage) -> Result<(), String> {
        let bytes = surface_control::encode(message, Direction::ParentToChild).map_err(error)?;
        tokio::time::timeout(TIMEOUT, async {
            let mut guard = self.writer.lock().await;
            let writer = guard.as_mut().ok_or("SurfaceTransportClosed")?;
            writer
                .write_all(&(bytes.len() as u32).to_be_bytes())
                .await
                .map_err(error)?;
            writer.write_all(&bytes).await.map_err(error)?;
            writer.flush().await.map_err(error)
        })
        .await
        .map_err(|_| "SurfaceWriteTimeout".to_owned())?
    }
    async fn exchange(
        &self,
        request: SurfaceControlMessage,
        expected: SurfaceControlType,
    ) -> Result<SurfaceControlMessage, String> {
        self.exchange_until(request, expected, Instant::now() + TIMEOUT).await
    }
    async fn exchange_until(
        &self,
        request: SurfaceControlMessage,
        expected: SurfaceControlType,
        deadline: Instant,
    ) -> Result<SurfaceControlMessage, String> {
        if Instant::now() >= deadline {
            return Err("SurfaceAcknowledgementTimeout".into());
        }
        let (send, receive) = oneshot::channel();
        {
            let mut replies = self.replies.lock().map_err(error)?;
            if replies.len() >= 4 {
                return Err("SurfaceRequestsBusy".into());
            }
            replies.insert(request.request_id.clone(), send);
        }
        let _pending = PendingReply { replies: &self.replies, request_id: &request.request_id };
        let result = tokio::time::timeout(deadline.saturating_duration_since(Instant::now()), async {
            self.send(&request).await?;
            let response = receive.await.map_err(error)?;
            if response.r#type != expected || Instant::now() >= deadline {
                return Err("SurfaceAcknowledgementRejected".into());
            }
            Ok(response)
        })
        .await
        .map_err(|_| "SurfaceAcknowledgementTimeout".to_owned())
        .and_then(|result| result);
        result
    }
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        if let Ok(mut view) = self.view.lock() {
            view.cancel();
        }
        if let Ok(mut target) = self.target.lock() {
            *target = None;
        }
    }
    async fn stop(&self, reason: &str) -> Result<(), String> {
        self.cancel();
        let request = message(
            SurfaceControlType::Shutdown,
            SurfaceShutdown {
                surface_session_id: self.id.clone(),
                reason: reason.into(),
            },
        )?;
        let acknowledged = self
            .exchange(request, SurfaceControlType::Stopped)
            .await
            .ok()
            .and_then(|reply| decode::<SurfaceStopped>(reply.payload).ok())
            .is_some_and(|reply| reply.surface_session_id == self.id);
        self.writer.lock().await.take();
        let mut child = self.child.lock().await;
        let exited = tokio::time::timeout(TIMEOUT, child.wait()).await;
        if !matches!(exited, Ok(Ok(_))) {
            child.start_kill().map_err(error)?;
            tokio::time::timeout(TIMEOUT, child.wait())
                .await
                .map_err(|_| "SurfaceExitUnconfirmed")?
                .map_err(error)?;
        }
        self.process_identity.lock().map_err(error)?.take();
        if !acknowledged {
            return Err("SurfaceShutdownUnacknowledged".into());
        }
        Ok(())
    }
}
impl SurfaceSupervisor {
    pub fn status(&self, app: &AppHandle) -> Result<SurfaceStatus, String> {
        let process = self.current()?;
        Ok(SurfaceStatus {
            renderer: if enabled(app) { "mygo" } else { "tauri" },
            managed_pid: process.as_ref().map(|process| process.pid),
            session_id: process.as_ref().map(|process| process.id.clone()),
            visible: self.visible(),
            last_error: self.last_error.lock().map_err(error)?.clone(),
        })
    }
    pub fn record_failure(&self, code: &str) {
        if let Ok(mut error) = self.last_error.lock() {
            *error = Some(code.into());
        }
        if code == "paste_unavailable" {
            if let Ok(mut notice) = self.pending_notice.lock() {
                *notice = Some(code.into());
            }
        }
    }
    fn pending_notice(&self) -> Option<String> {
        self.pending_notice
            .lock()
            .ok()
            .and_then(|notice| notice.clone())
    }
    fn consume_notice(&self, shown: &str) {
        if let Ok(mut notice) = self.pending_notice.lock() {
            if notice.as_deref() == Some(shown) {
                notice.take();
            }
        }
    }
    fn current_revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }
    fn claim_cancel(&self, expected: u64) -> Option<u64> {
        let _commit = self.route_commit.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let next = expected.checked_add(1)?;
        self.revision
            .compare_exchange(expected, next, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| next)
    }
    fn claim_failure<T>(&self, revision: u64, owner: &Option<Arc<T>>, current: &Option<Arc<T>>) -> Option<u64> {
        let matches = match (owner, current) {
            (Some(owner), Some(current)) => Arc::ptr_eq(owner, current),
            (None, None) => true,
            _ => false,
        };
        if matches { self.claim_cancel(revision) } else { None }
    }
    fn request_current(&self, revision: u64) -> bool {
        revision != u64::MAX && self.current_revision() == revision
    }
    pub fn begin_request(&self) -> Result<SurfaceRequest, String> {
        let mut request = SurfaceRequest { revision: self.invalidate(), owner: None, display: None, target: None };
        request.bind(self.current()?)?;
        Ok(request)
    }
    fn mark_hidden(&self, revision: u64) {
        let _ = self
            .visible
            .compare_exchange(revision, 0, Ordering::AcqRel, Ordering::Acquire);
        let _ = self
            .requested
            .compare_exchange(revision, 0, Ordering::AcqRel, Ordering::Acquire);
    }
    fn invalidate(&self) -> u64 {
        let _commit = self.route_commit.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        self.revision
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1)
            })
            .map(|previous| previous + 1)
            .unwrap_or(u64::MAX)
    }
    fn commit_main_handoff(&self, revision: u64, deadline: Instant,
        queue_open: impl FnOnce() -> bool, cancel_pending: impl FnOnce()) -> Result<(), String> {
        let _commit = self.route_commit.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if !queue_open() || Instant::now() >= deadline || !self.request_current(revision) {
            return Err("SurfaceDisplayCancelled".into());
        }
        // A later main request invalidates this route before creating its pending
        // show. Linearize only these atomics; native actions remain on the UI queue.
        cancel_pending();
        Ok(())
    }
    pub fn visible(&self) -> bool {
        let revision = self.current_revision();
        revision != 0 && revision != u64::MAX && self.visible.load(Ordering::Acquire) == revision
    }
    pub fn is_open(&self) -> bool {
        let revision = self.current_revision();
        revision != 0 && revision != u64::MAX && self.requested.load(Ordering::Acquire) == revision
    }
    fn current(&self) -> Result<Option<Arc<Managed>>, String> {
        Ok(self.process.lock().map_err(error)?.clone())
    }
    fn clear_current(&self, process: &Arc<Managed>) -> Result<(), String> {
        let mut current = self.process.lock().map_err(error)?;
        if current
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, process))
        {
            *current = None;
        }
        Ok(())
    }
    pub fn running(&self) -> bool {
        self.current().ok().flatten().is_some()
    }
    fn valid(&self, process: &Managed, revision: u64) -> Result<(), String> {
        if revision == u64::MAX
            || process.cancelled.load(Ordering::Acquire)
            || self.current_revision() != revision
        {
            Err("SurfaceDisplayCancelled".into())
        } else {
            Ok(())
        }
    }
    async fn start(&self, app: &AppHandle, request: &mut SurfaceRequest) -> Result<Arc<Managed>, String> {
        let expected = own_build()?;
        let executable = helper()?;
        let inspected = executable.clone();
        let variables = environment();
        let raw = tauri::async_runtime::spawn_blocking(move || {
            dzc_platform::ipc::inspect_private_helper(
                &inspected,
                &["--build-info"],
                TIMEOUT,
                16_384,
                &variables,
            )
        })
        .await
        .map_err(error)?
        .map_err(error)?;
        identity::validate(&raw, &expected)?;
        let access = app.state::<DesktopHost>().surface_access().await?;
        let mut command = tokio::process::Command::new(executable);
        command.env_clear().envs(environment());
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        crate::infrastructure::surface_focus::configure_spawn(command.as_std_mut());
        let started = command.spawn().map_err(error);
        let mut child = match started {
            Ok(child) => child,
            Err(error) => {
                let _ = access.revoke().await;
                return Err(error);
            }
        };
        let child_id = child.id().unwrap_or(0);
        let process_identity = ManagedChild::register(child_id);
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let writer = child.stdin.take();
        let initialized = child_id != 0
            && process_identity.is_ok()
            && stdout.is_some()
            && stderr.is_some()
            && writer.is_some();
        let process = Arc::new(Managed {
            id: id(),
            pid: child_id,
            access,
            created: Instant::now(),
            child: AsyncMutex::new(child),
            writer: AsyncMutex::new(writer),
            replies: Mutex::new(HashMap::new()),
            view: Mutex::new(state::View::default()),
            target: Mutex::new(None),
            cancelled: AtomicBool::new(false),
            process_identity: Mutex::new(process_identity.ok()),
            event_pending: AtomicBool::new(false),
            dismiss_pending: AtomicBool::new(false),
            request_revision: AtomicU64::new(request.revision),
            diagnostic_lines: AtomicU64::new(0),
        });
        request.bind(Some(process.clone()))?;
        *self.process.lock().map_err(error)? = Some(process.clone());
        if let Some(stdout) = stdout {
            tauri::async_runtime::spawn(read_stdout(app.clone(), Arc::downgrade(&process), stdout));
        }
        if let Some(stderr) = stderr {
            tauri::async_runtime::spawn(read_stderr(app.clone(), Arc::downgrade(&process), stderr));
        }
        if !initialized {
            if self.stop_current("disabled", true).await.is_ok() {
                request.bind(None)?;
            }
            return Err("SurfaceStartupUnavailable".into());
        }
        let (locale, theme) = appearance(app);
        let response = process
            .exchange(
                message(
                    SurfaceControlType::Initialize,
                    SurfaceInitialize {
                        surface_session_id: process.id.clone(),
                        endpoint: process.access.endpoint.clone(),
                        grant: process.access.grant.clone(),
                        state: process.access.initial_state.clone(),
                        build: expected.clone(),
                        locale,
                        theme,
                    },
                )?,
                SurfaceControlType::Ready,
            )
            .await?;
        let ready: SurfaceReady = decode(response.payload)?;
        if ready.surface_session_id != process.id
            || !surface_control::same_build(&ready.build, &expected)
        {
            return Err("SurfaceBuildMismatch".into());
        }
        process.access.check(&ready.state).await?;
        Ok(process)
    }
    pub async fn show_request(&self, app: &AppHandle, request: &mut SurfaceRequest) -> Result<(), String> {
        let revision = request.revision;
        self.requested.fetch_max(revision, Ordering::AcqRel);
        // Capture before helper startup can create any native window.
        let target = Arc::new(Target::capture()?);
        request.target = Some(target.clone());
        let capture_generation = self.revision.clone();
        target.prepare_request_time(move || {
            revision != u64::MAX && capture_generation.load(Ordering::Acquire) == revision
        }).await?;
        let (rect, scale) = geometry(app)?;
        let _serial = self.serial.lock().await;
        if !self.request_current(revision) { return Err("SurfaceDisplayCancelled".into()); }
        request.bind(self.current()?)?;
        let existing = self.current()?;
        if let Some(process) = existing.as_ref() {
            let exhausted = process.view.lock().map_err(error)?.exhausted();
            if process.created.elapsed() > Duration::from_secs(540)
                || process.cancelled.load(Ordering::Acquire)
                || exhausted
            {
                self.stop_current("disabled", true).await?;
                request.bind(None)?;
            }
        }
        let existing = self.current()?;
        let process = if let Some(process) = existing {
            process
        } else {
            self.start(app, request).await?
        };
        request.bind(Some(process.clone()))?;
        process.request_revision.store(revision, Ordering::Release);
        self.valid(&process, revision)?;
        let state = process.access.state().await?;
        self.valid(&process, revision)?;
        if !target.context_current() {
            return Err("SurfaceFocusChanged".into());
        }
        let (locale, theme) = appearance(app);
        let notice = self.pending_notice();
        let display = id();
        *process.target.lock().map_err(error)? = Some(target.clone());
        process.view.lock().map_err(error)?.show(state::Display {
            id: display.clone(),
            state: state.clone(),
            revision,
            shown: false,
        });
        request.display = Some(display.clone());
        let focus_deadline = target.presentation_deadline(TIMEOUT)?;
        let (handoff_send, handoff_receive) = oneshot::channel();
        let handoff_app = app.clone();
        let handoff_process = process.clone();
        let handoff_target = target.clone();
        app.run_on_main_thread(move || {
            let result = (|| {
                if handoff_send.is_closed() || Instant::now() >= focus_deadline {
                    return Err("SurfaceDisplayCancelled".into());
                }
                handoff_app.state::<SurfaceSupervisor>().valid(&handoff_process, revision)?;
                if !handoff_target.context_current() { return Err("SurfaceFocusChanged".into()); }
                crate::app::window_manager::hide_main_for_quick_paste(
                    &handoff_app, revision, focus_deadline, || !handoff_send.is_closed())
            })();
            let _ = handoff_send.send(result);
        }).map_err(error)?;
        tokio::time::timeout_at(tokio::time::Instant::from_std(focus_deadline), handoff_receive)
            .await.map_err(|_| "SurfaceFocusUnavailable".to_owned())?.map_err(error)??;
        self.valid(&process, revision)?;
        target.authorize_managed(process.pid, || self.valid(&process, revision).is_ok())?;
        eprintln!("[surface] focus-authorized pid={} revision={}", process.pid, revision);
        let response = process
            .exchange_until(
                message(
                    SurfaceControlType::Show,
                    SurfaceShow {
                        surface_session_id: process.id.clone(),
                        display_request_id: display.clone(),
                        state,
                        rect,
                        scale,
                        locale,
                        theme,
                        notice: notice.clone(),
                    },
                )?,
                SurfaceControlType::Shown,
                focus_deadline,
            )
            .await?;
        let shown: SurfaceShown = decode(response.payload)?;
        process.trace("show_visibility_reply", None);
        self.valid(&process, revision)?;
        if shown.surface_session_id != process.id
            || !process.view.lock().map_err(error)?.matches(
                &shown.display_request_id,
                revision,
                &shown.state,
            )
        {
            return Err("SurfaceStaleAcknowledgement".into());
        }
        tokio::time::timeout(focus_deadline.saturating_duration_since(Instant::now()), process.access.check(&shown.state))
            .await.map_err(|_| "SurfaceFocusUnavailable".to_owned())??;
        self.valid(&process, revision)?;
        // Shown acknowledges native visibility. Public success also requires
        // actual foreground ownership within this same presentation deadline.
        let activation_target = target.clone();
        process.trace("activation_prepare", None);
        let activation_process = process.clone();
        let activation_generation = self.revision.clone();
        let activation = tokio::time::timeout(focus_deadline.saturating_duration_since(Instant::now()), tauri::async_runtime::spawn_blocking(move || {
            activation_target.prepare_managed_activation(activation_process.pid, focus_deadline, move || {
                !activation_process.cancelled.load(Ordering::Acquire) && activation_generation.load(Ordering::Acquire) == revision
            })
        })).await.map_err(|_| "SurfaceFocusUnavailable".to_owned())?.map_err(error)??;
        self.valid(&process, revision)?;
        tokio::time::timeout(focus_deadline.saturating_duration_since(Instant::now()), process.access.check(&shown.state))
            .await.map_err(|_| "SurfaceFocusUnavailable".to_owned())??;
        self.valid(&process, revision)?;
        let activation_target = target.clone();
        process.trace("activation_send", None);
        let activation_process = process.clone();
        let activation_generation = self.revision.clone();
        tokio::time::timeout(focus_deadline.saturating_duration_since(Instant::now()), tauri::async_runtime::spawn_blocking(move || {
            activation_target.activate_managed_once(activation, activation_process.pid, focus_deadline, move || {
                !activation_process.cancelled.load(Ordering::Acquire) && activation_generation.load(Ordering::Acquire) == revision
            })
        })).await.map_err(|_| "SurfaceFocusUnavailable".to_owned())?.map_err(error)??;
        self.valid(&process, revision)?;
        let focus_target = target.clone();
        process.trace("activation_focus_wait", None);
        let focus_process = process.clone();
        let generation = self.revision.clone();
        tokio::time::timeout(focus_deadline.saturating_duration_since(Instant::now()), tauri::async_runtime::spawn_blocking(move || {
            focus_target.wait_managed_focus(focus_process.pid, focus_deadline, || {
                !focus_process.cancelled.load(Ordering::Acquire) && generation.load(Ordering::Acquire) == revision
            })
        })).await.map_err(|_| "SurfaceFocusUnavailable".to_owned())?.map_err(error)??;
        self.valid(&process, revision)?;
        tokio::time::timeout(focus_deadline.saturating_duration_since(Instant::now()), process.access.check(&shown.state))
            .await.map_err(|_| "SurfaceFocusUnavailable".to_owned())??;
        self.valid(&process, revision)?;
        let focus_target = target.clone();
        process.trace("activation_focus_recheck", None);
        let focus_process = process.clone();
        let generation = self.revision.clone();
        tokio::time::timeout(focus_deadline.saturating_duration_since(Instant::now()), tauri::async_runtime::spawn_blocking(move || {
            focus_target.wait_managed_focus(focus_process.pid, focus_deadline, || {
                !focus_process.cancelled.load(Ordering::Acquire) && generation.load(Ordering::Acquire) == revision
            })
        })).await.map_err(|_| "SurfaceFocusUnavailable".to_owned())?.map_err(error)??;
        self.valid(&process, revision)?;
        if Instant::now() >= focus_deadline { return Err("SurfaceFocusUnavailable".into()); }
        if !process.view.lock().map_err(error)?.presented(&shown.display_request_id, revision, &shown.state) {
            return Err("SurfaceStaleAcknowledgement".into());
        }
        eprintln!("[surface] focus-confirmed pid={} revision={}", process.pid, revision);
        self.visible.fetch_max(revision, Ordering::AcqRel);
        self.last_error.lock().map_err(error)?.take();
        if let Some(notice) = notice {
            self.consume_notice(&notice);
        }
        Ok(())
    }
    async fn hide_display(
        &self,
        process: &Managed,
        display: &str,
        reason: &str,
    ) -> Result<(), String> {
        self.hide_display_until(process, display, reason, Instant::now() + TIMEOUT).await
    }
    async fn hide_display_until(
        &self, process: &Managed, display: &str, reason: &str, deadline: Instant,
    ) -> Result<(), String> {
        let response = process
            .exchange_until(
                message(
                    SurfaceControlType::Hide,
                    SurfaceHide {
                        surface_session_id: process.id.clone(),
                        display_request_id: display.into(),
                        reason: reason.into(),
                    },
                )?,
                SurfaceControlType::Hidden,
                deadline,
            )
            .await?;
        let hidden: SurfaceHidden = decode(response.payload)?;
        if hidden.surface_session_id != process.id || hidden.display_request_id != display {
            return Err("SurfaceStaleAcknowledgement".into());
        }
        Ok(())
    }
    pub async fn hide(&self) -> Result<(), String> {
        let mut request = self.begin_request()?;
        self.hide_request(&mut request).await
    }
    pub async fn hide_request(&self, request: &mut SurfaceRequest) -> Result<(), String> {
        let deadline = Instant::now() + TIMEOUT;
        let revision = request.revision;
        let _serial = action_until(deadline, async { Ok(self.serial.lock().await) }).await?;
        if !self.request_current(revision) { return Err("SurfaceDisplayCancelled".into()); }
        let process = self.current()?;
        request.bind(process.clone())?;
        if let Some(process) = process {
            process.request_revision.store(revision, Ordering::Release);
            request.target = process.target.lock().map_err(error)?.clone();
            let display = process.view.lock().map_err(error)?.display.clone();
            if let Some(display) = display {
                self.hide_display_until(&process, &display.id, "cancel", deadline).await?;
                self.valid(&process, revision)?;
                action_budget(deadline)?;
                process.view.lock().map_err(error)?.cancel_matching(&display.id);
            }
            let target = process.target.lock().map_err(error)?.take();
            if let Some(target) = target {
                self.restore_target(process.clone(), target, revision, deadline).await?;
            }
        }
        Ok(())
    }
    pub async fn failed_request(&self, request: &SurfaceRequest, failure: &str, closing: bool) -> Result<Option<SurfaceFallback>, String> {
        eprintln!("[surface] route-error revision={} current={} pid={} closing={} code={}", request.revision,
            self.current_revision(), request.owner.as_ref().map(|process| process.pid).unwrap_or(0), closing, diagnostic_code(failure));
        if failure == "SurfaceDisplayCancelled" { return Ok(None); }
        let _serial = self.serial.lock().await;
        let current = self.current()?;
        let Some(ticket) = self.claim_failure(request.revision, &request.owner, &current) else {
            eprintln!("[surface] route-error-superseded revision={}", request.revision);
            return Ok(None);
        };
        if failure == "SurfaceFocusChanged" || request.target.as_ref().is_some_and(|target| !target.presentation_current()) {
            if let (Some(process), Some(display)) = (current.as_ref(), request.display.as_ref()) {
                let owned_display = process.view.lock().map_err(error)?.display.as_ref()
                    .is_some_and(|current| &current.id == display);
                if owned_display {
                    if let Err(error) = self.hide_display(process, display, FOCUS_CANCEL_REASON).await {
                        self.stop_current("disabled", true).await?;
                        return Err(error);
                    }
                    if process.view.lock().map_err(error)?.cancel_matching(display).is_some() {
                        process.target.lock().map_err(error)?.take();
                    }
                }
            }
            return Ok(None);
        }
        self.record_failure("renderer_unavailable");
        if current.is_some() { self.stop_current("disabled", true).await?; }
        if self.running() { return Err("SurfaceExitUnconfirmed".into()); }
        Ok((!closing && self.request_current(ticket)).then(|| SurfaceFallback {
            revision: ticket, target: request.target.clone(), original_tauri: false, deadline: Instant::now() + TIMEOUT,
            #[cfg(target_os = "linux")] presentation: None,
        }))
    }
    async fn restore_target(&self, process: Arc<Managed>, target: Arc<Target>, revision: u64, deadline: Instant) -> Result<(), String> {
        let generation = self.revision.clone();
        action_budget(deadline)?;
        action_until(deadline, async move {
            tauri::async_runtime::spawn_blocking(move || {
                let restore_process = process.clone();
                target.restore_if(deadline, move || {
                    revision != u64::MAX && generation.load(Ordering::Acquire) == revision
                        && !restore_process.cancelled.load(Ordering::Acquire)
                }, |foreground| process.trace_restore(foreground))
            }).await.map_err(error)?
        }).await
    }
    async fn dismissed(
        &self,
        process: Arc<Managed>,
        request: SurfaceDismiss,
        revision: u64,
        deadline: Instant,
    ) -> Result<(), String> {
        let _serial = action_until(deadline, async { Ok(self.serial.lock().await) }).await?;
        if self.current_revision() != revision
            || !self
                .current()?
                .is_some_and(|current| Arc::ptr_eq(&current, &process))
        {
            return Ok(());
        }
        if request.surface_session_id != process.id {
            return Ok(());
        }
        let display = process
            .view
            .lock()
            .map_err(error)?
            .cancel_matching(&request.display_request_id);
        if display.is_none() {
            return Ok(());
        }
        let target = process.target.lock().map_err(error)?.take();
        if display
            .as_ref()
            .is_some_and(|display| display.id == request.display_request_id)
            && state::restores_target(&request.reason, revision, self.current_revision())
        {
            if let Some(target) = target {
                self.restore_target(process.clone(), target, revision, deadline).await?;
            }
        }
        Ok(())
    }
    async fn stop_current(&self, reason: &str, revoke: bool) -> Result<(), String> {
        let process = self.current()?;
        let Some(process) = process else {
            return Ok(());
        };
        process.cancel();
        // Grant revocation is attempted independently of actual process exit.
        // A dead transport never permits leaving an unmanaged executable alive.
        let revoked = if revoke {
            tokio::time::timeout(TIMEOUT, process.access.revoke())
                .await
                .is_ok_and(|result| result.is_ok())
        } else {
            true
        };
        let stopped = process.stop(reason).await;
        let confirmed = process
            .child
            .lock()
            .await
            .try_wait()
            .map_err(error)?
            .is_some();
        if confirmed {
            self.clear_current(&process)?;
        }
        if !confirmed {
            return Err("SurfaceExitUnconfirmed".into());
        }
        stopped?;
        if !revoked {
            return Err("SurfaceGrantRevocationUnconfirmed".into());
        }
        Ok(())
    }
    pub async fn stop(&self, reason: &str, revoke: bool) -> Result<(), String> {
        self.invalidate();
        let _serial = self.serial.lock().await;
        self.stop_current(reason, revoke).await
    }
    async fn stop_matching(&self, process: &Arc<Managed>, reason: &str, unexpected: bool) -> Result<(), String> {
        let _serial = self.serial.lock().await;
        if !self.current()?.is_some_and(|current| Arc::ptr_eq(&current, process)) {
            return Ok(());
        }
        self.mark_hidden(process.request_revision.load(Ordering::Acquire));
        if unexpected { self.record_failure("renderer_unavailable"); }
        self.stop_current(reason, true).await
    }
    async fn stop_owned(&self, reason: &str) -> Result<(), String> {
        // Called while the embedded Host may own its request lock. Do not wait
        // for a parent action or send a recursive Host IPC request here.
        self.invalidate();
        let Some(process) = self.current()? else {
            return Ok(());
        };
        process.cancel();
        let stopped = process.stop(reason).await;
        if process
            .child
            .lock()
            .await
            .try_wait()
            .map_err(error)?
            .is_some()
        {
            self.clear_current(&process)?;
        } else {
            return Err("SurfaceExitUnconfirmed".into());
        }
        stopped
    }
    async fn perform_action(
        &self,
        process: &Arc<Managed>,
        request: &SurfacePaste,
        deadline: Instant,
    ) -> Result<(), String> {
        process.trace("paste_serial_wait", None);
        let _serial = action_until(deadline, async { Ok(self.serial.lock().await) }).await?;
        let revision = self.current_revision();
        self.valid(&process, revision)?;
        process.trace("paste_host_state", None);
        let actual = action_until(deadline, process.access.state()).await?;
        self.valid(&process, revision)?;
        let display =
            process
                .view
                .lock()
                .map_err(error)?
                .admit(&process.id, revision, &request, &actual)?;
        process.trace("paste_admitted", None);
        self.mark_hidden(revision);
        let target = process
            .target
            .lock()
            .map_err(error)?
            .clone()
            .ok_or("SurfaceTargetUnavailable")?;
        self.hide_display_until(process, &display.id, "paste", deadline).await?;
        action_budget(deadline)?;
        process.trace("paste_hidden", None);
        self.valid(process, revision)?;
        action_until(deadline, process.access.check(&request.state)).await?;
        self.valid(process, revision)?;
        process.trace("paste_restore", None);
        self.restore_target(process.clone(), target.clone(), revision, deadline).await?;
        process.trace("paste_copy", None);
        self.valid(&process, revision)?;
        action_until(deadline, process.access.copy(request.entry_id, &request.state)).await?;
        self.valid(process, revision)?;
        process.trace("paste_copy_completed", None);
        action_until(deadline, process.access.check(&request.state)).await?;
        self.valid(&process, revision)?;
        process.trace("paste_native_input", None);
        // No queued worker can outlive the last cancellation/focus check.
        if !target.is_current() {
            return Err("SurfaceFocusChanged".into());
        }
        self.valid(&process, revision)?;
        action_budget(deadline)?;
        dzc_platform::runtime::NativePasteController
            .paste()
            .map_err(|_| "SurfacePasteFailed".to_owned())?;
        Ok(())
    }
    async fn action(
        &self,
        process: Arc<Managed>,
        request_id: String,
        request: SurfacePaste,
        deadline: Instant,
    ) -> Result<(), String> {
        let performed = self.perform_action(&process, &request, deadline).await;
        process.trace("paste_result", performed.as_ref().err().map(String::as_str));
        {
            let _serial = self.serial.lock().await;
            let revision = self.current_revision();
            let mut view = process.view.lock().map_err(error)?;
            let mut target = process.target.lock().map_err(error)?;
            state::complete_selection(
                &mut view,
                &mut target,
                &request.display_request_id,
                revision,
            );
        }
        let response = SurfaceControlMessage {
            protocol_version: VERSION,
            request_id,
            r#type: SurfaceControlType::PasteResult,
            payload: serde_json::to_value(SurfacePasteResult {
                surface_session_id: process.id.clone(),
                display_request_id: request.display_request_id,
                action_id: request.action_id,
                performed: performed.is_ok(),
                error_code: performed.as_ref().err().map(|_| "paste_unavailable".into()),
            })
            .map_err(error)?,
        };
        if process.send(&response).await.is_err() {
            let _ = self.stop_matching(&process, "disabled", true).await;
        }
        // A lost reply after a completed native input never changes success to
        // failure and never schedules an automatic second paste.
        performed
    }
}
async fn read_stdout(
    app: AppHandle,
    process: Weak<Managed>,
    mut stream: tokio::process::ChildStdout,
) {
    loop {
        let result = async {
            let first = stream.read_u8().await.map_err(error)?;
            tokio::time::timeout(TIMEOUT, async {
                let mut header = [first, 0, 0, 0];
                stream.read_exact(&mut header[1..]).await.map_err(error)?;
                let length = u32::from_be_bytes(header) as usize;
                if length == 0 || length > surface_control::MAX_FRAME_BYTES {
                    return Err("SurfaceFrameTooLarge".into());
                }
                let mut bytes = vec![0; length];
                stream.read_exact(&mut bytes).await.map_err(error)?;
                surface_control::decode(&bytes, Direction::ChildToParent).map_err(error)
            })
            .await
            .map_err(|_| "SurfaceReadTimeout".to_owned())?
        }
        .await;
        let Some(process) = process.upgrade() else {
            return;
        };
        let message = match result {
            Ok(message) => message,
            Err(failure) => {
                eprintln!("[surface] stdout-error pid={} code={}", process.pid, diagnostic_code(&failure));
                schedule_stop(&app, &process, "stdout_error");
                return;
            }
        };
        if matches!(
            message.r#type,
            SurfaceControlType::Ready
                | SurfaceControlType::Shown
                | SurfaceControlType::Hidden
                | SurfaceControlType::Stopped
                | SurfaceControlType::Failed
        ) {
            let reply = process
                .replies
                .lock()
                .ok()
                .and_then(|mut replies| replies.remove(&message.request_id));
            if message.r#type == SurfaceControlType::Failed {
                let failed = decode::<SurfaceFailed>(message.payload.clone()).ok();
                eprintln!("[surface] child-failed pid={} matched_reply={} code={}", process.pid, reply.is_some(),
                    diagnostic_code(failed.as_ref().map(|failed| failed.error_code.as_str()).unwrap_or("unclassified")));
            }
            if let Some(reply) = reply {
                let _ = reply.send(message);
            } else if message.r#type == SurfaceControlType::Failed {
                schedule_stop(&app, &process, "unsolicited_failed");
                return;
            }
            continue;
        }
        if process.cancelled.load(Ordering::Acquire) {
            continue;
        }
        match message.r#type {
            SurfaceControlType::Paste => {
                let deadline = Instant::now() + TIMEOUT;
                let request = decode::<SurfacePaste>(message.payload);
                let app = app.clone();
                if let Ok(request) = request {
                    if !process.event_pending.swap(true, Ordering::AcqRel) {
                        process.trace("paste_received", None);
                        tauri::async_runtime::spawn(async move {
                            let manager = app.state::<SurfaceSupervisor>();
                            let outcome = manager
                                .action(process.clone(), message.request_id, request, deadline)
                                .await;
                            if outcome
                                .as_ref()
                                .is_err_and(|error| error != "SurfaceDisplayCancelled")
                            {
                                manager.record_failure("paste_unavailable");
                                let _ = app.emit_to(
                                    "main",
                                    "quick-paste-action-failed",
                                    "paste_unavailable",
                                );
                                notify_main_shown(&app);
                            }
                            process.event_pending.store(false, Ordering::Release);
                        });
                    }
                }
            }
            SurfaceControlType::Dismiss => {
                let deadline = Instant::now() + TIMEOUT;
                if let Ok(request) = decode::<SurfaceDismiss>(message.payload) {
                    let expected = process.view.lock().ok().and_then(|view| {
                        view.display
                            .as_ref()
                            .filter(|display| {
                                display.id == request.display_request_id
                                    && request.surface_session_id == process.id
                            })
                            .map(|display| display.revision)
                    });
                    if let Some(expected) = expected {
                        if !process.dismiss_pending.swap(true, Ordering::AcqRel) {
                            let manager = app.state::<SurfaceSupervisor>();
                            if let Some(revision) = manager.claim_cancel(expected) {
                                let app = app.clone();
                                tauri::async_runtime::spawn(async move {
                                    let _ = app
                                        .state::<SurfaceSupervisor>()
                                        .dismissed(process.clone(), request, revision, deadline)
                                        .await;
                                    process.dismiss_pending.store(false, Ordering::Release);
                                });
                            } else {
                                process.dismiss_pending.store(false, Ordering::Release);
                            }
                        }
                    }
                }
            }
            _ => {
                schedule_stop(&app, &process, "unexpected_message");
                return;
            }
        }
    }
}
async fn read_stderr(
    app: AppHandle,
    process: Weak<Managed>,
    mut stream: tokio::process::ChildStderr,
) {
    let mut total = 0usize;
    let mut bytes = [0; 1024];
    let mut diagnostics = diagnostics::ChildInput::default();
    loop {
        match stream.read(&mut bytes).await {
            Ok(0) | Err(_) => return,
            Ok(count) => {
                total += count;
                if total > 64 * 1024 {
                    if let Some(process) = process.upgrade() {
                        schedule_stop(&app, &process, "stderr_limit");
                    }
                    return;
                }
                if let Some(process) = process.upgrade() {
                    diagnostics.feed(&bytes[..count], |message| {
                        eprintln!("[surface-child] pid={} {}", process.pid, message);
                    });
                }
            }
        }
    }
}
fn schedule_stop(app: &AppHandle, process: &Arc<Managed>, reason: &'static str) {
    let unexpected = !process.cancelled.load(Ordering::Acquire);
    eprintln!("[surface] scheduled-stop pid={} unexpected={} reason={}", process.pid, unexpected, reason);
    process.cancel();
    let app = app.clone();
    let process = process.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(manager) = app.try_state::<SurfaceSupervisor>() {
            let _ = manager.stop_matching(&process, "disabled", unexpected).await;
        }
    });
}
pub fn enabled(app: &AppHandle) -> bool {
    app.state::<crate::database::DbState>()
        .settings_repo
        .get("app.quick_paste_renderer")
        .ok()
        .flatten()
        .as_deref()
        == Some("mygo")
}
pub fn route_ticket(app: &AppHandle) -> u64 {
    app.try_state::<SurfaceSupervisor>()
        .map(|manager| manager.current_revision())
        .unwrap_or(0)
}
pub fn route_current(app: &AppHandle, ticket: u64) -> bool {
    ticket != u64::MAX
        && app
            .try_state::<SurfaceSupervisor>()
            .map(|manager| manager.current_revision() == ticket)
            .unwrap_or(true)
        && app
            .state::<crate::app_state::SettingsState>()
            .quick_paste_enabled
            .load(Ordering::Acquire)
}
pub(crate) fn commit_main_handoff(app: &AppHandle, ticket: u64, deadline: Instant,
    queue_open: impl FnOnce() -> bool) -> Result<(), String> {
    let settings = app.state::<crate::app_state::SettingsState>();
    let manager = app.try_state::<SurfaceSupervisor>().ok_or("SurfaceDisplayCancelled")?;
    manager.commit_main_handoff(ticket, deadline,
        || queue_open() && settings.quick_paste_enabled.load(Ordering::Acquire),
        crate::app::idle_destroyer::cancel_pending_show)
}
pub fn notify_main_shown(app: &AppHandle) {
    // A hidden main toast is not feedback. Retain only the generic failure
    // until a user-visible, painted main frame or a fresh Surface show.
    if !crate::global_state::MAIN_WINDOW_PAINTED.load(Ordering::Acquire)
        || !app
            .get_webview_window("main")
            .is_some_and(|window| window.is_visible().unwrap_or(false))
    {
        return;
    }
    if let Some(manager) = app.try_state::<SurfaceSupervisor>() {
        if let Some(notice) = manager.pending_notice() {
            if app
                .emit_to("main", "toast", "quick_paste_previous_unconfirmed")
                .is_ok()
            {
                manager.consume_notice(&notice);
            }
        }
    }
}
pub fn invalidate(app: &AppHandle, reason: &'static str) {
    let process = if let Some(manager) = app.try_state::<SurfaceSupervisor>() {
        manager.invalidate();
        manager
            .current()
            .ok()
            .flatten()
            .inspect(|process| process.cancel())
    } else {
        None
    };
    let Some(process) = process else { return };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(manager) = app.try_state::<SurfaceSupervisor>() {
            let _serial = manager.serial.lock().await;
            if manager
                .current()
                .ok()
                .flatten()
                .is_some_and(|current| Arc::ptr_eq(&current, &process))
            {
                let _ = manager.stop_current(reason, true).await;
            }
        }
    });
}
pub fn stop_blocking(app: &AppHandle, reason: &'static str, revoke: bool) -> Result<(), String> {
    let app = app.clone();
    std::thread::Builder::new()
        .name("dzc-surface-stop".into())
        .spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(error)?
                .block_on(async {
                    if let Some(manager) = app.try_state::<SurfaceSupervisor>() {
                        if revoke {
                            manager.stop(reason, true).await
                        } else {
                            manager.stop_owned(reason).await
                        }
                    } else {
                        Ok(())
                    }
                })
        })
        .map_err(error)?
        .join()
        .map_err(|_| "SurfaceStopWorkerFailed".to_owned())?
}
pub fn hide_blocking(app: &AppHandle) -> Result<(), String> {
    let app = app.clone();
    std::thread::Builder::new()
        .name("dzc-surface-hide".into())
        .spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(error)?
                .block_on(async {
                    if let Some(manager) = app.try_state::<SurfaceSupervisor>() {
                        manager.hide().await
                    } else {
                        Ok(())
                    }
                })
        })
        .map_err(error)?
        .join()
        .map_err(|_| "SurfaceHideWorkerFailed".to_owned())?
}

#[cfg(test)]
mod transition_tests {
    use super::*;
    #[test]
    fn an_expired_action_budget_does_not_poll_copy_or_queue_native_input() {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let copied = std::cell::Cell::new(false);
        let native_input = std::cell::Cell::new(false);
        let result = runtime.block_on(async {
            action_until(Instant::now(), async { copied.set(true); Ok(()) }).await?;
            native_input.set(true);
            Ok::<(), String>(())
        });
        assert_eq!(result.unwrap_err(), "SurfaceActionTimeout");
        assert!(!copied.get() && !native_input.get());
    }
    #[test]
    fn a_slow_copy_consumes_the_remaining_budget_and_never_reaches_native_input() {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let native_input = std::cell::Cell::new(false);
        let dropped = std::cell::Cell::new(false);
        struct CopyPending<'a>(&'a std::cell::Cell<bool>);
        impl Drop for CopyPending<'_> { fn drop(&mut self) { self.0.set(true); } }
        let result = runtime.block_on(async {
            let deadline = Instant::now() + Duration::from_millis(5);
            action_until(deadline, async {
                let _pending = CopyPending(&dropped);
                std::future::pending::<Result<(), String>>().await
            }).await?;
            native_input.set(true);
            Ok::<(), String>(())
        });
        assert_eq!(result.unwrap_err(), "SurfaceActionTimeout");
        assert!(dropped.get());
        assert!(!native_input.get());
    }
    #[test]
    fn a_stage_completing_after_deadline_cannot_refresh_the_action_budget() {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let native_input = std::cell::Cell::new(false);
        let result = runtime.block_on(async {
            let deadline = Instant::now() + Duration::from_millis(1);
            action_until(deadline, async {
                std::thread::sleep(Duration::from_millis(5));
                Ok(())
            }).await?;
            native_input.set(true);
            Ok::<(), String>(())
        });
        assert_eq!(result.unwrap_err(), "SurfaceActionTimeout");
        assert!(!native_input.get());
    }
    #[test]
    fn a_dropped_pending_exchange_removes_its_sender_without_touching_other_requests() {
        let replies = Mutex::new(HashMap::new());
        let (first, _first_reply) = oneshot::channel();
        let (second, _second_reply) = oneshot::channel();
        replies.lock().unwrap().insert("first".into(), first);
        replies.lock().unwrap().insert("second".into(), second);
        {
            let _pending = PendingReply { replies: &replies, request_id: "first" };
            assert_eq!(replies.lock().unwrap().len(), 2);
        }
        let remaining = replies.lock().unwrap();
        assert_eq!(remaining.len(), 1);
        assert!(remaining.contains_key("second"));
    }
    fn failed_route(supervisor: &SurfaceSupervisor, request: &SurfaceRequest, failure: &str, closing: bool) -> Result<Option<SurfaceFallback>, String> {
        tokio::runtime::Builder::new_current_thread().build().unwrap()
            .block_on(supervisor.failed_request(request, failure, closing))
    }
    #[test]
    fn an_old_route_error_cannot_stop_or_hide_a_new_request() {
        let supervisor = SurfaceSupervisor::default();
        let old = supervisor.begin_request().unwrap();
        let current = supervisor.begin_request().unwrap();
        supervisor.requested.store(current.revision, Ordering::Release);
        supervisor.visible.store(current.revision, Ordering::Release);
        assert!(failed_route(&supervisor, &old, "SurfaceTransportUnavailable", true).unwrap().is_none());
        assert!(supervisor.request_current(current.revision));
        assert!(supervisor.visible() && supervisor.is_open());
        assert!(supervisor.last_error.lock().unwrap().is_none());
    }
    #[test]
    fn failed_route_cleanup_requires_the_exact_original_managed_owner() {
        let supervisor = SurfaceSupervisor::default();
        let revision = supervisor.invalidate();
        let owner = Some(Arc::new(()));
        let replacement = Some(Arc::new(()));
        assert!(supervisor.claim_failure(revision, &owner, &replacement).is_none());
        assert!(supervisor.claim_failure(revision, &None, &replacement).is_none());
        assert!(supervisor.request_current(revision));
        assert!(supervisor.claim_failure(revision, &owner, &owner).is_some());
        assert!(supervisor.claim_failure(revision, &owner, &owner).is_none());
    }
    #[test]
    fn a_fallback_keeps_its_claimed_ticket_after_a_new_request_starts() {
        let supervisor = SurfaceSupervisor::default();
        let failed = supervisor.begin_request().unwrap();
        let ticket = failed_route(&supervisor, &failed, "SurfaceStartupUnavailable", false).unwrap().unwrap();
        assert!(supervisor.request_current(ticket.revision));
        assert!(!ticket.context_current());
        let next = supervisor.begin_request().unwrap();
        assert!(!supervisor.request_current(ticket.revision));
        assert!(supervisor.request_current(next.revision));
        assert!(failed_route(&supervisor, &failed, "SurfaceStartupUnavailable", false).unwrap().is_none());
    }
    #[test]
    fn focus_change_or_closing_failure_never_requests_a_tauri_fallback() {
        for (failure, closing) in [("SurfaceFocusChanged", false), ("SurfaceTransportUnavailable", true)] {
            let supervisor = SurfaceSupervisor::default();
            let request = supervisor.begin_request().unwrap();
            assert!(failed_route(&supervisor, &request, failure, closing).unwrap().is_none());
            assert!(!supervisor.visible() && !supervisor.is_open());
        }
    }
    #[test]
    fn failure_diagnostics_never_print_unknown_child_text() {
        for code in ["SurfaceFocusChanged", "SurfaceActivationUnavailable", "state_changed", "SurfaceHelperNotInstalled",
            "SurfaceShutdownUnacknowledged", "SurfaceGrantRevocationUnconfirmed"] {
            assert_eq!(diagnostic_code(code), code);
        }
        assert_eq!(diagnostic_code("known synthetic clipboard contents"), "unclassified");
        assert_eq!(diagnostic_code("unknownbodywithoutspaces"), "unclassified");
    }
    #[test]
    fn focus_cancellation_uses_the_actual_parent_to_child_hide_schema() {
        let hide = |reason: &str| message(SurfaceControlType::Hide, SurfaceHide {
            surface_session_id: "session".into(), display_request_id: "display".into(), reason: reason.into(),
        }).unwrap();
        assert!(surface_control::encode(&hide(FOCUS_CANCEL_REASON), Direction::ParentToChild).is_ok());
        assert!(surface_control::encode(&hide("blur"), Direction::ParentToChild).is_err());
    }
    #[test]
    fn the_surface_inherits_the_x11_authority_path_but_no_database_or_signing_environment() {
        assert!(SURFACE_ENV_KEYS.contains(&"DISPLAY"));
        assert!(SURFACE_ENV_KEYS.contains(&"XAUTHORITY"));
        let unique: std::collections::HashSet<_> = SURFACE_ENV_KEYS.iter().collect();
        assert_eq!(unique.len(), SURFACE_ENV_KEYS.len());
        for excluded in ["DEZIRCLIP_DB_PATH", "GITHUB_TOKEN", "GH_TOKEN", "TAURI_SIGNING_PRIVATE_KEY",
            "TAURI_SIGNING_PRIVATE_KEY_PASSWORD", "SSH_AUTH_SOCK", "GIT_ASKPASS"] {
            assert!(!SURFACE_ENV_KEYS.contains(&excluded));
        }
    }
    #[test]
    fn a_late_old_dismiss_cannot_claim_or_hide_the_new_show() {
        let supervisor = SurfaceSupervisor::default();
        let old = supervisor.invalidate();
        supervisor.requested.fetch_max(old, Ordering::AcqRel);
        supervisor.visible.fetch_max(old, Ordering::AcqRel);
        let current = supervisor.invalidate();
        supervisor.requested.fetch_max(current, Ordering::AcqRel);
        supervisor.visible.fetch_max(current, Ordering::AcqRel);
        assert!(supervisor.claim_cancel(old).is_none());
        supervisor.mark_hidden(old);
        assert!(supervisor.visible());
        assert!(supervisor.is_open());
        assert!(supervisor.claim_cancel(current).is_some());
        assert!(!supervisor.visible());
        assert!(!supervisor.is_open());
    }
    #[test]
    fn handoff_does_not_cancel_a_main_request_that_already_superseded_it() {
        let supervisor = SurfaceSupervisor::default();
        let old = supervisor.invalidate();
        let main = supervisor.invalidate();
        let pending_main = AtomicBool::new(true);
        assert!(supervisor.commit_main_handoff(old, Instant::now() + TIMEOUT, || true,
            || pending_main.store(false, Ordering::Release)).is_err());
        assert!(pending_main.load(Ordering::Acquire));
        assert!(supervisor.request_current(main));
    }
    #[test]
    fn a_main_request_after_handoff_keeps_its_new_pending_show() {
        let supervisor = SurfaceSupervisor::default();
        let surface = supervisor.invalidate();
        let pending_main = AtomicBool::new(true);
        supervisor.commit_main_handoff(surface, Instant::now() + TIMEOUT, || true,
            || pending_main.store(false, Ordering::Release)).unwrap();
        assert!(!pending_main.load(Ordering::Acquire));
        // Main invalidates the Surface route before beginning its own show.
        let main = supervisor.invalidate();
        pending_main.store(true, Ordering::Release);
        assert!(supervisor.commit_main_handoff(surface, Instant::now() + TIMEOUT, || true,
            || pending_main.store(false, Ordering::Release)).is_err());
        assert!(pending_main.load(Ordering::Acquire));
        assert!(supervisor.request_current(main));
    }
    #[test]
    fn an_expired_or_abandoned_queued_handoff_never_cancels_main() {
        let supervisor = SurfaceSupervisor::default();
        let surface = supervisor.invalidate();
        let pending_main = AtomicBool::new(true);
        assert!(supervisor.commit_main_handoff(surface, Instant::now(), || true,
            || pending_main.store(false, Ordering::Release)).is_err());
        let (send, receive) = oneshot::channel::<()>();
        drop(receive);
        assert!(supervisor.commit_main_handoff(surface, Instant::now() + TIMEOUT, || !send.is_closed(),
            || pending_main.store(false, Ordering::Release)).is_err());
        assert!(pending_main.load(Ordering::Acquire));
        assert!(supervisor.request_current(surface));
    }
    #[test]
    fn a_generic_failure_remains_pending_until_the_next_visible_delivery() {
        let supervisor = SurfaceSupervisor::default();
        supervisor.record_failure("paste_unavailable");
        assert_eq!(
            supervisor.pending_notice().as_deref(),
            Some("paste_unavailable")
        );
        supervisor.record_failure("renderer_unavailable");
        assert_eq!(
            supervisor.pending_notice().as_deref(),
            Some("paste_unavailable")
        );
        supervisor.consume_notice("unrelated");
        assert!(supervisor.pending_notice().is_some());
        supervisor.consume_notice("paste_unavailable");
        assert!(supervisor.pending_notice().is_none());
    }
}
