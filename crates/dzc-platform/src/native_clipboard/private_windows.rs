//! A private Windows clipboard selection retains its original lease and native owner.
use super::{selection_lease::{action, Action, Selection}, Payload};
use dzc_core::api::NativeReadLease;
use std::sync::{atomic::{AtomicBool, Ordering}, mpsc, Arc, Condvar, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};
use windows::core::w;
use windows::Win32::Foundation::{GetLastError, GlobalFree, SetLastError, HANDLE, HWND, WIN32_ERROR};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, GetClipboardOwner, GetClipboardSequenceNumber, OpenClipboard, RegisterClipboardFormatW, SetClipboardData};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, DispatchMessageW, PeekMessageW, TranslateMessage, HWND_MESSAGE, MSG, PM_REMOVE, WINDOW_EX_STYLE, WINDOW_STYLE};

const LIFETIME: Duration = Duration::from_secs(300);
const MAX_BYTES: usize = 64 * 1024 * 1024;
static ACTIVE: OnceLock<Mutex<Vec<Weak<State>>>> = OnceLock::new();
static RETAINED: OnceLock<Mutex<Vec<Owner>>> = OnceLock::new();
static CHILDREN: OnceLock<Mutex<Vec<ChildOwner>>> = OnceLock::new();
struct ChildOwner {
    process: std::process::Child,
    control: std::process::ChildStdin,
}
struct State {
    revoked: AtomicBool,
    finished: Mutex<bool>,
    changed: Condvar,
}
fn unfinished(owner: &Weak<State>) -> bool {
    owner.upgrade().is_some_and(|state| !*state.finished.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
}
pub(super) struct Owner {
    state: Arc<State>,
}
impl Owner {
    pub(super) fn start(payload: Payload, leases: Vec<NativeReadLease>, exports: Option<crate::private_files::BoundExport>) -> Result<Self, String> {
        Self::start_with_lifetime(payload, leases, exports, LIFETIME)
    }
    fn start_with_lifetime(payload: Payload, leases: Vec<NativeReadLease>, exports: Option<crate::private_files::BoundExport>, lifetime: Duration) -> Result<Self, String> {
        if leases.is_empty() || leases.iter().any(|lease| !lease.is_current()) {
            return Err("NativeLeaseOwnerRequired".into());
        }
        let representations = representations(payload)?;
        let state = Arc::new(State { revoked: AtomicBool::new(false), finished: Mutex::new(false), changed: Condvar::new() });
        let worker_state = state.clone();
        let (ready_tx, ready) = mpsc::sync_channel(1);
        {
            let mut active = ACTIVE.get_or_init(Default::default).lock().map_err(|_| "PrivateClipboardOwnerUnavailable")?;
            active.retain(unfinished);
            if active.len() >= 64 { return Err("PrivateClipboardOwnerUnavailable".into()); }
            active.push(Arc::downgrade(&state));
        }
        std::thread::Builder::new().name("private-windows-clipboard".into()).spawn(move || {
            let result = run(representations, leases, exports, lifetime, &worker_state, &ready_tx);
            if let Err(error) = result { let _ = ready_tx.try_send(Err(error)); }
            *worker_state.finished.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = true;
            worker_state.changed.notify_all();
        }).map_err(|_| "PrivateClipboardOwnerUnavailable")?;
        let owner = Self { state };
        match ready.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(())) => Ok(owner),
            _ => { let _ = owner.stop(); Err("PrivateClipboardOwnerUnavailable".into()) }
        }
    }
    pub(super) fn stop(&self) -> Result<(), String> { stop(&self.state) }
    pub(super) fn is_stopped(&self) -> bool {
        *self.state.finished.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}
