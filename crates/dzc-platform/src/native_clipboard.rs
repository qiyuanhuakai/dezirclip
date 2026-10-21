//! Native clipboard access shared by the standalone CLI and its X11 owner process.
use serde::{Deserialize, Serialize};
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

fn open() -> Result<arboard::Clipboard, String> {
    #[cfg(target_os = "linux")]
    crate::runtime_display::validate_x11_display().map_err(|error| error.to_string())?;
    arboard::Clipboard::new().map_err(|e| format!("clipboard: {e}"))
}

pub fn read(mime: &str) -> Result<Vec<u8>, String> {
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

pub fn formats() -> Result<Vec<&'static str>, String> {
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
    Ok(result)
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
    #[cfg(target_os = "linux")]
    {
        let lease = lease.ok_or("NativeLeaseOwnerRequired")?;
        write(Payload::Leased {
            payload: Box::new(Payload::Private(Box::new(payload))),
            lease,
        })
    }
    #[cfg(target_os = "windows")]
    {
        let _ = lease;
        write(Payload::Private(Box::new(payload)))
    }
}

#[cfg(target_os = "linux")]
static PRIVATE_OWNERS: std::sync::OnceLock<std::sync::Mutex<Vec<std::process::Child>>> =
    std::sync::OnceLock::new();
pub(crate) fn revoke_private_owners() -> Result<(), String> {
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
        set(&mut open()?, payload)
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
        match rx.recv_timeout(Duration::from_secs(5)) {
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
                    "X11 clipboard owner failed to acquire selection: {result:?}"
                ))
            }
        }
    }
}

pub fn serve_owner() -> Result<(), String> {
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
        let (payload, mut lease) = match payload {
            Payload::Leased { payload, lease } => {
                if !lease.is_current() {
                    return Err("PrivacyEpochChanged".into());
                }
                (*payload, Some(lease))
            }
            value => (value, None),
        };
        let (payload, private) = match payload {
            Payload::Private(inner) => (*inner, true),
            value => (value, false),
        };
        let mut exports = None;
        let payload = match payload {
            Payload::LeasedFiles {
                files,
                lease: permission,
            } => {
                if !permission.is_current() {
                    return Err("PrivacyEpochChanged".into());
                }
                let files = files
                    .into_iter()
                    .map(|file| dzc_core::ports::ClipboardFile {
                        name: file.name,
                        bytes: file.bytes,
                    })
                    .collect();
                let owned = crate::private_files::Exports::create(files)?;
                let paths = owned.paths();
                exports = Some(owned);
                lease = Some(permission);
                Payload::Private(Box::new(Payload::Files(paths)))
            }
            value => {
                if private {
                    Payload::Private(Box::new(value))
                } else {
                    value
                }
            }
        };
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
            if lease
                .as_ref()
                .is_some_and(|lease| !lease.is_current() || std::time::Instant::now() >= deadline)
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
        drop(exports);
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        Err("clipboard owner helper is only used on X11".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::file_uri;

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
