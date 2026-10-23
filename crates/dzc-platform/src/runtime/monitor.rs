use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
#[derive(Default)]
pub struct NativeClipboardMonitor {
    active: AtomicBool,
}
impl ClipboardMonitor for NativeClipboardMonitor {
    fn capability(&self) -> CapabilityState {
        let active = self.active.load(Ordering::Acquire);
        #[cfg(target_os = "linux")]
        let active = if crate::wayland::requested() {
            crate::wayland::WaylandClipboardBackend::monitor_active()
        } else {
            active
        };
        let mut state = match crate::capture::probe_clipboard_monitor() {
            Ok(backend) => {
                CapabilityState::available(backend, active).with_probe("native_monitor_setup")
            }
            Err(error) => {
                let reason = if error == "ClipboardMonitorDisabled" {
                    UnsupportedReason::DisabledByConfiguration
                } else {
                    UnsupportedReason::BackendUnavailable
                };
                CapabilityState::unsupported("native-clipboard-monitor", reason)
                    .with_probe("native_monitor_setup")
            }
        };
        #[cfg(target_os = "linux")]
        if !crate::wayland::requested() {
            if let Err(error) = crate::runtime_display::validate_x11_display() {
                let reason = match error {
                    crate::runtime_display::DisplayUnavailable::NotSet
                        if std::env::var_os("WAYLAND_DISPLAY").is_some() =>
                    {
                        UnsupportedReason::NativeWaylandUnavailable
                    }
                    crate::runtime_display::DisplayUnavailable::NotSet => {
                        UnsupportedReason::NoGraphicalSession
                    }
                    crate::runtime_display::DisplayUnavailable::InvalidDisplay => {
                        UnsupportedReason::ConnectionUnavailable
                    }
                };
                state = CapabilityState::unsupported("x11", reason)
                    .with_probe("x11_display_validation");
            }
        }
        state.active = active;
        state
    }
    fn run(&self, changed: Box<dyn FnMut(ClipboardChanged) + Send>) -> PlatformResult<()> {
        if self
            .active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(PlatformError::failed(Capability::Monitor));
        }
        struct Reset<'a>(&'a AtomicBool);
        impl Drop for Reset<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Release);
            }
        }
        let _reset = Reset(&self.active);
        let changed = Arc::new(Mutex::new(changed));
        crate::capture::run_clipboard_monitor(Arc::new(move |change| {
            if let Ok(mut callback) = changed.lock() {
                callback(ClipboardChanged {
                    sequence: change.sequence as u64,
                });
            }
        }))
        .map_err(|_| match self.capability().availability {
            dzc_protocol::Availability::Unsupported { reason } => {
                PlatformError::unsupported(Capability::Monitor, reason)
            }
            dzc_protocol::Availability::Available => PlatformError::failed(Capability::Monitor),
        })
    }
}
