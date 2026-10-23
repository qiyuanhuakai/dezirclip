//! Native clipboard readers and notification workers; no desktop runtime is required.
mod html;
mod echo;
mod listener;
mod reader;
mod subscription;
#[cfg(any(target_os = "windows", test))]
mod acquisition;
#[cfg(target_os = "windows")]
pub mod windows_clipboard;
#[cfg(target_os = "windows")]
pub mod windows_privacy;
#[cfg(target_os = "linux")]
pub mod x11_clipboard;
#[cfg(target_os = "linux")]
pub mod x11_privacy;
#[cfg(target_os = "linux")]
mod x11_listener;

pub use html::parse_cf_html;
pub use echo::{clipboard_echo, ClipboardEcho};
pub use listener::{listen_clipboard, probe_clipboard_monitor, run_clipboard_monitor};
pub use reader::NativeCaptureReader;
pub use subscription::{subscribe_clipboard, ClipboardSubscription};

#[derive(Debug, Clone, Copy)]
pub struct ClipboardChange {
    pub sequence: u32,
}

pub fn sequence_number() -> u32 {
    #[cfg(target_os = "windows")]
    {
        windows_clipboard::get_clipboard_sequence_number()
    }
    #[cfg(target_os = "linux")]
    {
        use std::sync::atomic::{AtomicU32, Ordering};
        static SEQUENCE: AtomicU32 = AtomicU32::new(1);
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    }
}
