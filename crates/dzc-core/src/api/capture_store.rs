use super::HistoryService;
use crate::capture::CaptureStore;
use std::path::Path;
impl HistoryService {
    /// Capture and history share the same repository, privacy transaction, and cache budgets.
    pub fn capture_store<'a>(&'a self, directory: &'a Path) -> impl CaptureStore + 'a {
        self.repository.capture_store(directory)
    }
}
