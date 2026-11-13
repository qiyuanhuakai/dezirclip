//! A process-local publication marker avoids reopening our own clipboard data.
use std::sync::OnceLock;
use windows::{core::PCWSTR, Win32::System::DataExchange::{
    GetClipboardSequenceNumber, IsClipboardFormatAvailable, RegisterClipboardFormatW,
}};

pub(crate) fn format() -> Result<u32, String> {
    static FORMAT: OnceLock<u32> = OnceLock::new();
    let format = *FORMAT.get_or_init(|| {
        let name: Vec<u16> = format!("DezirClip.Publication.{}", uuid::Uuid::new_v4())
            .encode_utf16().chain(Some(0)).collect();
        unsafe { RegisterClipboardFormatW(PCWSTR(name.as_ptr())) }
    });
    if format == 0 { Err("ClipboardMarkerUnavailable".into()) } else { Ok(format) }
}

pub(super) fn owns_notification(sequence: u32) -> bool {
    let Ok(format) = format() else { return false; };
    // Availability does not open the clipboard or request delayed data. Both
    // sequence checks bind the marker to this notification, never a later copy.
    unsafe {
        sequence != 0 && GetClipboardSequenceNumber() == sequence
            && IsClipboardFormatAvailable(format).is_ok()
            && GetClipboardSequenceNumber() == sequence
    }
}
