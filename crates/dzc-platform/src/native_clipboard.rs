//! Native clipboard access shared by the standalone CLI and its X11 owner process.
use serde::{Deserialize, Serialize};
#[cfg(any(test, target_os = "windows"))]
mod selection_lease;
#[cfg(target_os = "windows")]
mod private_windows;
#[cfg(target_os = "linux")]
mod private_x11_files;
#[cfg(target_os = "linux")]
pub(crate) mod selection_export;
#[cfg(target_os = "linux")]
use std::io::{Read, Write};

#[derive(Serialize, Deserialize)]
pub enum Payload {
    Text(String),
    Html {
        html: String,
        text: String,
    },
    Image(Vec<u8>),
    Files(Vec<String>),
    Private(Box<Payload>),
    Leased {
        payload: Box<Payload>,
        lease: dzc_core::api::NativeReadLease,
    },
    LeasedFiles {
        files: Vec<NativeFile>,
        lease: dzc_core::api::NativeReadLease,
    },
}

#[derive(Serialize, Deserialize)]
pub struct NativeFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// Explicit long-lived host ownership. No connection is opened until write.
#[derive(Default)]
pub struct EmbeddedOwner {
    clipboard: Option<arboard::Clipboard>,
    #[cfg(target_os = "linux")]
    wayland: Option<crate::wayland::WaylandOwner>,
    #[cfg(target_os = "linux")]
    x11_files: Option<private_x11_files::FileOwner>,
    #[cfg(target_os = "windows")]
    windows_private: Option<private_windows::Owner>,
    exports: Option<crate::private_files::BoundExport>,
    private: bool,
    stopped: bool,
}
type SharedOwner = std::sync::Arc<std::sync::Mutex<EmbeddedOwner>>;
static EMBEDDED_OWNERS: std::sync::OnceLock<
    std::sync::Mutex<Vec<std::sync::Weak<std::sync::Mutex<EmbeddedOwner>>>>,
