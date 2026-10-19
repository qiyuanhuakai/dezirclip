//! Small native ports. Implementations report the capability of their current runtime.
use dzc_protocol::{Capability, CapabilityState, PlatformError, UnsupportedReason};

pub type PlatformResult<T> = Result<T, PlatformError>;

#[derive(Debug)]
pub enum ClipboardPayload {
    Text(String),
    Html { html: String, text: String },
    Image { bytes: Vec<u8>, mime: String },
    Files(Vec<String>),
}

pub trait ClipboardReader {
    fn capability(&self) -> CapabilityState;
    fn formats(&self) -> PlatformResult<Vec<String>>;
    fn read_representation(&self, mime: &str) -> PlatformResult<Vec<u8>>;
}

pub trait ClipboardWriter {
    fn capability(&self) -> CapabilityState;
    fn write(&self, payload: ClipboardPayload) -> PlatformResult<()>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipboardChanged {
    pub sequence: u64,
}

pub trait ClipboardMonitor {
    fn capability(&self) -> CapabilityState;
    /// Run on the caller's existing worker. This embedded port does not promise cancellation.
    fn run(&self, changed: Box<dyn FnMut(ClipboardChanged) + Send>) -> PlatformResult<()>;
}

pub trait PasteController {
    fn capability(&self) -> CapabilityState;
    /// Paste is a separate explicit action; clipboard writes never invoke it implicitly.
    fn paste(&self) -> PlatformResult<()>;
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SourceResolution {
    ClipboardOwner,
    ActiveWindow,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceIdentity {
    pub resolution: SourceResolution,
    pub app_name: Option<String>,
    pub process_path: Option<String>,
}

pub trait SourceResolver {
    fn capability(&self) -> CapabilityState;
    fn resolve_source(&self) -> PlatformResult<SourceIdentity>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcut {
    pub id: String,
    pub accelerator: String,
}

pub trait ShortcutRegistration: Send {
    fn unregister(&mut self) -> PlatformResult<()>;
}

pub trait ShortcutProvider {
    fn capability(&self) -> CapabilityState;
    fn register(
        &self,
        shortcut: &Shortcut,
        pressed: Box<dyn FnMut() + Send>,
    ) -> PlatformResult<Box<dyn ShortcutRegistration>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivacyHint {
    ExcludeFromHistory,
    Sensitive,
}

pub trait PrivacyHintProvider {
    fn capability(&self) -> CapabilityState;
    /// An empty successful result means the adapter checked and found no explicit hint.
    fn read_privacy_hints(&self) -> PlatformResult<Vec<PrivacyHint>>;
}

/// Adapter states are queried in this process; availability never implies a worker is active.
#[derive(Default)]
pub struct PlatformPorts<'a> {
    pub reader: Option<&'a dyn ClipboardReader>,
    pub writer: Option<&'a dyn ClipboardWriter>,
    pub monitor: Option<&'a dyn ClipboardMonitor>,
    pub paste: Option<&'a dyn PasteController>,
    pub source: Option<&'a dyn SourceResolver>,
    pub shortcuts: Option<&'a dyn ShortcutProvider>,
    pub privacy_hints: Option<&'a dyn PrivacyHintProvider>,
}

impl PlatformPorts<'_> {
    pub fn capabilities(&self) -> dzc_protocol::PlatformCapabilities {
        let mut report = dzc_protocol::PlatformCapabilities::unavailable(
            "unattached",
            UnsupportedReason::AdapterUnavailable,
        );
        if let Some(port) = self.reader {
            report.read = port.capability();
        }
        if let Some(port) = self.writer {
            report.write = port.capability();
        }
        if let Some(port) = self.monitor {
            report.monitor = port.capability();
        }
        if let Some(port) = self.paste {
            report.auto_paste = port.capability();
        }
        if let Some(port) = self.source {
            report.resolve_source = port.capability();
        }
        if let Some(port) = self.shortcuts {
            report.global_shortcut = port.capability();
        }
        if let Some(port) = self.privacy_hints {
            report.read_privacy_hints = port.capability();
        }
        report
    }
}

/// A missing adapter is explicit and cannot be mistaken for an absent privacy signal.
pub struct UnavailablePort {
    pub backend: &'static str,
    pub reason: UnsupportedReason,
}

impl UnavailablePort {
    fn state(&self) -> CapabilityState {
        CapabilityState::unsupported(self.backend, self.reason)
    }
    fn error(&self, operation: Capability) -> PlatformError {
        PlatformError::unsupported(operation, self.reason)
    }
}

impl ClipboardReader for UnavailablePort {
    fn capability(&self) -> CapabilityState {
        self.state()
    }
    fn formats(&self) -> PlatformResult<Vec<String>> {
        Err(self.error(Capability::Read))
    }
    fn read_representation(&self, _: &str) -> PlatformResult<Vec<u8>> {
        Err(self.error(Capability::Read))
    }
}
impl ClipboardWriter for UnavailablePort {
    fn capability(&self) -> CapabilityState {
        self.state()
    }
    fn write(&self, _: ClipboardPayload) -> PlatformResult<()> {
        Err(self.error(Capability::Write))
    }
}
impl ClipboardMonitor for UnavailablePort {
    fn capability(&self) -> CapabilityState {
        self.state()
    }
    fn run(&self, _: Box<dyn FnMut(ClipboardChanged) + Send>) -> PlatformResult<()> {
        Err(self.error(Capability::Monitor))
    }
}
impl PasteController for UnavailablePort {
    fn capability(&self) -> CapabilityState {
        self.state()
    }
    fn paste(&self) -> PlatformResult<()> {
        Err(self.error(Capability::AutoPaste))
    }
}
impl SourceResolver for UnavailablePort {
    fn capability(&self) -> CapabilityState {
        self.state()
    }
    fn resolve_source(&self) -> PlatformResult<SourceIdentity> {
        Err(self.error(Capability::ResolveSource))
    }
}
impl ShortcutProvider for UnavailablePort {
    fn capability(&self) -> CapabilityState {
        self.state()
    }
    fn register(
        &self,
        _: &Shortcut,
        _: Box<dyn FnMut() + Send>,
    ) -> PlatformResult<Box<dyn ShortcutRegistration>> {
        Err(self.error(Capability::GlobalShortcut))
    }
}
impl PrivacyHintProvider for UnavailablePort {
    fn capability(&self) -> CapabilityState {
        self.state()
    }
    fn read_privacy_hints(&self) -> PlatformResult<Vec<PrivacyHint>> {
        Err(self.error(Capability::ReadPrivacyHints))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_privacy_adapter_is_not_a_negative_privacy_evidence() {
        let missing = UnavailablePort {
            backend: "embedded",
            reason: UnsupportedReason::AdapterUnavailable,
        };
        let error = missing.read_privacy_hints().unwrap_err();
        assert_eq!(error.operation, Capability::ReadPrivacyHints);
        assert_eq!(error.reason, Some(UnsupportedReason::AdapterUnavailable));
        assert!(!PrivacyHintProvider::capability(&missing).is_available());
    }
}
