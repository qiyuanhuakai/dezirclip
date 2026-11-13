//! Prepare public payloads before acquiring the system-wide clipboard lock.
use super::{Payload, private_windows::{html_bytes, unicode}};

fn representations(payload: Payload) -> Result<Vec<(u32, Vec<u8>)>, String> {
    let formats = match payload {
        Payload::Text(text) => vec![(13, unicode(text.split('\0').next().unwrap_or_default())?)],
        Payload::Html { html, text } => {
            let format = clipboard_win::register_format("HTML Format").ok_or("ClipboardFormatUnavailable")?;
            vec![(13, unicode(text.split('\0').next().unwrap_or_default())?), (format.get(), html_bytes(&html))]
        }
        Payload::Files(paths) => {
            if paths.is_empty() { return Err("ClipboardFilesRejected".into()); }
            let mut bytes = vec![0u8; 20];
            bytes[..4].copy_from_slice(&20u32.to_le_bytes());
            bytes[16..20].copy_from_slice(&1u32.to_le_bytes());
            for path in paths {
                let path = dunce::canonicalize(path).map_err(|_| "ClipboardFileUnavailable")?;
                let path = path.to_str().ok_or("ClipboardFilePathRejected")?;
                bytes.extend(unicode(path)?);
                if bytes.len() > 64 * 1024 * 1024 { return Err("ClipboardPayloadTooLarge".into()); }
            }
            bytes.extend([0, 0]);
            vec![(15, bytes)]
        }
        _ => return Err("ClipboardPayloadRejected".into()),
    };
    if formats.iter().any(|(_, bytes)| bytes.len() > 64 * 1024 * 1024) {
        return Err("ClipboardPayloadTooLarge".into());
    }
    Ok(formats)
}

pub(super) fn write(payload: Payload) -> Result<(), String> {
    super::windows_image::write_formats(representations(payload)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_and_html_keep_whole_payload_and_fragment_offsets() {
        let text = "中文 😀\r\nsecond line";
        assert_eq!(representations(Payload::Text(text.into())).unwrap(), vec![(13, unicode(text).unwrap())]);
        let formats = representations(Payload::Html { html: "<b>中文 😀</b>".into(), text: text.into() }).unwrap();
        assert_eq!(formats[0], (13, unicode(text).unwrap()));
        assert_eq!(formats[1].1, html_bytes("<b>中文 😀</b>"));
        assert!(representations(Payload::Files(Vec::new())).is_err());
        assert_eq!(representations(Payload::Text("first\0second".into())).unwrap(), vec![(13, unicode("first").unwrap())]);
    }

    #[test]
    fn own_notification_is_identified_without_acquiring_the_clipboard() {
        if std::env::var("DZC_TEST_CLIPBOARD_READER").as_deref() != Ok("1") { return; }
        assert_eq!(std::env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
        assert_eq!(std::env::var("RUNNER_ENVIRONMENT").as_deref(), Ok("github-hosted"));
        write(Payload::Text("synthetic local publication 中文 😀".into())).unwrap();
        let sequence = crate::capture::sequence_number();
        assert!(crate::capture::owns_notification(sequence));
        assert!(!crate::capture::owns_notification(sequence.wrapping_add(1)));
        // Another thread holds the clipboard throughout detection. Any attempt
        // to reacquire it would fail, delaying or breaking the recipient's paste.
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let holder = std::thread::spawn(move || {
            let _lock = clipboard_win::Clipboard::new_attempts(10).unwrap();
            ready_tx.send(()).unwrap();
            done_rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        });
        ready_rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        let recognized = crate::capture::owns_notification(sequence);
        done_tx.send(()).unwrap();
        holder.join().unwrap();
        assert!(recognized);
        let mut clipboard = arboard::Clipboard::new().unwrap();
        assert_eq!(clipboard.get_text().unwrap(), "synthetic local publication 中文 😀");
        clipboard.set_text("synthetic external publication").unwrap();
        assert!(!crate::capture::owns_notification(sequence));
        assert!(!crate::capture::owns_notification(crate::capture::sequence_number()));
        clipboard.clear().unwrap();
    }
}
