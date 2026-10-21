//! Adapters for directly linked clients. Probes do not read clipboard content or start workers.
use dzc_core::api::ClipboardService;
use dzc_core::ports::*;
use dzc_protocol::{
    Capability, CapabilityState, PlatformCapabilities, PlatformError, UnsupportedReason,
};

#[cfg(target_os = "linux")]
mod linux;
mod session;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "linux")]
mod x11;
#[cfg(target_os = "linux")]
use self::linux as native;
#[cfg(target_os = "windows")]
use self::windows as native;
pub use session::{is_native_wayland, session_capabilities, SessionCapabilityReport};
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
        crate::native_clipboard::formats().map_err(|_| clipboard_failure(Capability::Read))
    }
    fn read_representation(&self, mime: &str) -> PlatformResult<Vec<u8>> {
        if !matches!(
            mime,
            "text/plain"
                | "text/plain;charset=utf-8"
                | "text/plain;charset=UTF-8"
                | "UTF8_STRING"
                | "text/html"
                | "text/uri-list"
                | "image/png"
                | "image/jpeg"
                | "image/gif"
                | "image/webp"
                | "image/bmp"
        ) {
            return Err(PlatformError::unsupported(
                Capability::Read,
                UnsupportedReason::UnsupportedRepresentation,
            ));
        }
        #[cfg(target_os = "linux")]
        if crate::wayland::requested() {
            return crate::native_clipboard::read(mime)
                .map_err(|_| clipboard_failure(Capability::Read));
        }
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
fn native_payload(payload: ClipboardPayload) -> PlatformResult<crate::native_clipboard::Payload> {
    Ok(match payload {
        ClipboardPayload::Private { .. } | ClipboardPayload::ProtectedFiles { .. } => {
            return Err(clipboard_failure(Capability::Write));
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
    })
}
#[derive(Clone)]
pub struct EmbeddedClipboardWriter {
    owner: std::sync::Arc<std::sync::Mutex<crate::native_clipboard::EmbeddedOwner>>,
    private_files: std::sync::Arc<std::sync::Mutex<Option<PrivateFileExport>>>,
}
impl Default for EmbeddedClipboardWriter {
    fn default() -> Self {
        Self {
            owner: crate::native_clipboard::EmbeddedOwner::shared(),
            private_files: Default::default(),
        }
    }
}
impl EmbeddedClipboardWriter {
    /// Explicit copy of an already recorded item. Generated text uses write().
    pub fn write_suppressing_recapture(&self, payload: ClipboardPayload) -> PlatformResult<()> {
        self.write_with_intent(payload, true)
    }
    fn write_with_intent(&self, payload: ClipboardPayload, suppress: bool) -> PlatformResult<()> {
        let mut owner = self
            .owner
            .lock()
            .map_err(|_| PlatformError::failed(Capability::Write))?;
        let mut export_slot = self
            .private_files
            .lock()
            .map_err(|_| PlatformError::failed(Capability::Write))?;
        let (payload, authorization) = match payload {
            ClipboardPayload::Private {
                payload,
                authorization,
            } => (*payload, Some(authorization)),
            value => (value, None),
        };
        if authorization
            .as_ref()
            .is_some_and(|auth| !auth.is_current())
        {
            return Err(clipboard_failure(Capability::Write));
        }
        let mut export = None;
        let (payload, authorization) = match payload {
            ClipboardPayload::Private { .. } => return Err(clipboard_failure(Capability::Write)),
            ClipboardPayload::ProtectedFiles {
                files,
                authorization: permission,
            } => {
                // The in-process owner retains these handles. It never spawns the current executable.
                let files = prepare_private_files(files, permission.clone())?;
                let paths = files.paths()?;
                export = Some(files);
                (
                    crate::native_clipboard::Payload::Files(paths),
                    Some(permission),
                )
            }
            ordinary => (native_payload(ordinary)?, authorization),
        };
        let payload = if let Some(auth) = &authorization {
            if !auth.is_current() {
                return Err(clipboard_failure(Capability::Write));
            }
            let payload = crate::native_clipboard::Payload::Private(Box::new(payload));
            #[cfg(target_os = "linux")]
            let payload = crate::native_clipboard::Payload::Leased {
                payload: Box::new(payload),
                lease: auth
                    .native_lease()
                    .ok_or_else(|| clipboard_failure(Capability::Write))?,
            };
            payload
        } else {
            payload
        };
        owner
            .write_with_intent(payload, suppress)
            .map_err(|_| clipboard_failure(Capability::Write))?;
        *export_slot = export;
        if authorization
            .as_ref()
            .is_some_and(|auth| !auth.is_current())
        {
            // Do not recursively acquire the global owner registry while holding this owner.
            owner
                .revoke_private()
                .map_err(|_| PlatformError::failed(Capability::Write))?;
            *export_slot = None;
            return Err(clipboard_failure(Capability::Write));
        }
        Ok(())
    }
    pub fn stop(&self) -> PlatformResult<()> {
        self.owner
            .lock()
            .map_err(|_| PlatformError::failed(Capability::Write))?
            .stop()
            .map_err(|_| PlatformError::failed(Capability::Write))?;
        self.private_files
            .lock()
            .map_err(|_| PlatformError::failed(Capability::Write))?
            .take();
        Ok(())
    }
}
impl ClipboardWriter for EmbeddedClipboardWriter {
    fn capability(&self) -> CapabilityState {
        native::clipboard_probe()
    }
    fn write(&self, payload: ClipboardPayload) -> PlatformResult<()> {
        self.write_with_intent(payload, false)
    }
}

pub fn embedded_clipboard_service() -> ClipboardService<NativeClipboard, EmbeddedClipboardWriter> {
    ClipboardService::new(NativeClipboard, EmbeddedClipboardWriter::default())
}
/// Long-lived GUI callers retain one in-process owner; service Hosts own their separate instance.
pub fn shared_embedded_clipboard_writer() -> &'static EmbeddedClipboardWriter {
    static WRITER: std::sync::OnceLock<EmbeddedClipboardWriter> = std::sync::OnceLock::new();
    WRITER.get_or_init(EmbeddedClipboardWriter::default)
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

pub struct NativePrivacyHintProvider;
impl PrivacyHintProvider for NativePrivacyHintProvider {
    fn capability(&self) -> CapabilityState {
        #[cfg(target_os = "windows")]
        {
            return native::clipboard_probe();
        }
        #[cfg(target_os = "linux")]
        if crate::wayland::requested() {
            return native::clipboard_probe();
        }
        #[cfg(target_os = "linux")]
        x11::clipboard_probe().with_probe("x11_targets_and_bounded_kde_hint")
    }
    fn read_privacy_hints(&self) -> PlatformResult<Vec<PrivacyHint>> {
        #[cfg(target_os = "windows")]
        {
            return crate::capture::windows_privacy::preflight()
                .map(|result| result.hints)
                .map_err(|_| PlatformError::failed(Capability::ReadPrivacyHints));
        }
        #[cfg(target_os = "linux")]
        if crate::wayland::requested() {
            return match crate::wayland::WaylandClipboardBackend::snapshot() {
                Ok(snapshot) => snapshot
                    .password_hint()
                    .map(|hint| {
                        if hint == Some(true) {
                            vec![PrivacyHint::Sensitive]
                        } else {
                            vec![]
                        }
                    })
                    .map_err(|_| PlatformError::failed(Capability::ReadPrivacyHints)),
                Err(crate::wayland::WaylandError::EmptySelection) => Ok(vec![]),
                Err(_) => Err(PlatformError::failed(Capability::ReadPrivacyHints)),
            };
        }
        #[cfg(target_os = "linux")]
        {
            let snapshot = crate::capture::x11_privacy::SelectionSnapshot::preflight()
                .map_err(|_| PlatformError::failed(Capability::ReadPrivacyHints))?;
            let report = snapshot.report();
            if report.hints.is_empty()
                && report.availability != dzc_core::capture::PrivacyHintAvailability::Checked
            {
                return Err(PlatformError::unsupported(
                    Capability::ReadPrivacyHints,
                    UnsupportedReason::ExtensionUnavailable,
                ));
            }
            Ok(report.hints)
        }
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
        privacy_hints: Some(&NativePrivacyHintProvider),
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
        assert!(!capabilities.read_privacy_hints.active);
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

    #[cfg(target_os = "linux")]
    #[test]
    fn isolated_wayland_ports_owned_lifecycle_and_core_preflight() {
        if std::env::var("DEZIRCLIP_RUN_WAYLAND_SMOKE_TESTS").as_deref() != Ok("1") {
            return;
        }
        use crate::wayland::{MimeData, WaylandOwner};
        use dzc_core::api::{HistoryQuery, HistoryService};
        use dzc_core::capture::*;
        use std::collections::VecDeque;
        use std::sync::{Arc, Mutex};
        let runtime = std::path::PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap());
        assert!(
            runtime.is_absolute() && runtime.join("dezirclip-isolated-wayland-fixture").is_file()
        );
        assert!(std::env::var_os("DISPLAY").is_none());
        let report = session_capabilities();
        let negotiated = report
            .wayland
            .as_ref()
            .unwrap()
            .connection
            .as_ref()
            .unwrap();
        eprintln!(
            "synthetic_wayland_negotiation={}",
            serde_json::to_string(negotiated).unwrap()
        );
        if let Ok(expected) = std::env::var("DEZIRCLIP_EXPECT_WAYLAND_PROTOCOL") {
            assert_eq!(
                serde_json::to_value(&negotiated.protocol).unwrap(),
                serde_json::Value::String(expected)
            );
        }
        assert!(
            report.native_wayland
                && report.platform.read.is_available()
                && report.platform.write.is_available()
        );
        assert!(
            !report.platform.auto_paste.is_available()
                && !report.platform.global_shortcut.is_available()
        );
        assert!(!report.platform.resolve_source.is_available());
        assert!(NativePasteController.paste().is_err());
        assert!(NativeSourceResolver.resolve_source().is_err());
        let writer = EmbeddedClipboardWriter::default();
        writer
            .write_suppressing_recapture(ClipboardPayload::Text("synthetic owned text".into()))
            .unwrap();
        assert_eq!(
            NativeClipboard.read_representation("text/plain").unwrap(),
            b"synthetic owned text"
        );
        let mut own_reader = crate::capture::NativeCaptureReader::default();
        assert!(own_reader
            .read(CaptureReadRequest {
                files: true,
                rich_text: true
            })
            .unwrap()
            .is_none());
        assert_eq!(own_reader.empty_reason(), CaptureSkip::OwnClipboard);
        writer.clone().stop().unwrap();
        assert!(writer
            .write(ClipboardPayload::Text("must remain stopped".into()))
            .is_err());
        assert!(NativeClipboard.formats().unwrap().is_empty());

        let history = HistoryService::open(&runtime.join("synthetic-history.db")).unwrap();
        let store = history.capture_store(&runtime);
        let session = Mutex::new(VecDeque::new());
        let context = CaptureContext {
            source: CaptureSource::default(),
            timestamp: 12345,
            settings: CaptureSettings {
                persistent: true,
                capture_files: true,
                capture_rich_text: true,
                ..Default::default()
            },
            session: &session,
            paused: false,
            sequential_echo: None,
        };
        struct TracedReader {
            native: crate::capture::NativeCaptureReader,
            body_reads: usize,
        }
        impl CaptureReader for TracedReader {
            fn preflight_privacy(&mut self) -> Result<PrivacyPreflight, CaptureFailure> {
                self.native.preflight_privacy()
            }
            fn selection_id(&self) -> Option<u64> {
                self.native.selection_id()
            }
            fn read(
                &mut self,
                request: CaptureReadRequest,
            ) -> Result<Option<CaptureData>, CaptureFailure> {
                self.body_reads += 1;
                self.native.read(request)
            }
            fn metadata(&self) -> CaptureReadMetadata {
                self.native.metadata()
            }
            fn empty_reason(&self) -> CaptureSkip {
                self.native.empty_reason()
            }
        }
        let mut owner = WaylandOwner::start(vec![
            MimeData {
                mime: "text/plain".into(),
                bytes: Arc::from(&b"synthetic secret must not persist"[..]),
            },
            MimeData {
                mime: "x-kde-passwordManagerHint".into(),
                bytes: Arc::from(&b"secret"[..]),
            },
        ])
        .unwrap();
        owner.wait_ready().unwrap();
        owner.wait_ready().unwrap();
        assert_eq!(
            NativePrivacyHintProvider.read_privacy_hints().unwrap(),
            vec![PrivacyHint::Sensitive]
        );
        let mut reader = TracedReader {
            native: Default::default(),
            body_reads: 0,
        };
        assert!(matches!(
            CaptureEngine::capture(&context, &mut reader, &store, &mut ()),
            CaptureOutcome::Skipped(CaptureSkip::ExplicitPrivacyHint)
        ));
        assert_eq!(reader.body_reads, 0);
        assert!(history
            .query(&HistoryQuery {
                limit: 10,
                ..Default::default()
            })
            .unwrap()
            .is_empty());
        owner.stop().unwrap();
        assert!(owner.is_stopped());
        let mut owner = WaylandOwner::start(vec![
            MimeData {
                mime: "text/plain".into(),
                bytes: Arc::from(&b"synthetic ordinary text"[..]),
            },
            MimeData {
                mime: "text/html".into(),
                bytes: Arc::from(&b"<b>synthetic ordinary text</b>"[..]),
            },
        ])
        .unwrap();
        owner.wait_ready().unwrap();
        let mut reader = crate::capture::NativeCaptureReader::default();
        assert!(matches!(
            CaptureEngine::capture(&context, &mut reader, &store, &mut ()),
            CaptureOutcome::Stored(_)
        ));
        let entries = history
            .query(&HistoryQuery {
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].content, "synthetic ordinary text");
        assert_eq!(
            entries[0].html_content.as_deref(),
            Some("<b>synthetic ordinary text</b>")
        );
        owner.stop().unwrap();
        assert!(owner.is_stopped());
        let writer = EmbeddedClipboardWriter::default();
        struct Events(Vec<CaptureEvent>);
        impl EventSink for Events {
            fn emit(&mut self, event: CaptureEvent) {
                self.0.push(event);
            }
        }
        for text in ["synthetic generated conversion", "synthetic generated OCR"] {
            writer.write(ClipboardPayload::Text(text.into())).unwrap();
            let mut reader = crate::capture::NativeCaptureReader::default();
            let mut events = Events(Vec::new());
            assert!(matches!(
                CaptureEngine::capture(&context, &mut reader, &store, &mut events),
                CaptureOutcome::Stored(_)
            ));
            assert_eq!(
                events
                    .0
                    .iter()
                    .filter(|event| matches!(event, CaptureEvent::Stored { .. }))
                    .count(),
                1
            );
        }
        assert_eq!(
            history
                .query(&HistoryQuery {
                    limit: 10,
                    ..Default::default()
                })
                .unwrap()
                .len(),
            3
        );
        writer
            .write_suppressing_recapture(ClipboardPayload::Text("synthetic generated OCR".into()))
            .unwrap();
        assert!(matches!(
            CaptureEngine::capture(
                &context,
                &mut crate::capture::NativeCaptureReader::default(),
                &store,
                &mut ()
            ),
            CaptureOutcome::Skipped(CaptureSkip::OwnClipboard)
        ));
        assert_eq!(
            history
                .query(&HistoryQuery {
                    limit: 10,
                    ..Default::default()
                })
                .unwrap()
                .len(),
            3
        );
        let now = || {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64
        };
        use dzc_core::api::PrivacySessionCommand;
        history
            .privacy()
            .set_session(PrivacySessionCommand::On { duration_ms: None }, now())
            .unwrap();
        writer
            .write(ClipboardPayload::Text(
                "synthetic private generation".into(),
            ))
            .unwrap();
        let mut denied = TracedReader {
            native: Default::default(),
            body_reads: 0,
        };
        assert!(matches!(
            CaptureEngine::capture(&context, &mut denied, &store, &mut ()),
            CaptureOutcome::Skipped(CaptureSkip::PrivacySession)
        ));
        assert_eq!(denied.body_reads, 0);
        history
            .privacy()
            .set_session(PrivacySessionCommand::Off, now())
            .unwrap();
        writer
            .write(ClipboardPayload::Text("synthetic stale generation".into()))
            .unwrap();
        let frame = CaptureEngine::read_authorized(
            &context,
            &mut crate::capture::NativeCaptureReader::default(),
            &store,
            &mut (),
        )
        .unwrap();
        history
            .privacy()
            .set_session(PrivacySessionCommand::On { duration_ms: None }, now())
            .unwrap();
        history
            .privacy()
            .set_session(PrivacySessionCommand::Off, now())
            .unwrap();
        assert!(matches!(
            CaptureEngine::capture_frame(&context, frame, &store, &mut ()),
            CaptureOutcome::Skipped(CaptureSkip::PrivacyEpochChanged)
        ));
        assert_eq!(
            history
                .query(&HistoryQuery {
                    limit: 10,
                    ..Default::default()
                })
                .unwrap()
                .len(),
            3
        );
        writer.stop().unwrap();
        private_wayland_payloads_share_hints_and_revoke(&runtime);
    }

    #[cfg(target_os = "linux")]
    fn private_wayland_payloads_share_hints_and_revoke(runtime: &std::path::Path) {
        use dzc_core::api::{
            ClipboardEntry, HistoryService, PrivacyAccessIntent, PrivacySessionCommand,
            SensitiveAccess,
        };
        let history = HistoryService::open(&runtime.join("private-copy-history.db")).unwrap();
        let now = || chrono_time();
        fn chrono_time() -> i64 {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64
        }
        let authorize = || {
            let epoch = history.privacy().status(now()).unwrap().privacy_epoch;
            let permit = history
                .privacy()
                .authorize_read(
                    PrivacyAccessIntent::SelectedCopy,
                    SensitiveAccess::ExplicitUserAction,
                    Some(epoch),
                    now(),
                )
                .unwrap();
            let entry: ClipboardEntry = serde_json::from_value(serde_json::json!({
                "id":-42,"content_type":"text","content":"private synthetic copy", "preview":"private",
                "source_app":"Synthetic","timestamp":1,"is_pinned":false,"tags":["sensitive"]
            })).unwrap();
            match history
                .session_clipboard_payload_authorized(&entry, &permit, now())
                .unwrap()
            {
                ClipboardPayload::Private { authorization, .. } => authorization,
                _ => panic!("sensitive session copy must retain Core authorization"),
            }
        };
        let writer = EmbeddedClipboardWriter::default();
        let image = image::RgbaImage::from_raw(1, 1, vec![7, 8, 9, 255]).unwrap();
        let mut png = std::io::Cursor::new(Vec::new());
        image.write_to(&mut png, image::ImageFormat::Png).unwrap();
        for (payload, mime, expected) in [
            (
                ClipboardPayload::Text("private synthetic text".into()),
                "text/plain",
                b"private synthetic text".to_vec(),
            ),
            (
                ClipboardPayload::Html {
                    html: "<b>private synthetic HTML</b>".into(),
                    text: "private synthetic HTML".into(),
                },
                "text/html",
                b"<b>private synthetic HTML</b>".to_vec(),
            ),
            (
                ClipboardPayload::Image {
                    bytes: png.get_ref().clone(),
                    mime: "image/png".into(),
                },
                "image/png",
                png.into_inner(),
            ),
        ] {
            writer
                .write_suppressing_recapture(ClipboardPayload::Private {
                    payload: Box::new(payload),
                    authorization: authorize(),
                })
                .unwrap();
            let offer = crate::wayland::WaylandClipboardBackend::snapshot().unwrap();
            assert_eq!(offer.password_hint().unwrap(), Some(true));
            assert_eq!(
                offer
                    .receive(mime, crate::wayland::MAX_TRANSFER_BYTES)
                    .unwrap(),
                expected
            );
            assert_eq!(
                NativePrivacyHintProvider.read_privacy_hints().unwrap(),
                vec![PrivacyHint::Sensitive]
            );
            let mut reader = crate::capture::NativeCaptureReader::default();
            use dzc_core::capture::CaptureReader;
            assert_eq!(
                reader.preflight_privacy().unwrap().hints,
                vec![PrivacyHint::Sensitive]
            );
        }
        writer
            .write_suppressing_recapture(ClipboardPayload::ProtectedFiles {
                files: vec![ClipboardFile {
                    name: "synthetic.txt".into(),
                    bytes: b"private synthetic file".to_vec(),
                }],
                authorization: authorize(),
            })
            .unwrap();
        let offer = crate::wayland::WaylandClipboardBackend::snapshot().unwrap();
        assert_eq!(offer.password_hint().unwrap(), Some(true));
        let uris = String::from_utf8(offer.receive("text/uri-list", 4096).unwrap()).unwrap();
        let path = urlencoding::decode(uris.trim().strip_prefix("file://").unwrap())
            .unwrap()
            .into_owned();
        assert_eq!(std::fs::read(&path).unwrap(), b"private synthetic file");
        assert!(path.starts_with("/proc/"));
        assert!(!path.starts_with(&format!("/proc/{}/fd/", std::process::id())));
        revoke_private_exports().unwrap();
        assert!(!std::path::Path::new(&path).exists());
        assert!(NativeClipboard.formats().unwrap().is_empty());
        writer
            .write_suppressing_recapture(ClipboardPayload::ProtectedFiles {
                files: vec![ClipboardFile {
                    name: "synthetic.txt".into(),
                    bytes: b"different private file B".to_vec(),
                }],
                authorization: authorize(),
            })
            .unwrap();
        let replacement = crate::wayland::WaylandClipboardBackend::snapshot().unwrap();
        let uris = String::from_utf8(replacement.receive("text/uri-list", 4096).unwrap()).unwrap();
        let replacement_path = urlencoding::decode(uris.trim().strip_prefix("file://").unwrap())
            .unwrap()
            .into_owned();
        assert_ne!(path, replacement_path);
        assert_eq!(
            std::fs::read(&replacement_path).unwrap(),
            b"different private file B"
        );
        assert!(
            std::fs::read(&path).is_err(),
            "revoked A must never resolve to replacement B"
        );
        revoke_private_exports().unwrap();
        assert!(!std::path::Path::new(&replacement_path).exists());
        // Revocation releases private ownership without permanently disabling the Host writer.
        writer
            .write(ClipboardPayload::Text("ordinary synthetic remains".into()))
            .unwrap();
        revoke_private_exports().unwrap();
        assert_eq!(
            NativeClipboard.read_representation("text/plain").unwrap(),
            b"ordinary synthetic remains"
        );
        let authorization = authorize();
        writer
            .write(ClipboardPayload::Private {
                payload: Box::new(ClipboardPayload::Text("revoked synthetic body".into())),
                authorization: authorization.clone(),
            })
            .unwrap();
        let stale = crate::wayland::WaylandClipboardBackend::snapshot().unwrap();
        history
            .privacy()
            .set_session(PrivacySessionCommand::On { duration_ms: None }, now())
            .unwrap();
        // A sender that refuses supply closes the pipe: the receiver may observe empty EOF.
        if let Ok(bytes) = stale.receive("text/plain", 4096) {
            assert!(bytes.is_empty());
        }
        revoke_private_exports().unwrap();
        assert!(writer
            .write(ClipboardPayload::Private {
                payload: Box::new(ClipboardPayload::Text("must not become ordinary".into())),
                authorization
            })
            .is_err());
        writer.stop().unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn isolated_x11_private_owner_retains_files_and_releases_only_its_selection() {
        if std::env::var("DEZIRCLIP_RUN_LINUX_CLIPBOARD_SMOKE_TESTS").as_deref() != Ok("1") {
            return;
        }
        assert!(!crate::wayland::requested());
        let runtime = std::path::PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap());
        assert!(
            runtime.is_absolute() && runtime.join("dezirclip-isolated-wayland-fixture").is_file()
        );
        use dzc_core::api::{ClipboardEntry, HistoryService, PrivacyAccessIntent, SensitiveAccess};
        let history = HistoryService::open(&runtime.join("private-x11-history.db")).unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let permit = history
            .privacy()
            .authorize_read(
                PrivacyAccessIntent::SelectedCopy,
                SensitiveAccess::ExplicitUserAction,
                None,
                now,
            )
            .unwrap();
        let entry: ClipboardEntry = serde_json::from_value(serde_json::json!({
            "id":-42,"content_type":"text","content":"private synthetic X11", "preview":"private",
            "source_app":"Synthetic","timestamp":1,"is_pinned":false,"tags":["sensitive"]
        }))
        .unwrap();
        let payload = history
            .session_clipboard_payload_authorized(&entry, &permit, now)
            .unwrap();
        let authorization = match &payload {
            ClipboardPayload::Private { authorization, .. } => authorization.clone(),
            _ => panic!("protected copy required"),
        };
        let writer = EmbeddedClipboardWriter::default();
        writer.write_suppressing_recapture(payload).unwrap();
        assert_eq!(
            NativeClipboard.read_representation("text/plain").unwrap(),
            b"private synthetic X11"
        );
        assert_eq!(
            NativePrivacyHintProvider.read_privacy_hints().unwrap(),
            vec![PrivacyHint::Sensitive]
        );
        writer
            .write_suppressing_recapture(ClipboardPayload::ProtectedFiles {
                files: vec![ClipboardFile {
                    name: "synthetic.txt".into(),
                    bytes: b"private X11 file".to_vec(),
                }],
                authorization: authorization.clone(),
            })
            .unwrap();
        assert_eq!(
            NativePrivacyHintProvider.read_privacy_hints().unwrap(),
            vec![PrivacyHint::Sensitive]
        );
        let uris = String::from_utf8(
            NativeClipboard
                .read_representation("text/uri-list")
                .unwrap(),
        )
        .unwrap();
        let path = urlencoding::decode(uris.trim().strip_prefix("file://").unwrap())
            .unwrap()
            .into_owned();
        assert_eq!(std::fs::read(&path).unwrap(), b"private X11 file");
        revoke_private_exports().unwrap();
        assert!(!std::path::Path::new(&path).exists());
        writer
            .write_suppressing_recapture(ClipboardPayload::ProtectedFiles {
                files: vec![ClipboardFile {
                    name: "synthetic.txt".into(),
                    bytes: b"different private X11 file B".to_vec(),
                }],
                authorization,
            })
            .unwrap();
        let uris = String::from_utf8(
            NativeClipboard
                .read_representation("text/uri-list")
                .unwrap(),
        )
        .unwrap();
        let replacement_path = urlencoding::decode(uris.trim().strip_prefix("file://").unwrap())
            .unwrap()
            .into_owned();
        assert_ne!(path, replacement_path);
        assert_eq!(
            std::fs::read(&replacement_path).unwrap(),
            b"different private X11 file B"
        );
        assert!(
            std::fs::read(&path).is_err(),
            "revoked X11 A must never resolve to replacement B"
        );
        revoke_private_exports().unwrap();
        assert!(!std::path::Path::new(&replacement_path).exists());
        writer
            .write(ClipboardPayload::Text("ordinary X11 remains".into()))
            .unwrap();
        revoke_private_exports().unwrap();
        assert_eq!(
            NativeClipboard.read_representation("text/plain").unwrap(),
            b"ordinary X11 remains"
        );
        writer.stop().unwrap();
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
