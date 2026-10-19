//! Business entry points shared by desktop, terminal and automation clients.
mod backup;
mod clipboard;
mod error;
mod history;

pub use crate::domain::models::ClipboardEntry;
pub use crate::privacy::{PrivacyAction, PrivacyDecision, PrivacyEvidence, PrivacyPolicy};
pub use backup::{BackupService, ExportEntry, ImportMode};
pub use clipboard::ClipboardService;
pub use error::{CoreError, CoreErrorCode, CoreResult};
pub use history::{AutomationHistory, HistoryQuery, HistorySearchMode, HistoryService};

pub use crate::infrastructure::repository::{
    sensitive_tags::SensitiveTagDeletion as TagDeletionReport, tag_batch::TagEntrySelection,
    tag_page::TagItemsPage,
};

pub use crate::capture::{CaptureEngine, CaptureStore, EventSink};

mod capture_store;
