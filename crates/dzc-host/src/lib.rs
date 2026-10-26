//! One headless Host implementation for embedded and optional service clients.
mod authorization;
mod events;
mod router;
mod server;
mod capture;
mod client;
mod work;
mod desktop;
mod desktop_actions;
mod desktop_policy;
mod hotkey;
mod upload;
mod ocr;
mod surface;
pub use ocr::schedule_capture_ocr;
pub mod data;
pub mod controller;
pub use client::{Client, ControlFile};
pub use capture::{CaptureManager, CaptureHooks};
pub use work::CaptureWork;
pub use server::{serve, serve_listener};
pub use authorization::{Grant, GrantAuthority, Session};
pub use router::{Host, HostActions, HostOptions, HostLifecycle};
pub use dzc_protocol::wire;

/// Inventory for an executable that actually routes through this Host library.
pub fn build_info(component: dzc_protocol::build_info::Component, schema: i32) -> dzc_protocol::build_info::BuildInfo {
    dzc_protocol::build_info::BuildInfo::current_with_host(component, schema)
}
