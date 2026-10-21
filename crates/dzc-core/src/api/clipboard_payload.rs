//! Selected copy translates authorized records inside Core; native clients never decrypt records.
use super::{ClipboardEntry, HistoryService, PrivacyError, PrivacyReadPermit, PrivacyResult};
use crate::ports::ClipboardPayload;
use base64::Engine;
use std::io::Read;
const MAX_IMAGE_BYTES: usize = 16 * 1024 * 1024;

fn payload(
    entry: ClipboardEntry,
    authorization: crate::ports::ClipboardAuthorization,
) -> PrivacyResult<ClipboardPayload> {
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
        "file" | "files" | "video"
            if entry
                .content
                .starts_with(crate::infrastructure::protected_resources::FILES_PREFIX) =>
        {
            let files = crate::infrastructure::protected_resources::decode_files(&entry.content)
                .map_err(|_| unavailable())?
                .into_iter()
                .map(|file| crate::ports::ClipboardFile {
                    name: file.name,
                    bytes: file.bytes,
                })
                .collect();
            ClipboardPayload::ProtectedFiles {
                files,
                authorization,
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
        let owner = self.clone();
        let permission = permit.clone();
        let authorization = crate::ports::ClipboardAuthorization::new(move || {
            owner
                .privacy()
                .check_read(&permission, chrono::Utc::now().timestamp_millis())
                .is_ok()
        });
        let protected = self
            .repository
            .entry_is_protected(id)
            .map_err(|_| PrivacyError::PrivacyStateUnavailable)?;
        let authorization =
            authorization.with_native_lease(self.privacy().native_read_lease(permit)?);
        let result = payload(entry, authorization.clone())?;
        let result = if protected && !matches!(result, ClipboardPayload::ProtectedFiles { .. }) {
            ClipboardPayload::Private {
                payload: Box::new(result),
                authorization,
            }
        } else {
            result
        };
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
        let owner = self.clone();
        let permission = permit.clone();
        let authorization = crate::ports::ClipboardAuthorization::new(move || {
            owner
                .privacy()
                .check_read(&permission, chrono::Utc::now().timestamp_millis())
                .is_ok()
        });
        let authorization =
            authorization.with_native_lease(self.privacy().native_read_lease(permit)?);
        let result = payload(
            self.authorize_session_entry(entry, permit, now_ms)?,
            authorization.clone(),
        )?;
        let result = if crate::database::has_sensitive_tag(&entry.tags)
            && !matches!(result, ClipboardPayload::ProtectedFiles { .. })
        {
            ClipboardPayload::Private {
                payload: Box::new(result),
                authorization,
            }
        } else {
            result
        };
        self.privacy().check_read(permit, now_ms)?;
        Ok(result)
    }
}

pub(super) fn preview_entry(
    mut entry: ClipboardEntry,
    maximum: Option<u32>,
) -> PrivacyResult<ClipboardEntry> {
    let unavailable = |_| PrivacyError::SensitiveAccessDenied;
    match entry.content_type.as_str() {
        "file" | "files" | "video" => {
            let names = if entry
                .content
                .starts_with(crate::infrastructure::protected_resources::FILES_PREFIX)
            {
                crate::infrastructure::protected_resources::decode_files(&entry.content)
                    .map_err(unavailable)?
                    .into_iter()
                    .map(|file| file.name)
                    .collect::<Vec<_>>()
            } else {
                let values: Vec<String> = if entry.content.trim_start().starts_with('[') {
                    serde_json::from_str(&entry.content)
                        .map_err(|_| PrivacyError::SensitiveAccessDenied)?
                } else {
                    entry.content.lines().map(str::to_owned).collect()
                };
                values
                    .iter()
                    .filter_map(|value| {
                        crate::infrastructure::attachments::path_from_uri(value).ok()
                    })
                    .filter_map(|path| {
                        std::path::Path::new(&path)
                            .file_name()
                            .and_then(|name| name.to_str())
                            .map(str::to_owned)
                    })
                    .take(64)
                    .collect()
            };
            entry.content.clear();
            entry.html_content = None;
            entry.ocr_text = None;
            entry.preview = names.join("\n");
            entry.is_external = false;
            entry.file_preview_exists = false;
        }
        "image" => {
            entry.content = match maximum {
                Some(maximum) => crate::infrastructure::protected_resources::thumbnail_data_url(
                    &entry.content,
                    maximum,
                ),
                None => crate::infrastructure::protected_resources::image_data_url(&entry.content),
            }
            .map_err(unavailable)?;
            entry.preview.clear();
            entry.is_external = false;
        }
        _ => {}
    }
    entry.html_content = entry
        .html_content
        .as_deref()
        .map(|html| match maximum {
            Some(maximum) => {
                crate::infrastructure::protected_resources::thumbnail_html(html, maximum)
            }
            None => crate::infrastructure::protected_resources::normalize_html(html),
        })
        .transpose()
        .map_err(unavailable)?;
    entry.source_app_path = None;
    Ok(entry)
}

impl HistoryService {
    pub fn get_entry_for_preview_authorized(
        &self,
        id: i64,
        permit: &PrivacyReadPermit,
        now_ms: i64,
    ) -> PrivacyResult<Option<ClipboardEntry>> {
        let result = self
            .get_entry_authorized(id, permit, now_ms)?
            .map(|entry| preview_entry(entry, None))
            .transpose()?;
        self.privacy().check_read(permit, now_ms)?;
        Ok(result)
    }
    pub fn session_entry_for_preview_authorized(
        &self,
        entry: &ClipboardEntry,
        permit: &PrivacyReadPermit,
        now_ms: i64,
    ) -> PrivacyResult<ClipboardEntry> {
        let result = preview_entry(self.authorize_session_entry(entry, permit, now_ms)?, None)?;
        self.privacy().check_read(permit, now_ms)?;
        Ok(result)
    }
}

impl HistoryService {
    pub fn get_entry_thumbnail_authorized(
        &self,
        id: i64,
        permit: &PrivacyReadPermit,
        now_ms: i64,
        maximum: u32,
    ) -> PrivacyResult<Option<ClipboardEntry>> {
        let result = self
            .get_entry_authorized(id, permit, now_ms)?
            .map(|entry| preview_entry(entry, Some(maximum)))
            .transpose()?;
        self.privacy().check_read(permit, now_ms)?;
        Ok(result)
    }
    pub fn session_entry_thumbnail_authorized(
        &self,
        entry: &ClipboardEntry,
        permit: &PrivacyReadPermit,
        now_ms: i64,
        maximum: u32,
    ) -> PrivacyResult<ClipboardEntry> {
        let result = preview_entry(
            self.authorize_session_entry(entry, permit, now_ms)?,
            Some(maximum),
        )?;
        self.privacy().check_read(permit, now_ms)?;
        Ok(result)
    }
}

fn external_files(
    entry: ClipboardEntry,
    authorization: crate::ports::ClipboardAuthorization,
) -> PrivacyResult<crate::ports::AuthorizedFileExport> {
    let files = if matches!(entry.content_type.as_str(), "file" | "files" | "video") {
        let content = crate::infrastructure::protected_resources::archive_files(&entry.content)
            .map_err(|_| PrivacyError::SensitiveAccessDenied)?;
        crate::infrastructure::protected_resources::decode_files(&content)
            .map_err(|_| PrivacyError::SensitiveAccessDenied)?
            .into_iter()
            .map(|file| crate::ports::ClipboardFile {
                name: file.name,
                bytes: file.bytes,
            })
            .collect()
    } else if entry.content_type == "image" {
        let ClipboardPayload::Image { bytes, mime } = payload(entry, authorization.clone())? else {
            return Err(PrivacyError::SensitiveAccessDenied);
        };
        let extension = match mime.as_str() {
            "image/jpeg" => "jpg",
            "image/gif" => "gif",
            "image/webp" => "webp",
            "image/bmp" => "bmp",
            _ => "png",
        };
        vec![crate::ports::ClipboardFile {
            name: format!("DezirClip.{extension}"),
            bytes,
        }]
    } else {
        if entry.content.len() > 16 * 1024 * 1024 {
            return Err(PrivacyError::SensitiveAccessDenied);
        }
        vec![crate::ports::ClipboardFile {
            name: "DezirClip.txt".into(),
            bytes: entry.content.into_bytes(),
        }]
    };
    Ok(crate::ports::AuthorizedFileExport {
        files,
        authorization,
    })
}
impl HistoryService {
    pub fn external_files_authorized(
        &self,
        id: i64,
        permit: &PrivacyReadPermit,
        now_ms: i64,
    ) -> PrivacyResult<crate::ports::AuthorizedFileExport> {
        permit.require_copy()?;
        let entry = self
            .get_entry_authorized(id, permit, now_ms)?
            .ok_or(PrivacyError::SensitiveAccessDenied)?;
        let owner = self.clone();
        let permission = permit.clone();
        let authorization = crate::ports::ClipboardAuthorization::new(move || {
            owner
                .privacy()
                .check_read(&permission, chrono::Utc::now().timestamp_millis())
                .is_ok()
        });
        let result = external_files(entry, authorization)?;
        self.privacy().check_read(permit, now_ms)?;
        Ok(result)
    }
    pub fn session_external_files_authorized(
        &self,
        entry: &ClipboardEntry,
        permit: &PrivacyReadPermit,
        now_ms: i64,
    ) -> PrivacyResult<crate::ports::AuthorizedFileExport> {
        permit.require_copy()?;
        let entry = self.authorize_session_entry(entry, permit, now_ms)?;
        let owner = self.clone();
        let permission = permit.clone();
        let authorization = crate::ports::ClipboardAuthorization::new(move || {
            owner
                .privacy()
                .check_read(&permission, chrono::Utc::now().timestamp_millis())
                .is_ok()
        });
        let result = external_files(entry, authorization)?;
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

impl HistoryService {
    fn derived_text_payload(
        &self,
        protected: bool,
        text: String,
        permit: &PrivacyReadPermit,
        now_ms: i64,
    ) -> PrivacyResult<ClipboardPayload> {
        permit.require_copy()?;
        if text.len() > 16 * 1024 * 1024 {
            return Err(PrivacyError::SensitiveAccessDenied);
        }
        let payload = ClipboardPayload::Text(text);
        let result = if protected {
            let owner = self.clone();
            let permission = permit.clone();
            let authorization = crate::ports::ClipboardAuthorization::new(move || {
                owner
                    .privacy()
                    .check_read(&permission, chrono::Utc::now().timestamp_millis())
                    .is_ok()
            })
            .with_native_lease(self.privacy().native_read_lease(permit)?);
            ClipboardPayload::Private {
                payload: Box::new(payload),
                authorization,
            }
        } else {
            payload
        };
        self.privacy().check_read(permit, now_ms)?;
        Ok(result)
    }
    pub fn derived_text_payload_authorized(
        &self,
        source_id: i64,
        text: String,
        permit: &PrivacyReadPermit,
        now_ms: i64,
    ) -> PrivacyResult<ClipboardPayload> {
        permit.require_copy()?;
        self.get_entry_authorized(source_id, permit, now_ms)?
            .ok_or(PrivacyError::SensitiveAccessDenied)?;
        let protected = self
            .repository
            .entry_is_protected(source_id)
            .map_err(|_| PrivacyError::PrivacyStateUnavailable)?;
        self.derived_text_payload(protected, text, permit, now_ms)
    }
    pub fn session_derived_text_payload_authorized(
        &self,
        source: &ClipboardEntry,
        text: String,
        permit: &PrivacyReadPermit,
        now_ms: i64,
    ) -> PrivacyResult<ClipboardPayload> {
        permit.require_copy()?;
        self.authorize_session_entry(source, permit, now_ms)?;
        let tags = serde_json::to_string(&source.tags)
            .map_err(|_| PrivacyError::PrivacyStateUnavailable)?;
        let protected = !crate::infrastructure::repository::migrations::search_visible(
            Some(&tags),
            [
                Some(&source.content),
                Some(&source.preview),
                source.html_content.as_deref(),
                source.ocr_text.as_deref(),
            ],
        );
        self.derived_text_payload(protected, text, permit, now_ms)
    }
}