> = std::sync::OnceLock::new();
impl EmbeddedOwner {
    pub(crate) fn shared() -> SharedOwner {
        let owner = std::sync::Arc::new(std::sync::Mutex::new(Self::default()));
        EMBEDDED_OWNERS
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(std::sync::Arc::downgrade(&owner));
        owner
    }
    pub fn write(&mut self, payload: Payload) -> Result<(), String> {
        self.write_with_intent(payload, false)
    }
    pub fn write_with_intent(
        &mut self,
        payload: Payload,
        suppress_recapture: bool,
    ) -> Result<(), String> {
        #[cfg(target_os = "windows")]
        let _ = suppress_recapture;
        if self.stopped {
            return Err("ClipboardOwnerStopped".into());
        }
        #[cfg(target_os = "linux")]
        if crate::wayland::requested() {
            return crate::wayland::replace_owner(&mut self.wayland, payload, suppress_recapture)
                .map_err(|error| error.to_string());
        }
        let prepared = prepare_owner_payload(payload)?;
        #[cfg(target_os = "windows")]
        {
            self.release_selection()?;
            if prepared.private {
                self.windows_private = Some(private_windows::Owner::start(prepared.payload, prepared.leases, prepared.exports)?);
                self.private = true;
                return Ok(());
            }
        }
        #[cfg(target_os = "linux")]
        if matches!(&prepared.payload, Payload::Private(inner) if matches!(inner.as_ref(),Payload::Files(_)))
        {
            let Payload::Private(inner) = prepared.payload else {
                unreachable!()
            };
            let Payload::Files(paths) = *inner else {
                unreachable!()
            };
            let mut fresh =
                private_x11_files::FileOwner::start(paths, prepared.leases, prepared.exports)?;
            if let Err(error) = self.release_selection() {
                let _ = fresh.stop();
                if !fresh.is_stopped() {
                    private_x11_files::retain(fresh);
                }
                return Err(error);
            }
            self.x11_files = Some(fresh);
            self.private = true;
            return Ok(());
        }
        if self.clipboard.is_none() {
            self.clipboard = Some(open()?);
        }
        set(
            self.clipboard.as_mut().expect("clipboard opened above"),
            prepared.payload,
        )?;
        #[cfg(target_os = "linux")]
        if let Some(owner) = self.x11_files.as_mut() {
            let result = owner.stop();
            if !owner.is_stopped() {
                return Err(result.err().unwrap_or("PrivateFileStopNotConfirmed".into()));
            }
            self.x11_files = None;
        }
        self.exports = prepared.exports;
        self.private = prepared.private;
        if prepared.leases.iter().any(|lease| !lease.is_current()) {
            self.release_selection()?;
            return Err("PrivacyEpochChanged".into());
        }
        Ok(())
    }
    fn release_selection(&mut self) -> Result<(), String> {
        #[cfg(target_os = "windows")]
        if let Some(owner) = self.windows_private.as_ref() {
            owner.stop()?;
            if !owner.is_stopped() { return Err("PrivateClipboardStopNotConfirmed".into()); }
            self.windows_private = None;
        }
        #[cfg(target_os = "linux")]
        if let Some(owner) = self.x11_files.as_mut() {
            let result = owner.stop();
            if !owner.is_stopped() {
                return Err(result.err().unwrap_or("PrivateFileStopNotConfirmed".into()));
            }
            self.x11_files = None;
        }
        #[cfg(target_os = "linux")]
        if let Some(owner) = self.wayland.as_mut() {
            let result = owner.stop();
            if !owner.is_stopped() {
                return Err(result
                    .err()
                    .unwrap_or(crate::wayland::WaylandError::TimedOut)
                    .to_string());
            }
            self.wayland = None;
        }
        self.clipboard = None;
        if let Some(export) = self.exports.as_mut() {
            export.close()?;
        }
        self.exports = None;
        self.private = false;
        Ok(())
    }
    pub(crate) fn revoke_private(&mut self) -> Result<(), String> {
        #[cfg(target_os = "linux")]
        let private = self.private
            || self
                .wayland
                .as_ref()
                .is_some_and(|owner| owner.is_private());
        #[cfg(target_os = "windows")]
        let private = self.private;
        if private {
            self.release_selection()?;
        }
        Ok(())
    }
    /// Stop confirmation is required for ownership handoff. A timeout retains the owner.
    pub fn stop(&mut self) -> Result<(), String> {
        self.stopped = true;
        self.release_selection()
    }
}
impl Drop for EmbeddedOwner {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
fn revoke_embedded_private_owners() -> Result<(), String> {
    if let Some(owners) = EMBEDDED_OWNERS.get() {
        let mut owners = owners
            .lock()
            .map_err(|_| "PrivateExportRevocationUnconfirmed")?;
        let mut failed = false;
        owners.retain(|weak| {
            let Some(owner) = weak.upgrade() else {
                return false;
            };
            if owner
                .lock()
                .map_err(|_| ())
                .and_then(|mut owner| owner.revoke_private().map_err(|_| ()))
                .is_err()
            {
                failed = true;
            }
            true
        });
        if failed {
            return Err("PrivateExportRevocationUnconfirmed".into());
        }
    }
    Ok(())
}

pub(crate) struct PreparedOwner {
    pub payload: Payload,
    pub leases: Vec<dzc_core::api::NativeReadLease>,
    pub exports: Option<crate::private_files::BoundExport>,
    pub private: bool,
}
/// Unwrap only native lifecycle permits. No history access or decryption occurs here.
pub(crate) fn prepare_owner_payload(mut payload: Payload) -> Result<PreparedOwner, String> {
    let mut leases = Vec::new();
    let mut private = false;
    let mut exports = None;
    let mut depth = 0;
    loop {
        depth += 1;
        if depth > 8 {
            return Err("OwnerPayloadRejected".into());
        }
        payload = match payload {
            Payload::Leased { payload, lease } => {
                if !lease.is_current() {
                    return Err("PrivacyEpochChanged".into());
                }
                leases.push(lease);
                *payload
            }
            Payload::Private(inner) => {
                private = true;
                *inner
            }
            Payload::LeasedFiles { files, lease } => {
                if !lease.is_current() {
                    return Err("PrivacyEpochChanged".into());
                }
                leases.push(lease.clone());
                private = true;
                let owned = crate::private_files::BoundExport::readonly(
                    files
                        .into_iter()
                        .map(|file| dzc_core::ports::ClipboardFile {
                            name: file.name,
                            bytes: file.bytes,
                        })
                        .collect(),
                    lease.authorization(),
                )?;
                let paths = owned.paths()?;
                exports = Some(owned);
                Payload::Files(paths)
            }
            ordinary => {
                if private && leases.is_empty() {
                    return Err("NativeLeaseOwnerRequired".into());
                }
                return Ok(PreparedOwner {
                    payload: if private {
                        Payload::Private(Box::new(ordinary))
                    } else {
                        ordinary
                    },
                    leases,
                    exports,
                    private,
                });
            }
        };
    }
}

fn open() -> Result<arboard::Clipboard, String> {
    #[cfg(target_os = "linux")]
    crate::runtime_display::validate_x11_display().map_err(|error| error.to_string())?;
    arboard::Clipboard::new().map_err(|e| format!("clipboard: {e}"))
}

pub fn read(mime: &str) -> Result<Vec<u8>, String> {
    #[cfg(target_os = "linux")]
    if crate::wayland::requested() {
        let selection =
            crate::wayland::WaylandClipboardBackend::snapshot().map_err(|e| e.to_string())?;
        let actual =
            if mime == "text/plain" && !selection.mime_types().iter().any(|value| value == mime) {
                [
                    "text/plain;charset=utf-8",
                    "text/plain;charset=UTF-8",
                    "UTF8_STRING",
                ]
                .into_iter()
                .find(|candidate| {
                    selection
                        .mime_types()
                        .iter()
                        .any(|value| value == candidate)
                })
                .unwrap_or(mime)
            } else {
                mime
            };
        return selection
            .receive(actual, crate::wayland::MAX_TRANSFER_BYTES)
            .map_err(|e| e.to_string());
    }
    #[cfg(target_os = "linux")]
    if mime == "text/uri-list" {
        crate::runtime_display::validate_x11_display().map_err(|error| error.to_string())?;
        // Keep CRLF URI delimiters out of decoded paths; arboard's file parser splits only LF.
        return crate::capture::x11_clipboard::get_clipboard_files()
            .map(|paths| {
                paths
                    .iter()
                    .map(|path| file_uri(path) + "\r\n")
                    .collect::<String>()
                    .into_bytes()
            })
            .ok_or_else(|| "ClipboardFileRepresentationUnavailable".into());
    }
    let mut cb = open()?;
    match mime {
        "text/plain" => cb
            .get_text()
            .map(String::into_bytes)
            .map_err(|e| e.to_string()),
        "text/html" => cb
            .get()
            .html()
            .map(String::into_bytes)
            .map_err(|e| e.to_string()),
        "text/uri-list" => cb
            .get()
            .file_list()
            .map(|paths| {
                paths
                    .iter()
                    .map(|p| file_uri(&p.to_string_lossy()) + "\r\n")
                    .collect::<String>()
                    .into_bytes()
            })
            .map_err(|e| e.to_string()),
        "image/png" => {
            let data = cb.get_image().map_err(|e| e.to_string())?;
            let image = image::RgbaImage::from_raw(
                data.width as u32,
                data.height as u32,
                data.bytes.into_owned(),
            )
            .ok_or("invalid clipboard image")?;
            let mut out = std::io::Cursor::new(Vec::new());
            image
                .write_to(&mut out, image::ImageFormat::Png)
                .map_err(|e| e.to_string())?;
            Ok(out.into_inner())
        }
        _ => Err(format!("unsupported clipboard MIME: {mime}")),
    }
}

pub fn file_uri(path: &str) -> String {
    let path = if path.starts_with('/') {
        std::borrow::Cow::Borrowed(path)
    } else {
        std::borrow::Cow::Owned(path.replace('\\', "/"))
    };
    let prefix = if path.starts_with("//") {
        "file:"
    } else if path.starts_with('/') {
        "file://"
    } else {
        "file:///"
    };
    let encoded = path
        .split('/')
        .enumerate()
        .map(|(index, segment)| {
            if index == 0
                && segment.len() == 2
                && segment.ends_with(':')
                && segment.as_bytes()[0].is_ascii_alphabetic()
            {
                segment.to_string()
            } else {
                urlencoding::encode(segment).into_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("/");
    format!("{prefix}{encoded}")
}

pub fn formats() -> Result<Vec<String>, String> {
    #[cfg(target_os = "linux")]
    if crate::wayland::requested() {
        return match crate::wayland::WaylandClipboardBackend::snapshot() {
            Ok(snapshot) => Ok(snapshot.mime_types().to_vec()),
            Err(crate::wayland::WaylandError::EmptySelection) => Ok(Vec::new()),
            Err(error) => Err(error.to_string()),
        };
    }
    let mut cb = open()?;
    let mut result = Vec::new();
    if cb.get_text().is_ok() {
        result.push("text/plain");
    }
    if cb.get().html().is_ok() {
        result.push("text/html");
    }
    if cb.get_image().is_ok() {
        result.push("image/png");
    }
    if cb.get().file_list().is_ok() {
        result.push("text/uri-list");
    }
    Ok(result.into_iter().map(str::to_owned).collect())
}

fn set(cb: &mut arboard::Clipboard, payload: Payload) -> Result<(), String> {
    let (payload, private) = match payload {
        Payload::Private(payload) => (*payload, true),
        payload => (payload, false),
    };
    let setter = cb.set();
    #[cfg(target_os = "windows")]
    let setter = {
        use arboard::SetExtWindows;
        if private {
            setter.exclude_from_monitoring()
        } else {
            setter
        }
    };
    #[cfg(target_os = "linux")]
    let setter = {
        use arboard::SetExtLinux;
        if private {
            setter.exclude_from_history()
        } else {
            setter
        }
    };
    let result = match payload {
        Payload::Text(text) => setter.text(text),
        Payload::Html { html, text } => setter.html(html, Some(text)),
        Payload::Files(paths) => setter.file_list(&paths),
        Payload::Image(bytes) => {
            let image = image::load_from_memory(&bytes)
                .map_err(|_| "ClipboardImageRejected")?
                .into_rgba8();
            setter.image(arboard::ImageData {
                width: image.width() as usize,
                height: image.height() as usize,
                bytes: std::borrow::Cow::Owned(image.into_raw()),
            })
        }
        Payload::Private(_) | Payload::LeasedFiles { .. } | Payload::Leased { .. } => {
            return Err("NativeLeaseOwnerRequired".into())
        }
    };
    result.map_err(|_| "ClipboardWriteUnavailable".into())
}
pub fn write_private(
    payload: Payload,
    lease: Option<dzc_core::api::NativeReadLease>,
) -> Result<(), String> {
    let lease = lease.ok_or("NativeLeaseOwnerRequired")?;
    write(Payload::Leased {
        payload: Box::new(Payload::Private(Box::new(payload))),
        lease,
    })
}

#[cfg(target_os = "linux")]
static PRIVATE_OWNERS: std::sync::OnceLock<std::sync::Mutex<Vec<std::process::Child>>> =
    std::sync::OnceLock::new();
pub(crate) fn revoke_private_owners() -> Result<(), String> {
    revoke_embedded_private_owners()?;
    #[cfg(target_os = "windows")]
    private_windows::revoke_all()?;
    #[cfg(target_os = "linux")]
    crate::wayland::revoke_private_owner().map_err(|_| "PrivateExportRevocationUnconfirmed")?;
    #[cfg(target_os = "linux")]
    private_x11_files::drain()?;
    #[cfg(target_os = "linux")]
    if let Some(owners) = PRIVATE_OWNERS.get() {
        let mut owners = owners
            .lock()
            .map_err(|_| "PrivateExportRevocationUnconfirmed")?;
        let children = std::mem::take(&mut *owners);
        let mut pending = Vec::new();
        for mut child in children {
            if matches!(child.try_wait(), Ok(Some(_))) {
                continue;
            }
            let _ = child.kill();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    _ if std::time::Instant::now() < deadline => {
                        std::thread::sleep(std::time::Duration::from_millis(10))
                    }
                    _ => {
                        pending.push(child);
                        break;
                    }
                }
            }
        }
        *owners = pending;
        if !owners.is_empty() {
            return Err("PrivateExportRevocationUnconfirmed".into());
        }
    }
    Ok(())
}

pub fn write(payload: Payload) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        if matches!(&payload, Payload::Private(_) | Payload::Leased { .. } | Payload::LeasedFiles { .. }) {
            private_windows::write_standalone(payload)
        } else {
            set(&mut open()?, payload)
        }
    }
    #[cfg(target_os = "linux")]
    {
        use std::io::BufRead;
        use std::process::{Command, Stdio};
        use std::time::Duration;
        let private = matches!(
            &payload,
            Payload::Private(_) | Payload::Leased { .. } | Payload::LeasedFiles { .. }
        ) || matches!(&payload,Payload::Private(inner) if matches!(inner.as_ref(),Payload::LeasedFiles{..}));
        let mut owners = if private {
            Some(
                PRIVATE_OWNERS
                    .get_or_init(Default::default)
                    .lock()
                    .map_err(|_| "PrivateExportRevocationUnconfirmed")?,
            )
        } else {
            None
        };
        let mut child = Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
            .arg("clipboard-owner")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        let data = serde_json::to_vec(&payload).map_err(|e| e.to_string())?;
        if let Err(error) = child
            .stdin
            .take()
            .ok_or("owner stdin unavailable")?
            .write_all(&data)
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error.to_string());
        }
        let stdout = child.stdout.take().ok_or("owner stdout unavailable")?;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let result = std::io::BufReader::new(stdout)
                .read_line(&mut line)
                .map(|_| line);
            let _ = tx.send(result);
        });
        let timeout = if crate::wayland::requested() { 15 } else { 5 };
        match rx.recv_timeout(Duration::from_secs(timeout)) {
            Ok(Ok(line)) if line.trim() == "ready" => {
                if private {
                    let owners = owners
                        .as_mut()
                        .ok_or("PrivateExportRevocationUnconfirmed")?;
                    owners.retain_mut(|owner| !matches!(owner.try_wait(), Ok(Some(_))));
                    owners.push(child);
                }
                Ok(())
            }
            result => {
                let _ = child.kill();
                let _ = child.wait();
                Err(format!(
                    "Native clipboard owner failed to acquire selection: {result:?}"
                ))
            }
        }
    }
}

