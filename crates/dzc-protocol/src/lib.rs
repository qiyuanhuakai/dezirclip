//! Stable value types and the optional, schema-validated local wire protocol.
pub mod build_info;
pub mod history;
#[cfg(feature = "wire")]
pub mod wire;
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion { major: 1, minor: 0 };
pub const CORE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolVersion {
    pub major: u16,
    pub minor: u16,
}

impl ProtocolVersion {
    pub const fn is_compatible_with(self, peer: Self) -> bool {
        self.major == peer.major
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Read,
    Write,
    Monitor,
    AutoPaste,
    GlobalShortcut,
    ResolveSource,
    ReadPrivacyHints,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnsupportedReason {
    AdapterUnavailable,
    DisabledByConfiguration,
    BackendUnavailable,
    NoGraphicalSession,
    ConnectionUnavailable,
    PermissionDenied,
    NativeWaylandUnavailable,
    ExtensionUnavailable,
    UnsupportedRepresentation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Availability {
    Available,
    Unsupported { reason: UnsupportedReason },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityState {
    pub availability: Availability,
    /// The adapter or native protocol that was probed, never a desktop-name guess.
    pub backend: String,
    pub probe: String,
    /// True only when a worker or registration is currently running in this client.
    pub active: bool,
}

impl CapabilityState {
    pub fn available(backend: impl Into<String>, active: bool) -> Self {
        Self {
            availability: Availability::Available,
            backend: backend.into(),
            probe: "adapter_runtime".to_owned(),
            active,
        }
    }
    pub fn unsupported(backend: impl Into<String>, reason: UnsupportedReason) -> Self {
        Self {
            availability: Availability::Unsupported { reason },
            backend: backend.into(),
            probe: "adapter_lookup".to_owned(),
            active: false,
        }
    }
    pub fn with_probe(mut self, probe: &str) -> Self {
        self.probe = probe.to_owned();
        self
    }
    pub fn is_available(&self) -> bool {
        self.availability == Availability::Available
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformCapabilities {
    pub protocol: ProtocolVersion,
    pub core_version: String,
    /// Capabilities belong to this directly linked client, not another running process.
    pub scope: String,
    pub read: CapabilityState,
    pub write: CapabilityState,
    pub monitor: CapabilityState,
    pub auto_paste: CapabilityState,
    pub global_shortcut: CapabilityState,
    pub resolve_source: CapabilityState,
    pub read_privacy_hints: CapabilityState,
}

impl PlatformCapabilities {
    pub fn unavailable(backend: &str, reason: UnsupportedReason) -> Self {
        let state = || CapabilityState::unsupported(backend, reason);
        Self {
            protocol: PROTOCOL_VERSION,
            core_version: CORE_VERSION.to_owned(),
            scope: "embedded_client".to_owned(),
            read: state(),
            write: state(),
            monitor: state(),
            auto_paste: state(),
            global_shortcut: state(),
            resolve_source: state(),
            read_privacy_hints: state(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformErrorCode {
    Unsupported,
    TemporarilyUnavailable,
    OperationFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformError {
    pub operation: Capability,
    pub code: PlatformErrorCode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<UnsupportedReason>,
}

impl PlatformError {
    pub fn unsupported(operation: Capability, reason: UnsupportedReason) -> Self {
        Self {
            operation,
            code: PlatformErrorCode::Unsupported,
            reason: Some(reason),
        }
    }
    pub fn failed(operation: Capability) -> Self {
        Self {
            operation,
            code: PlatformErrorCode::OperationFailed,
            reason: None,
        }
    }
}

impl std::fmt::Display for PlatformError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {:?}", self.operation, self.code)?;
        if let Some(reason) = self.reason {
            write!(f, " ({reason:?})")?;
        }
        Ok(())
    }
}
impl std::error::Error for PlatformError {}
impl From<PlatformError> for String {
    fn from(error: PlatformError) -> Self {
        error.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capabilities_keep_availability_and_running_state_separate() {
        let mut report =
            PlatformCapabilities::unavailable("none", UnsupportedReason::AdapterUnavailable);
        report.read = CapabilityState::available("connected-x11", false);
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["scope"], "embedded_client");
        assert_eq!(json["read"]["availability"]["status"], "available");
        assert_eq!(json["read"]["active"], false);
        assert_eq!(
            json["monitor"]["availability"]["reason"],
            "adapter_unavailable"
        );
        assert_eq!(
            serde_json::from_value::<PlatformCapabilities>(json).unwrap(),
            report
        );
    }
    #[test]
    fn version_compatibility_never_accepts_another_major() {
        assert!(PROTOCOL_VERSION.is_compatible_with(ProtocolVersion { major: 1, minor: 3 }));
        assert!(!PROTOCOL_VERSION.is_compatible_with(ProtocolVersion { major: 2, minor: 0 }));
    }
}
