use super::{CoreResult, HistoryService};
use crate::capture::CaptureSettings;

impl HistoryService {
    /// A fixed read-only capture policy snapshot, never arbitrary settings IPC.
    pub fn capture_settings_snapshot(&self) -> CoreResult<CaptureSettings> {
        self.repository.capture_settings_snapshot()
    }
}
