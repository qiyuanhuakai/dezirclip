//! One headless Host implementation for embedded and optional service clients.
mod authorization;
mod events;
#[cfg(test)]
mod fixtures;
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub mod external;
mod router;
mod server;
#[cfg(any(target_os = "linux", target_os = "windows"))]
mod capture;
mod client;
mod work;
mod desktop;
mod library;
#[cfg(any(target_os = "linux", target_os = "windows"))]
mod paste_queue;
#[cfg(any(target_os = "linux", target_os = "windows"))]
mod quick;
mod backup;
mod tools;
mod themes;
mod desktop_actions;
#[cfg(any(target_os = "linux", target_os = "windows"))]
mod file_transfer;
mod desktop_policy;
#[cfg(any(target_os = "linux", target_os = "windows"))]
mod hotkey;
#[cfg(any(target_os = "linux", target_os = "windows"))]
mod recording;
#[cfg(any(target_os = "linux", target_os = "windows"))]
mod system_settings;
mod upload;
#[cfg(any(target_os = "linux", target_os = "windows"))]
mod ocr;
mod surface;
#[cfg(all(feature = "native-screen", any(target_os = "linux", target_os = "windows")))]
mod screen;
mod history_version;
pub mod update;
pub use surface::same_surface_state;
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub use ocr::{engine_info, prepare_manual_ocr, schedule_capture_ocr, OCR_ENGINE_DISPLAY_NAME};
pub mod data;
#[cfg(target_os = "macos")]
pub mod darwin_profile;
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub mod controller;
pub use client::{Client, ControlFile};
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub use capture::{CaptureManager, CaptureHooks};
pub use work::CaptureWork;
pub use server::{serve, serve_listener};
pub use authorization::{Grant, GrantAuthority, Session};
pub use router::{Host, HostActions, HostOptions, HostLifecycle, PreparedPaste};
pub use dzc_protocol::wire;

/// Inventory for an executable that actually routes through this Host library.
pub fn build_info(component: dzc_protocol::build_info::Component, schema: i32) -> dzc_protocol::build_info::BuildInfo {
    dzc_protocol::build_info::BuildInfo::current_with_host(component, schema)
}
