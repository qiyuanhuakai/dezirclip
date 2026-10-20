use dzc_protocol::{CapabilityState, PlatformCapabilities, UnsupportedReason};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct SessionCapabilityReport {
    #[serde(flatten)]
    pub platform: PlatformCapabilities,
    /// Selects conservative Wayland behavior, including an unavailable native
    /// backend. Availability remains in each probed capability, not this flag.
    pub native_wayland: bool,
    #[cfg(target_os = "linux")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wayland: Option<WaylandRuntimeReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_interactions: Option<WindowInteractions>,
}
#[derive(Debug, Serialize)]
pub struct WindowInteractions {
    pub focus_restore: CapabilityState,
    pub always_on_top: CapabilityState,
    pub transparency: CapabilityState,
    pub screenshot: CapabilityState,
    pub edge_docking: CapabilityState,
}
#[cfg(target_os = "linux")]
#[derive(Debug, Serialize)]
pub struct WaylandRuntimeReport {
    pub experimental: bool,
    pub connection: Option<crate::wayland::WaylandStatus>,
    pub failure: Option<crate::wayland::WaylandError>,
    pub portals: Vec<PortalProbe>,
}
#[cfg(target_os = "linux")]
#[derive(Debug, Serialize)]
pub struct PortalProbe {
    pub interface: &'static str,
    pub version: Option<u32>,
    pub status: &'static str,
    pub permission: &'static str,
    pub adapter_attached: bool,
}
pub fn is_native_wayland() -> bool {
    #[cfg(target_os = "linux")]
    {
        crate::wayland::requested()
    }
    #[cfg(target_os = "windows")]
    {
        false
    }
}
/// No remote network, clipboard body or permission prompt is used by this query.
pub fn session_capabilities() -> SessionCapabilityReport {
    let native_wayland = is_native_wayland();
    let platform = super::standalone_capabilities();
    let window_interactions = native_wayland.then(|| {
        let missing = |backend: &str| {
            CapabilityState::unsupported(backend, UnsupportedReason::AdapterUnavailable)
                .with_probe("native_wayland_adapter_not_attached")
        };
        WindowInteractions {
            focus_restore: missing("wayland-focus-restore"),
            always_on_top: missing("wayland-always-on-top"),
            transparency: missing("wayland-window-transparency-unverified"),
            screenshot: missing("wayland-authorized-screenshot"),
            edge_docking: missing("wayland-global-window-position"),
        }
    });
    #[cfg(target_os = "linux")]
    let wayland = native_wayland.then(|| {
        let result = crate::wayland::WaylandClipboardBackend::probe();
        let (connection, failure) = match result {
            Ok(status) => (Some(status), None),
            Err(error) => (None, Some(error)),
        };
        WaylandRuntimeReport {
            experimental: true,
            connection,
            failure,
            portals: portal_probes(),
        }
    });
    SessionCapabilityReport {
        platform,
        native_wayland,
        #[cfg(target_os = "linux")]
        wayland,
        window_interactions,
    }
}

#[cfg(target_os = "linux")]
fn portal_probes() -> Vec<PortalProbe> {
    use dbus::blocking::{stdintf::org_freedesktop_dbus::Properties, Connection};
    use std::time::Duration;
    let interfaces = [
        "org.freedesktop.portal.GlobalShortcuts",
        "org.freedesktop.portal.RemoteDesktop",
        "org.freedesktop.portal.Screenshot",
    ];
    let missing = |status| {
        interfaces
            .into_iter()
            .map(|interface| PortalProbe {
                interface,
                version: None,
                status,
                permission: "not_requested",
                adapter_attached: false,
            })
            .collect()
    };
    if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
        return missing("session_bus_unavailable");
    }
    let Ok(connection) = Connection::new_session() else {
        return missing("session_bus_unavailable");
    };
    let bus = connection.with_proxy(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        Duration::from_millis(250),
    );
    // Resolve only an existing unique owner. Property queries must not activate
    // xdg-desktop-portal or create any authorization session while probing.
    let owner: Result<(String,), _> = bus.method_call(
        "org.freedesktop.DBus",
        "GetNameOwner",
        ("org.freedesktop.portal.Desktop",),
    );
    let Ok((owner,)) = owner else {
        return missing("portal_not_running");
    };
    let proxy = connection.with_proxy(
        owner,
        "/org/freedesktop/portal/desktop",
        Duration::from_millis(250),
    );
    interfaces
        .into_iter()
        .map(|interface| {
            let version = proxy
                .get::<u32>(interface, "version")
                .ok()
                .filter(|version| *version > 0);
            PortalProbe {
                interface,
                version,
                status: if version.is_some() {
                    "interface_present"
                } else {
                    "interface_unavailable"
                },
                permission: "not_requested",
                adapter_attached: false,
            }
        })
        .collect()
}
