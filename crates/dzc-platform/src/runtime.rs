//! Adapters for directly linked clients. Probes do not read clipboard content or start workers.
use dzc_core::api::ClipboardService;
use dzc_core::ports::*;
use dzc_protocol::{
    Capability, CapabilityState, PlatformCapabilities, PlatformError, UnsupportedReason,
};

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "linux")]
mod x11;
#[cfg(target_os = "windows")]
use self::windows as native;
#[cfg(target_os = "linux")]
use self::x11 as native;
mod monitor;
pub use monitor::NativeClipboardMonitor;

fn clipboard_failure(operation: Capability) -> PlatformError {
    match native::clipboard_probe().availability {
        dzc_protocol::Availability::Unsupported { reason } => {
            PlatformError::unsupported(operation, reason)
        }
        dzc_protocol::Availability::Available => PlatformError::failed(operation),
    }
}

#[derive(Clone, Copy)]
pub struct NativeClipboard;
impl ClipboardReader for NativeClipboard {
    fn capability(&self) -> CapabilityState {
        native::clipboard_probe()
    }
    fn formats(&self) -> PlatformResult<Vec<String>> {
        crate::native_clipboard::formats()
            .map(|items| items.into_iter().map(str::to_owned).collect())
            .map_err(|_| clipboard_failure(Capability::Read))
    }
    fn read_representation(&self, mime: &str) -> PlatformResult<Vec<u8>> {
        if !matches!(
            mime,
            "text/plain" | "text/html" | "image/png" | "text/uri-list"
        ) {
            return Err(PlatformError::unsupported(
                Capability::Read,
                UnsupportedReason::UnsupportedRepresentation,
            ));
        }
        crate::native_clipboard::read(mime).map_err(|_| clipboard_failure(Capability::Read))
    }
}
/// The X11 writer uses the existing standalone CLI selection-owner helper.
/// Embedded graphical clients inject their in-process writer instead.
#[derive(Clone, Copy)]
pub struct StandaloneClipboardWriter;
impl ClipboardWriter for StandaloneClipboardWriter {
    fn capability(&self) -> CapabilityState {
        native::clipboard_probe()
    }
    fn write(&self, payload: ClipboardPayload) -> PlatformResult<()> {
        let payload = match payload {
            ClipboardPayload::Text(value) => crate::native_clipboard::Payload::Text(value),
            ClipboardPayload::Html { html, text } => {
                crate::native_clipboard::Payload::Html { html, text }
            }
            ClipboardPayload::Files(paths) => crate::native_clipboard::Payload::Files(paths),
            ClipboardPayload::Image { bytes, mime } => {
                if !matches!(
                    mime.as_str(),
                    "image/png" | "image/jpeg" | "image/gif" | "image/webp" | "image/bmp"
                ) {
                    return Err(PlatformError::unsupported(
                        Capability::Write,
                        UnsupportedReason::UnsupportedRepresentation,
                    ));
                }
                crate::native_clipboard::Payload::Image(bytes)
            }
        };
        crate::native_clipboard::write(payload).map_err(|_| clipboard_failure(Capability::Write))
    }
}
pub fn standalone_clipboard_service() -> ClipboardService<NativeClipboard, StandaloneClipboardWriter>
{
    ClipboardService::new(NativeClipboard, StandaloneClipboardWriter)
}

pub struct NativeSourceResolver;
impl SourceResolver for NativeSourceResolver {
    fn capability(&self) -> CapabilityState {
        native::source_probe()
    }
    fn resolve_source(&self) -> PlatformResult<SourceIdentity> {
        native::source()
    }
}
pub struct NativePasteController;
impl PasteController for NativePasteController {
    fn capability(&self) -> CapabilityState {
        native::paste_probe()
    }
    fn paste(&self) -> PlatformResult<()> {
        native::paste()
    }
}

/// Reports this client's attached adapters. A successful probe never implies an active worker.
pub fn standalone_capabilities() -> PlatformCapabilities {
    let monitor = NativeClipboardMonitor::default();
    PlatformPorts {
        reader: Some(&NativeClipboard),
        writer: Some(&StandaloneClipboardWriter),
        monitor: Some(&monitor),
        source: Some(&NativeSourceResolver),
        paste: Some(&NativePasteController),
        ..Default::default()
    }
    .capabilities()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capabilities_are_local_and_do_not_claim_unattached_services() {
        let capabilities = standalone_capabilities();
        assert_eq!(capabilities.scope, "embedded_client");
        assert!(!capabilities.read.active);
        assert!(!capabilities.monitor.active);
        assert!(!capabilities.global_shortcut.is_available());
        assert!(!capabilities.read_privacy_hints.is_available());
    }
    #[test]
    fn unsupported_mime_is_rejected_before_clipboard_access() {
        assert_eq!(
            NativeClipboard
                .read_representation("application/x-unknown")
                .unwrap_err()
                .reason,
            Some(UnsupportedReason::UnsupportedRepresentation)
        );
    }
}
