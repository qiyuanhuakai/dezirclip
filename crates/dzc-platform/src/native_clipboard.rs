//! Native clipboard access shared by the standalone CLI and its X11 owner process.
use serde::{Deserialize, Serialize};
#[cfg(target_os = "linux")]
use std::io::{Read, Write};

#[derive(Serialize, Deserialize)]
pub enum Payload {
    Text(String),
    Html { html: String, text: String },
    Image(Vec<u8>),
    Files(Vec<String>),
}

/// Explicit long-lived host ownership. No connection is opened until write.
#[derive(Default)]
pub struct EmbeddedOwner {
    clipboard: Option<arboard::Clipboard>,
    #[cfg(target_os = "linux")]
    wayland: Option<crate::wayland::WaylandOwner>,
    stopped: bool,
}
impl EmbeddedOwner {
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
                .map_err(|e| e.to_string());
        }
        if self.clipboard.is_none() {
            self.clipboard = Some(open()?);
        }
        set(
            self.clipboard.as_mut().expect("clipboard opened above"),
            payload,
        )
    }
    /// Stop after draining this Host's writes/reads. This releases only this
    /// instance; unrelated process or legacy GUI owners are never targeted.
    pub fn stop(&mut self) -> Result<(), String> {
        self.stopped = true;
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
        Ok(())
    }
}
impl Drop for EmbeddedOwner {
    fn drop(&mut self) {
        let _ = self.stop();
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
    let result = match payload {
        Payload::Text(text) => cb.set_text(text),
        Payload::Html { html, text } => cb.set_html(html, Some(text)),
        Payload::Files(paths) => cb.set().file_list(&paths),
        Payload::Image(bytes) => {
            let image = image::load_from_memory(&bytes)
                .map_err(|e| e.to_string())?
                .into_rgba8();
            cb.set_image(arboard::ImageData {
                width: image.width() as usize,
                height: image.height() as usize,
                bytes: std::borrow::Cow::Owned(image.into_raw()),
            })
        }
    };
    result.map_err(|e| format!("clipboard write: {e}"))
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
            Ok(Ok(line)) if line.trim() == "ready" => Ok(()),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                Err("Native clipboard owner failed to acquire selection".into())
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
        write(payload)
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
    #[cfg(target_os = "linux")]
    {
        use x11rb::{connection::Connection, protocol::xproto::ConnectionExt};
        let mut input = Vec::new();
        std::io::stdin()
            .take(256 * 1024 * 1024)
            .read_to_end(&mut input)
            .map_err(|e| e.to_string())?;
        let payload = serde_json::from_slice(&input).map_err(|e| format!("owner payload: {e}"))?;
        if crate::wayland::requested() {
            let payloads = crate::wayland::representations(payload).map_err(|e| e.to_string())?;
            let (cancel, _keep_alive) =
                crate::wayland::WaylandClipboardBackend::cancellation_pair()
                    .map_err(|e| e.to_string())?;
            return crate::wayland::WaylandClipboardBackend::serve(
                payloads,
                || {
                    println!("ready");
                    let _ = std::io::stdout().flush();
                },
                cancel,
            )
            .map_err(|e| e.to_string());
        }
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
        loop {
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
