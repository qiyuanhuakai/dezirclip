use super::{CoreResult, HistoryService};
use crate::capture::CaptureSettings;

impl HistoryService {
    /// Existing desktop accelerators reserved by this profile. This fixed list
    /// is used by the trusted Host before registering the privacy shortcut.
    pub fn reserved_shortcuts(&self) -> CoreResult<Vec<String>> {
        self.repository.reserved_shortcuts()
    }
    /// A fixed read-only capture policy snapshot, never arbitrary settings IPC.
    pub fn capture_settings_snapshot(&self) -> CoreResult<CaptureSettings> {
        self.repository.capture_settings_snapshot()
    }
}
