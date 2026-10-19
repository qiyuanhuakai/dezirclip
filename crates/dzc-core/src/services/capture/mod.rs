//! Capture policy and storage orchestration without desktop or native API dependencies.
pub mod content;
mod discovery;
mod engine;
mod notifications;
pub mod source_exclusion;
mod store;
mod types;

pub use crate::database::thumbnail_hash;
pub use engine::CaptureEngine;
pub use notifications::{CaptureNotificationState, OwnClipboardCopy};
pub use store::{CaptureCommit, CaptureDuplicate, CaptureStore, PreparedCapture};
pub use types::*;

pub(crate) use store::sqlite_capture_store;