/// Long-lived hosts retain their selection owner in this process.
pub fn write_embedded(payload: Payload) -> Result<(), String> {
    write_embedded_with_intent(payload, false)
}
pub fn write_embedded_with_intent(
    payload: Payload,
    suppress_recapture: bool,
) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let _ = suppress_recapture;
    #[cfg(target_os = "windows")]
    {
        let prepared = prepare_owner_payload(payload)?;
        if prepared.private {
            let owner = private_windows::Owner::start(prepared.payload, prepared.leases, prepared.exports)?;
            private_windows::retain(owner);
            Ok(())
        } else {
            set(&mut open()?, prepared.payload)
        }
    }
    #[cfg(target_os = "linux")]
    {
        if crate::wayland::requested() {
            return crate::wayland::write_in_process_with_intent(payload, suppress_recapture)
                .map_err(|error| error.to_string());
        }
        use crate::capture::x11_clipboard as x11;
        match payload {
            Payload::Text(text) => x11::set_clipboard_text_and_html(text, None),
            Payload::Html { html, text } => x11::set_clipboard_text_and_html(text, Some(html)),
            Payload::Files(paths) => x11::set_clipboard_files(paths),
            Payload::Private(_) | Payload::Leased { .. } | Payload::LeasedFiles { .. } => {
                return Err("NativeLeaseOwnerRequired".into());
            }
            Payload::Image(bytes) => {
                let image = image::load_from_memory(&bytes)
                    .map_err(|_| "ClipboardImageInvalid")?
                    .into_rgba8();
                x11::set_clipboard_image_with_formats(x11::ImageData {
                    width: image.width() as usize,
                    height: image.height() as usize,
                    bytes: image.into_raw(),
                })
            }
        }
    }
}

