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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    fn history(protected: bool, kind: &str) -> HistoryService {
        let connection = crate::database::init_db(":memory:").unwrap();
        connection.execute("INSERT INTO clipboard_history(id,content_type,content,preview,tags,source_app,timestamp,is_pinned) VALUES(1,?1,?2,'synthetic preview',?3,'Synthetic',1,0)",
            rusqlite::params![kind,"synthetic queue body",if protected {"[\"sensitive\"]"} else {"[]"}]).unwrap();
        HistoryService::from_connection(Arc::new(Mutex::new(connection)))
    }

    #[test]
    fn queue_cannot_decrypt_protected_text_html_images_or_files_without_a_user_read_grant() {
        for kind in ["text", "rich_text", "image", "files", "video"] {
            let history = history(true, kind);
            assert_eq!(
                history.get_entry_content(1).unwrap().as_deref(),
                Some("synthetic queue body")
            );
            assert_eq!(
                history.prepare_queue_entry(1, None, 10).unwrap_err(),
                PrivacyError::SensitiveAccessDenied,
                "{kind}"
            );
        }
    }

    #[test]
    fn ordinary_queue_reads_keep_the_original_epoch_and_work_while_recording_is_paused() {
        let history = history(false, "text");
        let before = history.prepare_queue_entry(1, None, 10).unwrap().unwrap();
        history.privacy().toggle(11).unwrap();
        assert_eq!(
            history
                .privacy()
                .check_read(&before.permit, 12)
                .unwrap_err(),
            PrivacyError::PrivacyEpochChanged
        );
        let active = history.prepare_queue_entry(1, None, 12).unwrap().unwrap();
        assert_eq!(active.entry.content, "synthetic queue body");
        assert!(history.privacy().check_read(&active.permit, 12).is_ok());
        assert!(history.privacy().capture_ticket(12).is_err());
        assert!(history.prepare_queue_entry(2, None, 12).unwrap().is_none());
        assert!(history.prepare_queue_entry(-1, None, 12).is_err());
    }
}