impl Drop for Owner {
    fn drop(&mut self) { self.state.revoked.store(true, Ordering::Release); }
}
pub(super) fn retain(owner: Owner) {
    let mut retained = RETAINED.get_or_init(Default::default).lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    retained.retain(|owner| !owner.is_stopped());
    retained.push(owner);
}
fn stop(state: &State) -> Result<(), String> {
    state.revoked.store(true, Ordering::Release);
    let finished = state.finished.lock().map_err(|_| "PrivateClipboardStopNotConfirmed")?;
    let (finished, _) = state.changed.wait_timeout_while(finished, Duration::from_secs(2), |finished| !*finished).map_err(|_| "PrivateClipboardStopNotConfirmed")?;
    if *finished { Ok(()) } else { Err("PrivateClipboardStopNotConfirmed".into()) }
}
pub(super) fn revoke_all() -> Result<(), String> {
    let owners = {
        let mut registry = ACTIVE.get_or_init(Default::default).lock().map_err(|_| "PrivateClipboardStopNotConfirmed")?;
        registry.retain(unfinished);
        registry.iter().filter_map(Weak::upgrade).collect::<Vec<_>>()
    };
    let mut failed = false;
    for owner in owners { failed |= stop(&owner).is_err(); }
    let mut children = CHILDREN.get_or_init(Default::default).lock().map_err(|_| "PrivateClipboardStopNotConfirmed")?;
    children.retain_mut(|child| { let stopped = child.stop(); failed |= !stopped; !stopped });
    if failed { Err("PrivateClipboardStopNotConfirmed".into()) } else { Ok(()) }
}

impl ChildOwner {
    fn stop(&mut self) -> bool {
        use std::io::Write;
        if matches!(self.process.try_wait(), Ok(Some(_))) { return true; }
        let _ = self.control.write_all(b"stop\n").and_then(|()| self.control.flush());
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if matches!(self.process.try_wait(), Ok(Some(_))) { return true; }
            if Instant::now() >= deadline { return false; }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
pub(super) fn write_standalone(payload: Payload) -> Result<(), String> {
    start_child(&std::env::current_exe().map_err(|_| "PrivateClipboardOwnerUnavailable")?, payload)
}
fn start_child(executable: &std::path::Path, payload: Payload) -> Result<(), String> {
    use std::io::{BufRead, Write};
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    let data = serde_json::to_vec(&payload).map_err(|_| "PrivateClipboardPayloadRejected")?;
    if data.len() >= 256 * 1024 * 1024 { return Err("PrivateClipboardPayloadRejected".into()); }
    let mut children = CHILDREN.get_or_init(Default::default).lock().map_err(|_| "PrivateClipboardOwnerUnavailable")?;
    children.retain_mut(|child| !matches!(child.process.try_wait(), Ok(Some(_))));
    if children.len() >= 64 { return Err("PrivateClipboardOwnerUnavailable".into()); }
    let mut process = Command::new(executable).arg("clipboard-owner").creation_flags(0x08000000)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|_| "PrivateClipboardOwnerUnavailable")?;
    let mut control = process.stdin.take().ok_or("PrivateClipboardOwnerUnavailable")?;
    let stdout = process.stdout.take().ok_or("PrivateClipboardOwnerUnavailable")?;
    let sent = control.write_all(&data).and_then(|()| control.write_all(b"\n")).and_then(|()| control.flush());
    let (tx, ready) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut line = String::new();
        let result = std::io::BufReader::new(stdout).read_line(&mut line).map(|_| line);
        let _ = tx.send(result);
    });
    let mut child = ChildOwner { process, control };
    let accepted = sent.is_ok() && matches!(ready.recv_timeout(Duration::from_secs(5)), Ok(Ok(line)) if line.trim() == "ready");
    if !accepted && child.stop() { return Err("PrivateClipboardOwnerUnavailable".into()); }
    // A helper with unconfirmed stop retains its lease and must never be killed:
    // Windows would retain already materialized data after process termination.
    children.push(child);
    if accepted { Ok(()) } else { Err("PrivateClipboardStopNotConfirmed".into()) }
}
pub(super) fn serve_owner() -> Result<(), String> {
    use std::io::{BufRead, Read, Write};
    let mut input = std::io::BufReader::new(std::io::stdin());
    let mut data = Vec::new();
    Read::take(input.by_ref(), 256 * 1024 * 1024).read_until(b'\n', &mut data).map_err(|_| "PrivateClipboardPayloadRejected")?;
    if data.last() != Some(&b'\n') { return Err("PrivateClipboardPayloadRejected".into()); }
    let payload = serde_json::from_slice(&data).map_err(|_| "PrivateClipboardPayloadRejected")?;
    let prepared = super::prepare_owner_payload(payload)?;
    if !prepared.private { return Err("NativeLeaseOwnerRequired".into()); }
    let owner = match Owner::start(prepared.payload, prepared.leases, prepared.exports) {
        Ok(owner) => owner,
        Err(error) => {
            // A timed out worker may already have published data. This helper
            // must remain alive until its native cleanup has actually finished.
            while revoke_all().is_err() { std::thread::sleep(Duration::from_millis(20)); }
            return Err(error);
        }
    };
    println!("ready");
    let acknowledged = std::io::stdout().flush().is_ok();
    if !acknowledged { owner.state.revoked.store(true, Ordering::Release); }
    let state = owner.state.clone();
    std::thread::spawn(move || {
        let mut command = String::new();
        if input.read_line(&mut command).is_ok() && command == "stop\n" { state.revoked.store(true, Ordering::Release); }
        // A normal standalone caller exits immediately. EOF leaves this helper
        // alive until its original Core epoch or the fixed lifetime ends.
    });
    while !owner.is_stopped() { std::thread::sleep(Duration::from_millis(20)); }
    if acknowledged { Ok(()) } else { Err("PrivateClipboardOwnerUnavailable".into()) }
}

