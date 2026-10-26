//! Experimental Quick Paste is a managed read-only child, never a second Host.
mod identity;
mod state;
use super::ipc_host::{DesktopHost, SurfaceAccess};
use crate::infrastructure::repository::settings_repo::SettingsRepository;
use crate::infrastructure::surface_focus::{ManagedChild, Target};
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
type Reply = oneshot::Sender<SurfaceControlMessage>;

#[derive(Default)]
pub struct SurfaceSupervisor {
    serial: AsyncMutex<()>,
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
    [
        "SystemRoot",
        "WINDIR",
        "PATH",
        "TEMP",
        "TMP",
        "HOME",
        "USERPROFILE",
        "LOCALAPPDATA",
        "APPDATA",
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XDG_RUNTIME_DIR",
        "DBUS_SESSION_BUS_ADDRESS",
        "LANG",
        "LC_ALL",
        "FONTCONFIG_FILE",
        "FONTCONFIG_PATH",
    ]
    .into_iter()
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
    let main = app
        .get_webview_window("main")
        .ok_or("SurfaceGeometryUnavailable")?;
    let cursor = main.cursor_position().map_err(error)?;
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
        let (send, receive) = oneshot::channel();
        {
            let mut replies = self.replies.lock().map_err(error)?;
            if replies.len() >= 4 {
                return Err("SurfaceRequestsBusy".into());
            }
            replies.insert(request.request_id.clone(), send);
        }
        let result = tokio::time::timeout(TIMEOUT, async {
            self.send(&request).await?;
            let response = receive.await.map_err(error)?;
            if response.r#type != expected {
                return Err("SurfaceAcknowledgementRejected".into());
            }
            Ok(response)
        })
        .await
        .map_err(|_| "SurfaceAcknowledgementTimeout".to_owned())
        .and_then(|result| result);
        if let Ok(mut replies) = self.replies.lock() {
            replies.remove(&request.request_id);
        }
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
        let next = expected.checked_add(1)?;
        self.revision
            .compare_exchange(expected, next, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| next)
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
        self.revision
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1)
            })
            .map(|previous| previous + 1)
            .unwrap_or(u64::MAX)
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
    async fn start(&self, app: &AppHandle) -> Result<Arc<Managed>, String> {
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
        });
        *self.process.lock().map_err(error)? = Some(process.clone());
        if let Some(stdout) = stdout {
            tauri::async_runtime::spawn(read_stdout(app.clone(), Arc::downgrade(&process), stdout));
        }
        if let Some(stderr) = stderr {
            tauri::async_runtime::spawn(read_stderr(app.clone(), Arc::downgrade(&process), stderr));
        }
        if !initialized {
            let _ = self.stop_current("disabled", true).await;
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
    pub async fn show(&self, app: &AppHandle) -> Result<(), String> {
        let revision = self.invalidate();
        self.requested.fetch_max(revision, Ordering::AcqRel);
        // Capture before helper startup can create any native window.
        let target = Arc::new(Target::capture()?);
        let (rect, scale) = geometry(app)?;
        let _serial = self.serial.lock().await;
        let existing = self.current()?;
        if let Some(process) = existing.as_ref() {
            let exhausted = process.view.lock().map_err(error)?.exhausted();
            if process.created.elapsed() > Duration::from_secs(540)
                || process.cancelled.load(Ordering::Acquire)
                || exhausted
            {
                self.stop_current("disabled", true).await?;
            }
        }
        let existing = self.current()?;
        let process = if let Some(process) = existing {
            process
        } else {
            self.start(app).await?
        };
        self.valid(&process, revision)?;
        let state = process.access.state().await?;
        self.valid(&process, revision)?;
        if !target.context_current() {
            return Err("SurfaceFocusChanged".into());
        }
        let (locale, theme) = appearance(app);
        let notice = self.pending_notice();
        let display = id();
        *process.target.lock().map_err(error)? = Some(target);
        process.view.lock().map_err(error)?.show(state::Display {
            id: display.clone(),
            state: state.clone(),
            revision,
            shown: false,
        });
        if let Some(main) = app.get_webview_window("main") {
            if main.is_visible().unwrap_or(false) {
                crate::app::window_manager::hide_window_cmd(app.clone()).map_err(error)?;
            }
        }
        let response = process
            .exchange(
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
            )
            .await?;
        let shown: SurfaceShown = decode(response.payload)?;
        self.valid(&process, revision)?;
        if shown.surface_session_id != process.id
            || !process.view.lock().map_err(error)?.presented(
                &shown.display_request_id,
                revision,
                &shown.state,
            )
        {
            return Err("SurfaceStaleAcknowledgement".into());
        }
        process.access.check(&shown.state).await?;
        self.valid(&process, revision)?;
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
        let response = process
            .exchange(
                message(
                    SurfaceControlType::Hide,
                    SurfaceHide {
                        surface_session_id: process.id.clone(),
                        display_request_id: display.into(),
                        reason: reason.into(),
                    },
                )?,
                SurfaceControlType::Hidden,
            )
            .await?;
        let hidden: SurfaceHidden = decode(response.payload)?;
        if hidden.surface_session_id != process.id || hidden.display_request_id != display {
            return Err("SurfaceStaleAcknowledgement".into());
        }
        Ok(())
    }
    pub async fn hide(&self) -> Result<(), String> {
        let revision = self.invalidate();
        let _serial = self.serial.lock().await;
        let process = self.current()?;
        if let Some(process) = process {
            let display = process.view.lock().map_err(error)?.cancel();
            let target = process.target.lock().map_err(error)?.take();
            if let Some(display) = display {
                self.hide_display(&process, &display.id, "cancel").await?;
            }
            if let Some(target) = target {
                self.restore_target(target, revision).await?;
            }
        }
        Ok(())
    }
    async fn restore_target(&self, target: Arc<Target>, revision: u64) -> Result<(), String> {
        let generation = self.revision.clone();
        tauri::async_runtime::spawn_blocking(move || {
            target.restore_if(|| {
                revision != u64::MAX && generation.load(Ordering::Acquire) == revision
            })
        })
        .await
        .map_err(error)?
    }
    async fn dismissed(
        &self,
        process: Arc<Managed>,
        request: SurfaceDismiss,
        revision: u64,
    ) -> Result<(), String> {
        let _serial = self.serial.lock().await;
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
                self.restore_target(target, revision).await?;
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
        process: &Managed,
        request: &SurfacePaste,
    ) -> Result<(), String> {
        let _serial = self.serial.lock().await;
        let revision = self.current_revision();
        self.valid(&process, revision)?;
        let actual = process.access.state().await?;
        self.valid(&process, revision)?;
        let display =
            process
                .view
                .lock()
                .map_err(error)?
                .admit(&process.id, revision, &request, &actual)?;
        self.mark_hidden(revision);
        let target = process
            .target
            .lock()
            .map_err(error)?
            .clone()
            .ok_or("SurfaceTargetUnavailable")?;
        self.hide_display(process, &display.id, "paste").await?;
        self.valid(process, revision)?;
        process.access.check(&request.state).await?;
        self.restore_target(target.clone(), revision).await?;
        self.valid(&process, revision)?;
        process
            .access
            .copy(request.entry_id, &request.state)
            .await?;
        process.access.check(&request.state).await?;
        self.valid(&process, revision)?;
        // No queued worker can outlive the last cancellation/focus check.
        if !target.is_current() {
            return Err("SurfaceFocusChanged".into());
        }
        self.valid(&process, revision)?;
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
    ) -> Result<(), String> {
        let performed = self.perform_action(&process, &request).await;
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
            let _ = self.stop("disabled", true).await;
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
            Err(_) => {
                schedule_stop(&app, &process);
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
            if let Some(reply) = reply {
                let _ = reply.send(message);
            } else if message.r#type == SurfaceControlType::Failed {
                schedule_stop(&app, &process);
                return;
            }
            continue;
        }
        if process.cancelled.load(Ordering::Acquire) {
            continue;
        }
        match message.r#type {
            SurfaceControlType::Paste => {
                let request = decode::<SurfacePaste>(message.payload);
                let app = app.clone();
                if let Ok(request) = request {
                    if !process.event_pending.swap(true, Ordering::AcqRel) {
                        tauri::async_runtime::spawn(async move {
                            let manager = app.state::<SurfaceSupervisor>();
                            let outcome = manager
                                .action(process.clone(), message.request_id, request)
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
                                        .dismissed(process.clone(), request, revision)
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
                schedule_stop(&app, &process);
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
    loop {
        match stream.read(&mut bytes).await {
            Ok(0) | Err(_) => return,
            Ok(count) => {
                total += count;
                if total > 64 * 1024 {
                    if let Some(process) = process.upgrade() {
                        schedule_stop(&app, &process);
                    }
                    return;
                }
            }
        }
    }
}
fn schedule_stop(app: &AppHandle, process: &Arc<Managed>) {
    let unexpected = !process.cancelled.load(Ordering::Acquire);
    process.cancel();
    if let Some(manager) = app.try_state::<SurfaceSupervisor>() {
        if manager
            .current()
            .ok()
            .flatten()
            .is_some_and(|current| Arc::ptr_eq(&current, process))
        {
            manager.invalidate();
            if unexpected {
                manager.record_failure("renderer_unavailable");
            }
        }
    }
    let app = app.clone();
    let process = process.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(manager) = app.try_state::<SurfaceSupervisor>() {
            let _serial = manager.serial.lock().await;
            let current = manager
                .current()
                .ok()
                .flatten()
                .is_some_and(|current| Arc::ptr_eq(&current, &process));
            if current {
                let _ = manager.stop_current("disabled", true).await;
            }
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
