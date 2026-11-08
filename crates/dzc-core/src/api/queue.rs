use super::{
    ClipboardEntry, HistoryQuery, HistoryService, PrivacyError, PrivacyReadPermit, SensitiveAccess,
};

#[derive(Debug)]
pub struct PreparedQueueEntry {
    pub entry: ClipboardEntry,
    pub permit: PrivacyReadPermit,
}

impl HistoryService {
    pub fn prepare_queue_entry(
        &self,
        id: i64,
        epoch: Option<u64>,
        now: i64,
    ) -> Result<Option<PreparedQueueEntry>, PrivacyError> {
        if id <= 0 {
            return Err(PrivacyError::SensitiveAccessDenied);
        }
        let permit = self.privacy().authorize_selected_copy(
            id,
            None,
            SensitiveAccess::Denied,
            epoch,
            now,
        )?;
        let entry = self.get_entry_authorized(id, &permit, now)?;
        if entry.is_none() {
            let metadata = self
                .query(&HistoryQuery {
                    id: Some(id),
                    limit: 1,
                    metadata: true,
                    ..Default::default()
                })
                .map_err(|_| PrivacyError::PrivacyStateUnavailable)?;
            self.privacy().check_read(&permit, now)?;
            if !metadata.is_empty() {
                return Err(PrivacyError::SensitiveAccessDenied);
            }
        }
        Ok(entry.map(|entry| PreparedQueueEntry { entry, permit }))
    }
}