struct Window(HWND);
impl Window {
    fn new() -> Result<Self, String> {
        // All window and clipboard handles remain on this worker's native thread.
        unsafe { CreateWindowExW(WINDOW_EX_STYLE::default(), w!("STATIC"), w!("DezirClip private clipboard"), WINDOW_STYLE::default(), 0, 0, 0, 0, Some(HWND_MESSAGE), None, None, None) }
            .map(Self).map_err(|_| "PrivateClipboardOwnerUnavailable".into())
    }
}
impl Drop for Window {
    fn drop(&mut self) { unsafe { let _ = DestroyWindow(self.0); } }
}
struct ClipboardLock;
impl ClipboardLock {
    fn acquire(window: HWND) -> Result<Self, String> {
        for attempt in 0..6 {
            if unsafe { OpenClipboard(Some(window)) }.is_ok() { return Ok(Self); }
            if attempt < 5 { std::thread::sleep(Duration::from_millis(20)); }
        }
        Err("PrivateClipboardOccupied".into())
    }
}
impl Drop for ClipboardLock {
    fn drop(&mut self) { unsafe { let _ = CloseClipboard(); } }
}
fn current_selection() -> Option<Selection> {
    // A failed identity query cannot confirm that an old selection was replaced.
    unsafe { SetLastError(WIN32_ERROR(0)); }
    let owner = match unsafe { GetClipboardOwner() } {
        Ok(owner) => owner.0 as usize,
        Err(_) if unsafe { GetLastError() }.0 == 0 => 0,
        Err(_) => return None,
    };
    let sequence = unsafe { GetClipboardSequenceNumber() };
    (sequence != 0).then_some(Selection { owner, sequence })
}
fn pump() {
    let mut message = MSG::default();
    unsafe {
        while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}
fn registered(name: &str) -> Result<u32, String> {
    let name = name.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let format = unsafe { RegisterClipboardFormatW(windows::core::PCWSTR(name.as_ptr())) };
    if format == 0 { Err("PrivateClipboardFormatUnavailable".into()) } else { Ok(format) }
}
fn set_bytes(format: u32, bytes: &[u8]) -> Result<(), String> {
    if bytes.is_empty() || bytes.len() > MAX_BYTES { return Err("PrivateClipboardPayloadRejected".into()); }
    // HGLOBAL transfers to Windows only after SetClipboardData succeeds.
    unsafe {
        let memory = GlobalAlloc(GMEM_MOVEABLE, bytes.len()).map_err(|_| "PrivateClipboardAllocationFailed")?;
        let target = GlobalLock(memory);
        if target.is_null() { let _ = GlobalFree(Some(memory)); return Err("PrivateClipboardAllocationFailed".into()); }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), target.cast::<u8>(), bytes.len());
        let _ = GlobalUnlock(memory);
        if SetClipboardData(format, Some(HANDLE(memory.0))).is_err() {
            let _ = GlobalFree(Some(memory));
            return Err("PrivateClipboardWriteFailed".into());
        }
    }
    Ok(())
}
fn run(representations: Vec<(u32, Vec<u8>)>, leases: Vec<NativeReadLease>, mut exports: Option<crate::private_files::BoundExport>, lifetime: Duration, state: &State, ready: &mpsc::SyncSender<Result<(), String>>) -> Result<(), String> {
    let window = Window::new()?;
    let (original, accepted) = {
        let _clipboard = ClipboardLock::acquire(window.0)?;
        if state.revoked.load(Ordering::Acquire) || leases.iter().any(|lease| !lease.is_current()) { return Err("PrivacyEpochChanged".into()); }
        unsafe { EmptyClipboard() }.map_err(|_| "PrivateClipboardWriteFailed")?;
        let mut result = Ok(());
        for (format, bytes) in representations {
            if let Err(error) = set_bytes(format, &bytes) { result = Err(error); break; }
        }
        let identity = current_selection();
        let accepted = result.is_ok() && identity.is_some_and(|original| original.owner == window.0.0 as usize) && leases.iter().all(NativeReadLease::is_current);
        if !accepted {
            state.revoked.store(true, Ordering::Release);
            // The initial lock still excludes every later writer. Partial publication
            // is cleared before relinquishing this lock, without claiming a new owner.
            while unsafe { EmptyClipboard() }.is_err() { pump(); std::thread::sleep(Duration::from_millis(20)); }
        }
        (identity.unwrap_or(Selection { owner: 0, sequence: 0 }), accepted)
    };
    if accepted { let _ = ready.send(Ok(())); } else { let _ = ready.send(Err("PrivateClipboardWriteFailed".into())); }
    let started = Instant::now();
    loop {
        pump();
        let revoked = state.revoked.load(Ordering::Acquire) || leases.iter().any(|lease| !lease.is_current());
        if !revoked && action(original, current_selection(), false, started.elapsed(), lifetime) == Action::Keep {
            std::thread::sleep(Duration::from_millis(20));
            continue;
        }
        let decision = match ClipboardLock::acquire(window.0) {
            Ok(_clipboard) => match action(original, current_selection(), revoked, started.elapsed(), lifetime) {
                Action::ClearOriginal => if unsafe { EmptyClipboard() }.is_ok() { Action::ReleaseWithoutClear } else { Action::Retry },
                other => other,
            },
            Err(_) => Action::Retry,
        };
        if decision == Action::ReleaseWithoutClear { break; }
        std::thread::sleep(Duration::from_millis(20));
    }
    if let Some(export) = exports.as_mut() {
        while export.close().is_err() { pump(); std::thread::sleep(Duration::from_millis(20)); }
    }
    Ok(())
}

