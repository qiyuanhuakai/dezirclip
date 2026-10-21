use super::*;

pub fn clipboard_probe() -> CapabilityState {
    if !crate::wayland::requested() {
        return super::x11::clipboard_probe();
    }
    match crate::wayland::WaylandClipboardBackend::probe() {
        Ok(status) => CapabilityState::available(
            match status.protocol {
                crate::wayland::DataControlProtocol::Ext => "wayland-ext-data-control-v1",
                crate::wayland::DataControlProtocol::Wlr => "wayland-wlr-data-control",
            },
            false,
        ),
        Err(error) => CapabilityState::unsupported(
            "native-wayland",
            match error {
                crate::wayland::WaylandError::PermissionDenied => {
                    UnsupportedReason::PermissionDenied
                }
                crate::wayland::WaylandError::DataControlUnavailable => {
                    UnsupportedReason::ExtensionUnavailable
                }
                crate::wayland::WaylandError::DisabledByConfiguration => {
                    UnsupportedReason::DisabledByConfiguration
                }
                _ => UnsupportedReason::ConnectionUnavailable,
            },
        ),
    }
    .with_probe("wayland_data_device_roundtrip")
}
pub fn source_probe() -> CapabilityState {
    if !crate::wayland::requested() {
        return super::x11::source_probe();
    }
    CapabilityState::unsupported(
        "wayland-data-control-no-source-identity",
        UnsupportedReason::AdapterUnavailable,
    )
    .with_probe("data_control_protocol_contract")
}
pub fn source() -> PlatformResult<SourceIdentity> {
    if !crate::wayland::requested() {
        return super::x11::source();
    }
    Err(PlatformError::unsupported(
        Capability::ResolveSource,
        UnsupportedReason::AdapterUnavailable,
    ))
}
pub fn paste_probe() -> CapabilityState {
    if !crate::wayland::requested() {
        return super::x11::paste_probe();
    }
    CapabilityState::unsupported(
        "wayland-authorized-paste-not-attached",
        UnsupportedReason::AdapterUnavailable,
    )
    .with_probe("no_authorized_portal_session")
}
pub fn paste() -> PlatformResult<()> {
    if !crate::wayland::requested() {
        return super::x11::paste();
    }
    Err(PlatformError::unsupported(
        Capability::AutoPaste,
        UnsupportedReason::AdapterUnavailable,
    ))
}
