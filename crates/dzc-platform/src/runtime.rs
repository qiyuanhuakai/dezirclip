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
        crate::private_files::revoke_clipboard();
        crate::native_clipboard::revoke_private_owners()
            .map_err(|_| clipboard_failure(Capability::Write))?;
        let (payload, mut authorization) = match payload {
            ClipboardPayload::Private {
                payload,
                authorization,
            } => (*payload, Some(authorization)),
            payload => (payload, None),
        };
        if authorization
            .as_ref()
            .is_some_and(|authorization| !authorization.is_current())
        {
            return Err(clipboard_failure(Capability::Write));
        }
        #[cfg(target_os = "windows")]
        let mut private: Option<crate::private_files::BoundExport> = None;
        let payload = match payload {
            ClipboardPayload::Private { .. } => return Err(clipboard_failure(Capability::Write)),
            ClipboardPayload::ProtectedFiles {
                files,
                authorization: permission,
            } => {
                if !permission.is_current() {
                    return Err(clipboard_failure(Capability::Write));
                }
                #[cfg(target_os = "linux")]
                {
                    let lease = permission
                        .native_lease()
                        .ok_or_else(|| clipboard_failure(Capability::Write))?;
                    authorization = Some(permission);
                    crate::native_clipboard::Payload::LeasedFiles {
                        files: files
                            .into_iter()
                            .map(|file| crate::native_clipboard::NativeFile {
                                name: file.name,
                                bytes: file.bytes,
                            })
                            .collect(),
                        lease,
                    }
                }
                #[cfg(target_os = "windows")]
                {
                    let exports =
                        crate::private_files::BoundExport::readonly(files, permission.clone())
                            .map_err(|_| clipboard_failure(Capability::Write))?;
                    let paths = exports
                        .paths()
                        .map_err(|_| clipboard_failure(Capability::Write))?;
                    private = Some(exports);
                    authorization = Some(permission);
                    crate::native_clipboard::Payload::Files(paths)
                }
            }
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
        if authorization
            .as_ref()
            .is_some_and(|authorization| !authorization.is_current())
        {
            return Err(clipboard_failure(Capability::Write));
        }
        if authorization.is_some() {
            crate::native_clipboard::write_private(
                payload,
                authorization
                    .as_ref()
                    .and_then(ClipboardAuthorization::native_lease),
            )
        } else {
            crate::native_clipboard::write(payload)
        }
        .map_err(|_| clipboard_failure(Capability::Write))?;
        #[cfg(target_os = "windows")]
        if let Some(exports) = private {
            exports.retain();
        }
        if authorization
            .as_ref()
            .is_some_and(|authorization| !authorization.is_current())
        {
            revoke_private_exports()?;
            return Err(clipboard_failure(Capability::Write));
        }
        Ok(())
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

/// The unique native owner calls this synchronously before acknowledging a privacy epoch change.
pub fn revoke_private_exports() -> PlatformResult<()> {
    crate::private_files::revoke();
    crate::native_clipboard::revoke_private_owners()
        .map_err(|_| PlatformError::failed(Capability::Write))
}

/// Native-only export handles; paths never belong to a WebView or external Surface response.
pub struct PrivateFileExport(crate::private_files::BoundExport);
impl PrivateFileExport {
    pub fn paths(&self) -> PlatformResult<Vec<String>> {
        self.0
            .paths()
            .map_err(|_| clipboard_failure(Capability::Write))
    }
    pub fn is_current(&self) -> bool {
        self.0.paths().is_ok()
    }
    pub fn retain(self) {
        self.0.retain();
    }
}
pub fn private_file_export(
    export: dzc_core::ports::AuthorizedFileExport,
) -> PlatformResult<PrivateFileExport> {
    crate::private_files::BoundExport::new(export.files, export.authorization)
        .map(PrivateFileExport)
        .map_err(|_| clipboard_failure(Capability::Write))
}

pub fn prepare_private_files(
    files: Vec<ClipboardFile>,
    authorization: ClipboardAuthorization,
) -> PlatformResult<PrivateFileExport> {
    crate::private_files::BoundExport::readonly(files, authorization)
        .map(PrivateFileExport)
        .map_err(|_| clipboard_failure(Capability::Write))
}