fn unicode(value: &str) -> Result<Vec<u8>, String> {
    if value.contains('\0') { return Err("PrivateClipboardPayloadRejected".into()); }
    Ok(value.encode_utf16().chain(Some(0)).flat_map(u16::to_le_bytes).collect())
}
fn html_bytes(fragment: &str) -> Vec<u8> {
    let prefix = "<html><body><!--StartFragment-->";
    let suffix = "<!--EndFragment--></body></html>";
    let header = |start, end, fragment_start, fragment_end| format!("Version:1.0\r\nStartHTML:{start:010}\r\nEndHTML:{end:010}\r\nStartFragment:{fragment_start:010}\r\nEndFragment:{fragment_end:010}\r\n");
    let start = header(0, 0, 0, 0).len();
    format!("{}{}{}{}\0", header(start, start + prefix.len() + fragment.len() + suffix.len(), start + prefix.len(), start + prefix.len() + fragment.len()), prefix, fragment, suffix).into_bytes()
}
fn image_bytes(bytes: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    if bytes.len() > MAX_BYTES { return Err("PrivateClipboardPayloadRejected".into()); }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().map_err(|_| "PrivateClipboardImageRejected")?;
    let mut limits = image::Limits::default(); limits.max_alloc = Some(MAX_BYTES as u64);
    reader.limits(limits);
    let image = reader.decode().map_err(|_| "PrivateClipboardImageRejected")?.into_rgba8();
    let (width, height) = image.dimensions();
    if width > i32::MAX as u32 || height > i32::MAX as u32 || image.len() > MAX_BYTES - 124 { return Err("PrivateClipboardImageRejected".into()); }
    let mut png = std::io::Cursor::new(Vec::new());
    image.write_to(&mut png, image::ImageFormat::Png).map_err(|_| "PrivateClipboardImageRejected")?;
    let mut dib = vec![0u8; 124];
    for (offset, value) in [(0, 124u32), (4, width), (8, (-(height as i32)) as u32), (16, 3), (20, image.len() as u32), (40, 0x00ff0000), (44, 0x0000ff00), (48, 0x000000ff), (52, 0xff000000), (56, 0x73524742), (108, 4)] { dib[offset..offset + 4].copy_from_slice(&value.to_le_bytes()); }
    dib[12..14].copy_from_slice(&1u16.to_le_bytes()); dib[14..16].copy_from_slice(&32u16.to_le_bytes());
    for pixel in image.pixels() { dib.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]); }
    Ok((dib, png.into_inner()))
}
fn representations(payload: Payload) -> Result<Vec<(u32, Vec<u8>)>, String> {
    let Payload::Private(payload) = payload else { return Err("NativeLeaseOwnerRequired".into()); };
    let mut result = vec![(registered("ExcludeClipboardContentFromMonitorProcessing")?, vec![0]), (registered("CanIncludeInClipboardHistory")?, 0u32.to_le_bytes().to_vec()), (registered("CanUploadToCloudClipboard")?, 0u32.to_le_bytes().to_vec())];
    match *payload {
        Payload::Text(text) => result.push((13, unicode(&text)?)),
        Payload::Html { html, text } => { result.push((13, unicode(&text)?)); result.push((registered("HTML Format")?, html_bytes(&html))); }
        Payload::Image(bytes) => { let (dib, png) = image_bytes(&bytes)?; result.push((17, dib)); result.push((registered("PNG")?, png)); }
        Payload::Files(paths) => {
            if paths.is_empty() || paths.len() > 64 { return Err("PrivateClipboardPayloadRejected".into()); }
            let mut bytes = vec![0u8; 20]; bytes[..4].copy_from_slice(&20u32.to_le_bytes()); bytes[16..20].copy_from_slice(&1u32.to_le_bytes());
            for path in paths { bytes.extend(unicode(&path)?); } bytes.extend([0, 0]); result.push((15, bytes));
        }
        _ => return Err("PrivateClipboardPayloadRejected".into()),
    }
    if result.iter().any(|(_, bytes)| bytes.len() > MAX_BYTES) { return Err("PrivateClipboardPayloadRejected".into()); }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_finished_handles_do_not_consume_active_worker_capacity() {
        let finished = Arc::new(State { revoked: AtomicBool::new(true), finished: Mutex::new(true), changed: Condvar::new() });
        let live = Arc::new(State { revoked: AtomicBool::new(false), finished: Mutex::new(false), changed: Condvar::new() });
        let mut registry = vec![Arc::downgrade(&finished), Arc::downgrade(&live), Weak::new()];
        registry.retain(unfinished);
        assert_eq!(registry.len(), 1);
        assert!(Arc::ptr_eq(&registry[0].upgrade().unwrap(), &live));
        assert_eq!(Arc::strong_count(&finished), 1);
    }

    #[test]
    fn html_offsets_are_utf8_byte_offsets() {
        let fragment = "<b>中文🙂</b>";
        let bytes = html_bytes(fragment);
        let text = std::str::from_utf8(&bytes).unwrap();
        let offset = |name: &str| text.lines().find_map(|line| line.strip_prefix(name)).unwrap().parse::<usize>().unwrap();
        let start = offset("StartHTML:");
        let end = offset("EndHTML:");
        assert!(text[start..end].starts_with("<html>"));
        assert!(text[start..end].ends_with("</html>"));
        assert_eq!(&bytes[offset("StartFragment:")..offset("EndFragment:")], fragment.as_bytes());
        assert_eq!(&bytes[end..], &[0]);
    }
    #[test]
    fn private_image_retains_every_rgba_pixel_in_png_and_dib() {
        let pixels = vec![1, 2, 3, 255, 200, 201, 202, 128];
        let source = image::RgbaImage::from_raw(2, 1, pixels.clone()).unwrap();
        let mut input = std::io::Cursor::new(Vec::new());
        source.write_to(&mut input, image::ImageFormat::Png).unwrap();
        let (dib, png) = image_bytes(input.get_ref()).unwrap();
        assert_eq!(image::load_from_memory(&png).unwrap().into_rgba8().into_raw(), pixels);
        assert_eq!(&dib[124..], &[3, 2, 1, 255, 202, 201, 200, 128]);
        assert_eq!(i32::from_le_bytes(dib[8..12].try_into().unwrap()), -1);
        assert!(image_bytes(b"not an image").is_err());
    }
    #[test]
    fn unicode_rejects_embedded_nul_before_native_write() {
        assert!(unicode("one\0two").is_err());
        let bytes = unicode("中文🙂").unwrap();
        let units = bytes.chunks_exact(2).map(|pair| u16::from_le_bytes(pair.try_into().unwrap())).collect::<Vec<_>>();
        assert_eq!(units.last(), Some(&0));
        assert_eq!(String::from_utf16(&units[..units.len() - 1]).unwrap(), "中文🙂");
    }

    // Native access is deliberately opt-in and restricted to an ephemeral CI runner.
    #[test]
    #[ignore = "requires the disposable Windows GitHub runner and explicit fixture opt-in"]
    fn disposable_native_lifecycle() {
        use dzc_core::api::{ClipboardEntry, HistoryService, PrivacySessionCommand, SensitiveAccess};
        use dzc_core::ports::{ClipboardPayload, ClipboardWriter};
        use windows::Win32::Foundation::HGLOBAL;
        use windows::Win32::System::DataExchange::{CountClipboardFormats, GetClipboardData};
        use windows::Win32::System::Memory::GlobalSize;

        assert_eq!(std::env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
        assert_eq!(std::env::var("RUNNER_ENVIRONMENT").as_deref(), Ok("github-hosted"));
        assert_eq!(std::env::var("DZC_PRIVATE_LEASE_DISPOSABLE").as_deref(), Ok("1"));
        let runner = std::fs::canonicalize(std::env::var_os("RUNNER_TEMP").expect("runner-owned temporary directory")).unwrap();
        let root = runner.join(format!("dzc-private-lease-fixture-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        assert_eq!(root.parent(), Some(runner.as_path()));
        std::env::set_var("TEMP", &root);
        std::env::set_var("TMP", &root);
        let executable = std::fs::canonicalize(std::env::var_os("DZC_PRIVATE_OWNER_FIXTURE_EXE").expect("CI-built standalone CLI")).unwrap();
        assert_eq!(executable.file_name().unwrap(), "dzc.exe");
        let history = HistoryService::open(&root.join("synthetic.db")).unwrap();
        let now = || std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
        let authorize = || {
            let epoch = history.privacy().status(now()).unwrap().privacy_epoch;
            let permit = history.privacy().authorize_selected_copy(-42, Some("00000000-0000-4000-8000-000000000042"), SensitiveAccess::ExplicitUserAction, Some(epoch), now()).unwrap();
            let entry: ClipboardEntry = serde_json::from_value(serde_json::json!({"id":-42,"content_type":"text","content":"synthetic private text","preview":"private","source_app":"Synthetic","timestamp":1,"is_pinned":false,"tags":["sensitive"]})).unwrap();
            match history.session_clipboard_payload_for_host_authorized(&entry, "00000000-0000-4000-8000-000000000042", &permit, now()).unwrap() {
                ClipboardPayload::Private { authorization, .. } => authorization,
                _ => panic!("original Core private authorization required"),
            }
        };
        let wait = |condition: &dyn Fn() -> bool| {
            let deadline = Instant::now() + Duration::from_secs(8);
            while !condition() { assert!(Instant::now() < deadline, "native lease did not settle"); pump(); std::thread::sleep(Duration::from_millis(10)); }
        };
        let reader = Window::new().unwrap();
        let current_selection = || super::current_selection().expect("native selection identity available");
        let read_raw = |format| {
            let _lock = ClipboardLock::acquire(reader.0).unwrap();
            let value = unsafe { GetClipboardData(format) }.unwrap();
            let memory = HGLOBAL(value.0);
            let size = unsafe { GlobalSize(memory) };
            assert!(size > 0 && size <= MAX_BYTES);
            let bytes = unsafe { GlobalLock(memory) };
            assert!(!bytes.is_null());
            let result = unsafe { std::slice::from_raw_parts(bytes.cast::<u8>(), size) }.to_vec();
            unsafe { let _ = GlobalUnlock(memory); }
            result
        };
        let empty = || { let _lock = ClipboardLock::acquire(reader.0).unwrap(); assert_eq!(unsafe { CountClipboardFormats() }, 0); };
        let writer = crate::runtime::EmbeddedClipboardWriter::default();
        let pixels = vec![7, 8, 9, 255, 1, 2, 3, 128];
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_raw(2, 1, pixels.clone()).unwrap().write_to(&mut png, image::ImageFormat::Png).unwrap();
        for (payload, format, expected) in [
            (ClipboardPayload::Text("synthetic private 中文🙂".into()), 13, unicode("synthetic private 中文🙂").unwrap()),
            (ClipboardPayload::Html { html: "<b>synthetic 中文🙂</b>".into(), text: "synthetic 中文🙂".into() }, registered("HTML Format").unwrap(), html_bytes("<b>synthetic 中文🙂</b>")),
            (ClipboardPayload::Image { bytes: png.get_ref().clone(), mime: "image/png".into() }, registered("PNG").unwrap(), png.get_ref().clone()),
        ] {
            writer.write(ClipboardPayload::Private { payload: Box::new(payload), authorization: authorize() }).unwrap();
            let raw = read_raw(format);
            assert_eq!(&raw[..expected.len()], expected.as_slice());
            assert!(raw[expected.len()..].iter().all(|byte| *byte == 0));
            assert_eq!(&read_raw(registered("CanIncludeInClipboardHistory").unwrap())[..4], &[0; 4]);
            assert_eq!(&read_raw(registered("CanUploadToCloudClipboard").unwrap())[..4], &[0; 4]);
            assert!(current_selection().owner != 0);
            history.privacy().set_session(PrivacySessionCommand::On { duration_ms: None }, now()).unwrap();
            writer.release_private_selection().unwrap();
            empty();
            history.privacy().set_session(PrivacySessionCommand::Off, now()).unwrap();
        }
        let replacement = Window::new().unwrap();
        let publish_replacement = || {
            let _lock = ClipboardLock::acquire(replacement.0).unwrap();
            unsafe { EmptyClipboard() }.unwrap();
            set_bytes(13, &unicode("later ordinary owner").unwrap()).unwrap();
            current_selection()
        };
        writer.write(ClipboardPayload::Private { payload: Box::new(ClipboardPayload::Text("old private".into())), authorization: authorize() }).unwrap();
        let later = publish_replacement();
        history.privacy().set_session(PrivacySessionCommand::On { duration_ms: None }, now()).unwrap();
        writer.release_private_selection().unwrap();
        assert_eq!(current_selection(), later);
        assert_eq!(&read_raw(13)[..unicode("later ordinary owner").unwrap().len()], unicode("later ordinary owner").unwrap().as_slice());
        history.privacy().set_session(PrivacySessionCommand::Off, now()).unwrap();

        let same_owner = Owner::start(Payload::Private(Box::new(Payload::Text("earlier sequence".into()))), vec![authorize().native_lease().unwrap()], None).unwrap();
        let original = current_selection();
        {
            let _lock = ClipboardLock::acquire(HWND(original.owner as *mut std::ffi::c_void)).unwrap();
            unsafe { EmptyClipboard() }.unwrap();
            set_bytes(13, &unicode("later sequence same owner").unwrap()).unwrap();
            let later = current_selection();
            assert_eq!(later.owner, original.owner);
            assert_ne!(later.sequence, original.sequence);
        }
        same_owner.stop().unwrap();
        assert_eq!(&read_raw(13)[..unicode("later sequence same owner").unwrap().len()], unicode("later sequence same owner").unwrap().as_slice());

        // The original worker independently observes epoch changes, without a GUI stop call.
        let epoch_owner = Owner::start(Payload::Private(Box::new(Payload::Text("epoch bound".into()))), vec![authorize().native_lease().unwrap()], None).unwrap();
        history.privacy().set_session(PrivacySessionCommand::On { duration_ms: None }, now()).unwrap();
        wait(&|| epoch_owner.is_stopped());
        empty();
        history.privacy().set_session(PrivacySessionCommand::Off, now()).unwrap();
        let expiry_owner = Owner::start_with_lifetime(Payload::Private(Box::new(Payload::Text("deadline bound".into()))), vec![authorize().native_lease().unwrap()], None, Duration::from_millis(500)).unwrap();
        wait(&|| expiry_owner.is_stopped());
        empty();

        let files = super::super::prepare_owner_payload(Payload::LeasedFiles { files: vec![super::super::NativeFile { name: "synthetic.txt".into(), bytes: b"leased file".to_vec() }], lease: authorize().native_lease().unwrap() }).unwrap();
        let paths = files.exports.as_ref().unwrap().paths().unwrap();
        assert!(std::path::Path::new(&paths[0]).starts_with(&root));
        let file_owner = Owner::start(files.payload, files.leases, files.exports).unwrap();
        assert_eq!(std::fs::read(&paths[0]).unwrap(), b"leased file");
        assert_eq!(read_raw(15)[..4], 20u32.to_le_bytes());
        file_owner.stop().unwrap();
        assert!(std::fs::read(&paths[0]).is_err());
        empty();

        writer.write(ClipboardPayload::ProtectedFiles { files: vec![dzc_core::ports::ClipboardFile { name: "original-epoch.txt".into(), bytes: b"Core protected file".to_vec() }], authorization: authorize() }).unwrap();
        let drop_files = read_raw(15);
        let path_units = drop_files[20..].chunks_exact(2).map(|pair| u16::from_le_bytes(pair.try_into().unwrap())).take_while(|unit| *unit != 0).collect::<Vec<_>>();
        let original_path = String::from_utf16(&path_units).unwrap();
        assert!(std::path::Path::new(&original_path).starts_with(&root));
        assert_eq!(std::fs::read(&original_path).unwrap(), b"Core protected file");
        let replacement_selection = publish_replacement();
        history.privacy().set_session(PrivacySessionCommand::On { duration_ms: None }, now()).unwrap();
        writer.release_private_selection().unwrap();
        assert!(std::fs::read(&original_path).is_err());
        assert_eq!(current_selection(), replacement_selection);
        assert_eq!(&read_raw(13)[..unicode("later ordinary owner").unwrap().len()], unicode("later ordinary owner").unwrap().as_slice());
        history.privacy().set_session(PrivacySessionCommand::Off, now()).unwrap();

        let helper_payload = || Payload::Leased { payload: Box::new(Payload::Private(Box::new(Payload::Text("standalone original lease".into())))), lease: authorize().native_lease().unwrap() };
        start_child(&executable, helper_payload()).unwrap();
        assert_eq!(&read_raw(13)[..unicode("standalone original lease").unwrap().len()], unicode("standalone original lease").unwrap().as_slice());
        revoke_all().unwrap();
        empty();
        start_child(&executable, helper_payload()).unwrap();
        // Closing the caller's pipe models successful CLI exit. It must not stop
        // the helper or discard the original epoch before the later revocation.
        let children = std::mem::take(&mut *CHILDREN.get().unwrap().lock().unwrap());
        let mut detached = children.into_iter().map(|child| { drop(child.control); child.process }).collect::<Vec<_>>();
        assert_eq!(&read_raw(13)[..unicode("standalone original lease").unwrap().len()], unicode("standalone original lease").unwrap().as_slice());
        history.privacy().set_session(PrivacySessionCommand::On { duration_ms: None }, now()).unwrap();
        // Process termination is checked below without killing a live selection.
        for child in &mut detached {
            let deadline = Instant::now() + Duration::from_secs(8);
            loop { if let Some(status) = child.try_wait().unwrap() { assert!(status.success()); break; } assert!(Instant::now() < deadline); std::thread::sleep(Duration::from_millis(10)); }
        }
        empty();
        history.privacy().set_session(PrivacySessionCommand::Off, now()).unwrap();
        start_child(&executable, Payload::LeasedFiles { files: vec![super::super::NativeFile { name: "helper-owned.txt".into(), bytes: b"helper owns the only export handles".to_vec() }], lease: authorize().native_lease().unwrap() }).unwrap();
        let drop_files = read_raw(15);
        let path_units = drop_files[20..].chunks_exact(2).map(|pair| u16::from_le_bytes(pair.try_into().unwrap())).take_while(|unit| *unit != 0).collect::<Vec<_>>();
        let helper_path = String::from_utf16(&path_units).unwrap();
        assert!(std::path::Path::new(&helper_path).starts_with(&root));
        let children = std::mem::take(&mut *CHILDREN.get().unwrap().lock().unwrap());
        let mut detached_files = children.into_iter().map(|child| { drop(child.control); child.process }).collect::<Vec<_>>();
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(std::fs::read(&helper_path).unwrap(), b"helper owns the only export handles");
        for child in &mut detached_files { assert!(child.try_wait().unwrap().is_none()); }
        history.privacy().set_session(PrivacySessionCommand::On { duration_ms: None }, now()).unwrap();
        for child in &mut detached_files {
            let deadline = Instant::now() + Duration::from_secs(8);
            loop { if let Some(status) = child.try_wait().unwrap() { assert!(status.success()); break; } assert!(Instant::now() < deadline); std::thread::sleep(Duration::from_millis(10)); }
        }
        assert!(std::fs::read(&helper_path).is_err());
        empty();
        writer.stop().unwrap();
        println!("Windows private lease native cases passed: text/html/PNG pixels, original epoch, expiry, later owner preservation, file deletion, helper stop and caller exit");
    }
}