pub fn serve_owner() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    { private_windows::serve_owner() }
    #[cfg(target_os = "linux")]
    {
        use x11rb::{connection::Connection, protocol::xproto::ConnectionExt};
        let mut input = Vec::new();
        std::io::stdin()
            .take(256 * 1024 * 1024)
            .read_to_end(&mut input)
            .map_err(|e| e.to_string())?;
        let payload: Payload =
            serde_json::from_slice(&input).map_err(|_| "OwnerPayloadRejected")?;
        let prepared = prepare_owner_payload(payload)?;
        if crate::wayland::requested() {
            let payloads = crate::wayland::representations(prepared.payload)
                .map_err(|error| error.to_string())?;
            let (cancel, _keep_alive) =
                crate::wayland::WaylandClipboardBackend::cancellation_pair()
                    .map_err(|error| error.to_string())?;
            return crate::wayland::WaylandClipboardBackend::serve_authorized(
                payloads,
                prepared.leases,
                prepared.exports,
                || {
                    println!("ready");
                    let _ = std::io::stdout().flush();
                },
                cancel,
            )
            .map_err(|error| error.to_string());
        }
        if matches!(&prepared.payload,Payload::Private(inner) if matches!(inner.as_ref(),Payload::Files(_)))
        {
            let Payload::Private(inner) = prepared.payload else {
                unreachable!()
            };
            let Payload::Files(paths) = *inner else {
                unreachable!()
            };
            let mut owner =
                private_x11_files::FileOwner::start(paths, prepared.leases, prepared.exports)?;
            println!("ready");
            std::io::stdout()
                .flush()
                .map_err(|_| "PrivateFileOwnerUnavailable")?;
            return owner.wait();
        }
        let leases = prepared.leases;
        let exports = prepared.exports;
        let payload = prepared.payload;
        let mut cb = open()?;
        set(&mut cb, payload)?;
        let (conn, _) = x11rb::connect(None).map_err(|e| e.to_string())?;
        let atom = conn
            .intern_atom(false, b"CLIPBOARD")
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?
            .atom;
        let owner = conn
            .get_selection_owner(atom)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?
            .owner;
        if owner == 0 {
            return Err("clipboard selection has no owner".to_string());
        }
        println!("ready");
        std::io::stdout().flush().map_err(|e| e.to_string())?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
        loop {
            if !leases.is_empty()
                && (leases.iter().any(|lease| !lease.is_current())
                    || std::time::Instant::now() >= deadline)
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
            let current = conn
                .get_selection_owner(atom)
                .map_err(|e| e.to_string())?
                .reply()
                .map_err(|e| e.to_string())?
                .owner;
            if current != owner {
                break;
            }
            conn.flush().map_err(|e| e.to_string())?;
        }
        if let Some(mut exports) = exports {
            exports.close()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::file_uri;

    #[test]
    fn private_payload_without_original_native_lease_is_rejected_before_io() {
        assert_eq!(super::prepare_owner_payload(super::Payload::Private(Box::new(super::Payload::Text("private".into())))).err().as_deref(), Some("NativeLeaseOwnerRequired"));
        assert_eq!(super::write_private(super::Payload::Text("private".into()), None).err().as_deref(), Some("NativeLeaseOwnerRequired"));
    }

    #[test]
    fn file_uris_escape_path_segments_and_keep_absolute_roots() {
        assert_eq!(file_uri("/tmp/a b#c.txt"), "file:///tmp/a%20b%23c.txt");
        assert_eq!(
            file_uri(r"C:\My Files\a.txt"),
            "file:///C:/My%20Files/a.txt"
        );
        assert_eq!(
            file_uri(r"\\server\shared files\a.txt"),
            "file://server/shared%20files/a.txt"
        );
        assert_eq!(file_uri("/tmp/name:part"), "file:///tmp/name%3Apart");
        assert_eq!(file_uri(r"/tmp/name\part"), "file:///tmp/name%5Cpart");
    }
}
