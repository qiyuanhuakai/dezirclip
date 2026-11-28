//! Query and project the embedded desktop's history, including its RAM entries.
use super::{ClipboardEntry, CoreError, HistoryService, PrivacyError, SessionHistoryStore};

pub enum DesktopHistoryQuery<'a> {
    Page { offset: i32, kind: Option<&'a str> },
    Search { term: &'a str },
}

#[derive(Debug)]
pub enum DesktopHistoryError {
    Storage(CoreError),
    Privacy(PrivacyError),
    SessionUnavailable,
}

impl HistoryService {
    pub fn embedded_history_page(
        &self,
        session: &SessionHistoryStore,
        query: DesktopHistoryQuery<'_>,
        limit: i32,
    ) -> Result<Vec<ClipboardEntry>, DesktopHistoryError> {
        let limit = limit.clamp(1, 150);
        let (rows, include_session, kind, term) = match query {
            DesktopHistoryQuery::Page { offset, kind } => (
                self.get_history(limit, offset, kind),
                offset == 0,
                kind,
                None,
            ),
            DesktopHistoryQuery::Search { term } => (
                self.search(term, limit),
                true,
                None,
                Some(term.to_lowercase()),
            ),
        };
        let mut rows = rows.map_err(DesktopHistoryError::Storage)?;
        if include_session {
            let session = session
                .lock()
                .map_err(|_| DesktopHistoryError::SessionUnavailable)?;
            for entry in session.iter().rev() {
                if kind.is_some_and(|kind| kind != entry.content_type)
                    || term.as_ref().is_some_and(|term| {
                        !entry.content.to_lowercase().contains(term)
                            && !entry.source_app.to_lowercase().contains(term)
                            && !entry
                                .tags
                                .iter()
                                .any(|tag| tag.to_lowercase().contains(term))
                    })
                {
                    continue;
                }
                if entry.id == 0 || !rows.iter().any(|row| row.id == entry.id) {
                    rows.push(entry.clone());
                }
            }
        }
        // Embedded paging counts persistent rows in the offset. Its ordinary
        // page puts pins first, while search keeps chronological ordering.
        if term.is_none() {
            rows.sort_by(|a, b| {
                b.is_pinned
                    .cmp(&a.is_pinned)
                    .then_with(|| b.pinned_order.cmp(&a.pinned_order))
                    .then_with(|| b.timestamp.cmp(&a.timestamp))
                    .then_with(|| b.id.cmp(&a.id))
            });
        } else {
            rows.sort_by(|a, b| b.timestamp.cmp(&a.timestamp).then_with(|| b.id.cmp(&a.id)));
        }
        rows.truncate(limit as usize);
        self.entries_for_gui_list(
            &mut rows,
            term.is_none(),
            chrono::Utc::now().timestamp_millis(),
        )
        .map_err(DesktopHistoryError::Privacy)?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::VecDeque, path::Path};

    fn entry(id: i64, text: &str, timestamp: i64) -> ClipboardEntry {
        serde_json::from_value(serde_json::json!({
            "id": id, "content_type": "text", "content": text, "source_app": "fixture",
            "timestamp": timestamp, "preview": text, "is_pinned": false,
        }))
        .unwrap()
    }

    #[test]
    fn embedded_pages_keep_ram_on_the_first_page_and_search_is_chronological() {
        let history = HistoryService::open_desktop(Path::new(":memory:")).unwrap();
        history
            .set_desktop_setting("app.privacy_protection", "false")
            .unwrap();
        let mut pinned = entry(0, "fixture needle old pin", 1);
        pinned.is_pinned = true;
        pinned.pinned_order = 3;
        let pinned_id = history.save(&pinned, None).unwrap();
        let saved = history
            .save(&entry(0, "fixture needle durable", 2), None)
            .unwrap();
        let ram = entry(-1, "fixture needle ram", 3);
        let mut protected = entry(-2, "fixture needle concealed", 4);
        protected.tags = vec!["sensitive".into()];
        let mut other = entry(-3, "fixture unrelated kind", 5);
        other.content_type = "file".into();
        let session = SessionHistoryStore::new(VecDeque::from([ram, protected, other]));
        let page = |offset| DesktopHistoryQuery::Page {
            offset,
            kind: Some("text"),
        };
        let ids = |rows: &[ClipboardEntry]| rows.iter().map(|row| row.id).collect::<Vec<_>>();
        let first = history
            .embedded_history_page(&session, page(0), 10)
            .unwrap();
        assert_eq!(ids(&first), [pinned_id, -2, -1, saved]);
        assert!(first[1].content.is_empty() && first[1].preview.is_empty());
        assert_eq!(
            ids(&history
                .embedded_history_page(&session, page(1), 10)
                .unwrap()),
            [saved]
        );
        assert_eq!(
            ids(&history.embedded_history_page(&session, page(0), 0).unwrap()),
            [pinned_id]
        );
        let found = history
            .embedded_history_page(&session, DesktopHistoryQuery::Search { term: "NEEDLE" }, 10)
            .unwrap();
        assert_eq!(ids(&found), [-1, saved, pinned_id]);
        assert_eq!(session.revision().unwrap(), 0);
    }
}
