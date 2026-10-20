//! Selected copy translates authorized records inside Core; native clients never decrypt records.
use super::{ClipboardEntry, HistoryService, PrivacyError, PrivacyReadPermit, PrivacyResult};
use crate::ports::ClipboardPayload;
use base64::Engine;
use std::io::Read;
const MAX_IMAGE_BYTES: usize = 16 * 1024 * 1024;

fn payload(entry: ClipboardEntry) -> PrivacyResult<ClipboardPayload> {
    let unavailable = || PrivacyError::SensitiveAccessDenied;
    Ok(match entry.content_type.as_str() {
        "image" => {
            let bytes = if entry.content.starts_with("data:image/") {
                if entry.content.len() > MAX_IMAGE_BYTES * 4 / 3 + 1024 {
                    return Err(unavailable());
                }
                let (header, data) = entry.content.split_once(',').ok_or_else(unavailable)?;
                if !header.ends_with(";base64") {
                    return Err(unavailable());
                }
                base64::engine::general_purpose::STANDARD
                    .decode(data)
                    .map_err(|_| unavailable())?
            } else {
                let path = crate::infrastructure::attachments::path_from_uri(&entry.content)
                    .map_err(|_| unavailable())?;
                let file = crate::infrastructure::local_file::open_regular_nofollow(
                    std::path::Path::new(&path),
                )
                .map_err(|_| unavailable())?;
                let mut bytes = Vec::new();
                file.take(MAX_IMAGE_BYTES as u64 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| unavailable())?;
                bytes
            };
            if bytes.len() > MAX_IMAGE_BYTES {
                return Err(unavailable());
            }
            let format = image::guess_format(&bytes).map_err(|_| unavailable())?;
            ClipboardPayload::Image {
                bytes,
                mime: format.to_mime_type().to_owned(),
            }
        }
        "file" | "files" | "video" => {
            let values = if entry.content.trim_start().starts_with('[') {
                serde_json::from_str::<Vec<String>>(&entry.content).map_err(|_| unavailable())?
            } else {
                entry.content.lines().map(str::to_owned).collect()
            };
            if values.len() > 256 {
                return Err(unavailable());
            }
            let mut paths = Vec::new();
            for value in values
                .into_iter()
                .filter(|v| !v.is_empty() && !v.starts_with('#'))
            {
                paths.push(
                    crate::infrastructure::attachments::path_from_uri(&value)
                        .map_err(|_| unavailable())?,
                );
            }
            if paths.is_empty() {
                return Err(unavailable());
            }
            ClipboardPayload::Files(paths)
        }
        "html" => ClipboardPayload::Html {
            html: entry.html_content.unwrap_or_else(|| entry.content.clone()),
            text: entry.content,
        },
        _ => match entry.html_content {
            Some(html) => ClipboardPayload::Html {
                html,
                text: entry.content,
            },
            None => ClipboardPayload::Text(entry.content),
        },
    })
}

impl HistoryService {
    pub fn clipboard_payload_authorized(
        &self,
        id: i64,
        permit: &PrivacyReadPermit,
        now_ms: i64,
    ) -> PrivacyResult<ClipboardPayload> {
        permit.require_copy()?;
        let entry = self
            .get_entry_authorized(id, permit, now_ms)?
            .ok_or(PrivacyError::SensitiveAccessDenied)?;
        let result = payload(entry)?;
        self.privacy().check_read(permit, now_ms)?;
        Ok(result)
    }
    pub fn session_clipboard_payload_authorized(
        &self,
        entry: &ClipboardEntry,
        permit: &PrivacyReadPermit,
        now_ms: i64,
    ) -> PrivacyResult<ClipboardPayload> {
        permit.require_copy()?;
        let result = payload(self.authorize_session_entry(entry, permit, now_ms)?)?;
        self.privacy().check_read(permit, now_ms)?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{
        PrivacyAccessIntent, PrivacySessionCommand, SearchRequest, SensitiveAccess, SummaryQuery,
        SummarySearchMode,
    };
    fn entry(id: i64, content: &str) -> ClipboardEntry {
        serde_json::from_value(serde_json::json!({"id":id,"content_type":"text","content":content,"preview":content,"source_app":"Synthetic","timestamp":1,"is_pinned":false})).unwrap()
    }
    #[test]
    fn copy_requires_copy_intent_and_revokes_after_epoch_change() {
        let history = HistoryService::open(std::path::Path::new(":memory:")).unwrap();
        let id = history.save(&entry(0, "copy body"), None).unwrap();
        let summary = history
            .privacy()
            .authorize_read(
                PrivacyAccessIntent::RestrictedSummary,
                SensitiveAccess::Denied,
                None,
                1,
            )
            .unwrap();
        assert!(history
            .clipboard_payload_authorized(id, &summary, 1)
            .is_err());
        let permit = history
            .privacy()
            .authorize_read(
                PrivacyAccessIntent::SelectedCopy,
                SensitiveAccess::Denied,
                None,
                1,
            )
            .unwrap();
        assert!(
            matches!(history.clipboard_payload_authorized(id,&permit,1).unwrap(),ClipboardPayload::Text(value) if value=="copy body")
        );
        history
            .privacy()
            .set_session(PrivacySessionCommand::On { duration_ms: None }, 2)
            .unwrap();
        assert_eq!(
            history
                .clipboard_payload_authorized(id, &permit, 2)
                .unwrap_err(),
            PrivacyError::PrivacyEpochChanged
        );
    }
    #[test]
    fn session_search_filters_before_paging_and_all_modes_use_core_semantics() {
        let history = HistoryService::open(std::path::Path::new(":memory:")).unwrap();
        let permit = history
            .privacy()
            .authorize_read(
                PrivacyAccessIntent::RestrictedSummary,
                SensitiveAccess::Denied,
                None,
                1,
            )
            .unwrap();
        let ordinary = entry(-1, "synthetic searchable value");
        let mut protected = entry(-2, "synthetic searchable secret");
        protected.tags.push("sensitive".into());
        for mode in [
            SummarySearchMode::Contains,
            SummarySearchMode::Fuzzy,
            SummarySearchMode::Regex,
            SummarySearchMode::Fts5,
        ] {
            let result = history
                .search_summaries_for_session_authorized(
                    &[protected.clone(), ordinary.clone()],
                    &SearchRequest {
                        query: "searchable".into(),
                        mode,
                        page: SummaryQuery {
                            limit: 1,
                            ..Default::default()
                        },
                    },
                    &permit,
                    1,
                )
                .unwrap();
            assert_eq!(
                result.entries.iter().map(|e| e.id).collect::<Vec<_>>(),
                vec![-1]
            );
        }
    }
}
