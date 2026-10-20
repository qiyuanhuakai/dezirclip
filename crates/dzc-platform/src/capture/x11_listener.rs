use super::{sequence_number, ClipboardChange};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;

use x11rb::connection::Connection;
use x11rb::protocol::xfixes::{ConnectionExt as _, SelectionEvent, SelectionEventMask};
use x11rb::protocol::xproto::ConnectionExt as _;
use x11rb::protocol::Event;

const POLL_INTERVAL: Duration = Duration::from_millis(100);

pub fn probe_clipboard_monitor() -> Result<&'static str, String> {
    if service_disabled("clipboard_listener") {
        return Err("ClipboardMonitorDisabled".into());
    }
    crate::runtime_display::validate_x11_display().map_err(|error| error.to_string())?;
    if !service_disabled("clipboard_xfixes") && connect_xfixes().is_ok() {
        return Ok("x11-xfixes");
    }
    arboard::Clipboard::new()
        .map(|_| "x11-poll")
        .map_err(|error| error.to_string())
}

fn service_disabled_in(value: &str, service: &str) -> bool {
    value
        .split([',', ';', ' '])
        .filter(|item| !item.is_empty())
        .any(|item| item.eq_ignore_ascii_case(service) || item.eq_ignore_ascii_case("all"))
}

fn service_disabled(service: &str) -> bool {
    std::env::var("DEZIRCLIP_DISABLE_LINUX_SERVICES")
        .or_else(|_| std::env::var("TIEZ_DISABLE_LINUX_SERVICES"))
        .map(|value| service_disabled_in(&value, service))
        .unwrap_or(false)
}

pub fn run_clipboard_monitor(
    callback: Arc<dyn Fn(ClipboardChange) + Send + Sync + 'static>,
) -> Result<(), String> {
    if service_disabled("clipboard_listener") {
        eprintln!("[clipboard] Linux clipboard listener disabled by environment");
        return Ok(());
    }
    crate::runtime_display::validate_x11_display().map_err(|error| error.to_string())?;

    if !service_disabled("clipboard_xfixes") {
        if let Err(error) = listen_xfixes(&callback) {
            eprintln!("[clipboard] XFixes unavailable ({error}); using clipboard polling");
        }
    }
    poll_clipboard(&callback)
}

fn listen_xfixes(
    callback: &Arc<dyn Fn(ClipboardChange) + Send + Sync + 'static>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (connection, clipboard) = connect_xfixes()?;
    eprintln!("[clipboard] X11 XFixes clipboard listener started");
    loop {
        if let Event::XfixesSelectionNotify(event) = connection.wait_for_event()? {
            if event.selection == clipboard
                && event.owner != x11rb::NONE
                && event.subtype == SelectionEvent::SET_SELECTION_OWNER
            {
                callback(ClipboardChange {
                    sequence: sequence_number(),
                });
            }
        }
    }
}

fn connect_xfixes(
) -> Result<(x11rb::rust_connection::RustConnection, u32), Box<dyn std::error::Error>> {
    let (connection, screen) = x11rb::connect(None)?;
    connection.xfixes_query_version(5, 0)?.reply()?;
    let clipboard = connection.intern_atom(false, b"CLIPBOARD")?.reply()?.atom;
    let root = connection.setup().roots[screen].root;
    connection
        .xfixes_select_selection_input(
            root,
            clipboard,
            SelectionEventMask::SET_SELECTION_OWNER
                | SelectionEventMask::SELECTION_WINDOW_DESTROY
                | SelectionEventMask::SELECTION_CLIENT_CLOSE,
        )?
        .check()?;
    connection.flush()?;
    Ok((connection, clipboard))
}

fn clipboard_fingerprint(
    text: Option<&str>,
    files: Option<&[String]>,
    image: Option<(usize, usize, &[u8])>,
) -> Option<u64> {
    if text.is_none() && files.is_none() && image.is_none() {
        return None;
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    files.hash(&mut hasher);
    image.hash(&mut hasher);
    Some(hasher.finish())
}

fn read_fingerprint(clipboard: &mut arboard::Clipboard) -> Option<u64> {
    let text = clipboard.get_text().ok();
    let files = super::x11_clipboard::get_clipboard_files();
    let image = clipboard.get_image().ok();
    clipboard_fingerprint(
        text.as_deref(),
        files.as_deref(),
        image
            .as_ref()
            .map(|image| (image.width, image.height, image.bytes.as_ref())),
    )
}

fn poll_clipboard(callback: &Arc<dyn Fn(ClipboardChange) + Send + Sync + 'static>) -> ! {
    let mut last_fingerprint = None;
    let mut initialized = false;
    loop {
        match arboard::Clipboard::new() {
            Ok(mut clipboard) => {
                let fingerprint = read_fingerprint(&mut clipboard);
                if initialized && fingerprint.is_some() && fingerprint != last_fingerprint {
                    callback(ClipboardChange {
                        sequence: sequence_number(),
                    });
                }
                initialized = true;
                last_fingerprint = fingerprint;
            }
            Err(_) => initialized = false,
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_only_copies_have_distinct_fingerprints() {
        let first = clipboard_fingerprint(None, None, Some((1, 1, &[0, 0, 0, 255])));
        let next = clipboard_fingerprint(None, None, Some((1, 1, &[255, 0, 0, 255])));
        assert!(first.is_some());
        assert_ne!(first, next);
        assert_eq!(clipboard_fingerprint(None, None, None), None);
    }

    #[test]
    fn fingerprints_include_dimensions_formats_and_raw_text() {
        let pixels = &[0, 0, 0, 255, 0, 0, 0, 255];
        assert_ne!(
            clipboard_fingerprint(None, None, Some((1, 2, pixels))),
            clipboard_fingerprint(None, None, Some((2, 1, pixels)))
        );
        assert_ne!(
            clipboard_fingerprint(Some(" a\r\n"), None, None),
            clipboard_fingerprint(Some("a\n"), None, None)
        );
        assert_ne!(
            clipboard_fingerprint(Some("/tmp/a"), None, None),
            clipboard_fingerprint(None, Some(&["/tmp/a".to_string()]), None)
        );
    }

    #[test]
    fn listener_opt_out_respects_named_services_and_all() {
        assert!(service_disabled_in(
            "mouse_hotkey; CLIPBOARD_XFIXES",
            "clipboard_xfixes"
        ));
        assert!(service_disabled_in("all", "clipboard_listener"));
        assert!(!service_disabled_in("window_tracker", "clipboard_listener"));
        assert!(!service_disabled_in(
            "clipboard_xfixes",
            "clipboard_listener"
        ));
    }
}
