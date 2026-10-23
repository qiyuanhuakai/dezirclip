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
    /// Trusted background OCR retains the pre-read capture ticket across queueing and recognition.
    /// The internal read permit is not returned to the caller or exposed as a client read grant.
    pub fn image_bytes_for_capture_authorized(
        &self,
        id: i64,
        ticket: &super::CaptureTicket,
        now_ms: i64,
    ) -> PrivacyResult<Option<Vec<u8>>> {
        let permit = self.privacy().capture_ocr_permit(id, ticket, now_ms)?;
        let entry = self.get_entry_authorized(id, &permit, now_ms)?;
        let bytes = match entry {
            Some(entry) if entry.content_type == "image" => match payload(entry, crate::ports::ClipboardAuthorization::new(|| false))? {
                ClipboardPayload::Image { bytes, .. } => Some(bytes),
                _ => return Err(PrivacyError::SensitiveAccessDenied),
            },
            _ => None,
        };
        self.privacy().check_capture(ticket, now_ms)?;
        Ok(bytes)
    }

    /// Native OCR consumes decoded image bytes through the same selected-copy authorization.
    pub fn image_bytes_authorized(
        &self,
        id: i64,
        permit: &PrivacyReadPermit,
        now_ms: i64,
    ) -> PrivacyResult<Option<Vec<u8>>> {
        let result = self.clipboard_payload_authorized(id, permit, now_ms)?;
        let result = match result {
            ClipboardPayload::Private { payload, .. } => *payload,
            payload => payload,
        };
        let bytes = match result {
            ClipboardPayload::Image { bytes, .. } => Some(bytes),
            _ => None,
        };
        self.privacy().check_read(permit, now_ms)?;
        Ok(bytes)
    }

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
        self.session_clipboard_payload_scoped(entry, None, permit, now_ms)
    }
    pub fn session_clipboard_payload_for_host_authorized(
        &self,
        entry: &ClipboardEntry,
        host_instance: &str,
        permit: &PrivacyReadPermit,
        now_ms: i64,
    ) -> PrivacyResult<ClipboardPayload> {
        self.session_clipboard_payload_scoped(entry, Some(host_instance), permit, now_ms)
    }
    fn session_clipboard_payload_scoped(
        &self,
        entry: &ClipboardEntry,
        host_instance: Option<&str>,
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
            self.authorize_session_entry_scoped(entry, host_instance, permit, now_ms)?,
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

    /// A new caller-owned payload has no history identity or history read grant.
    pub fn generated_clipboard_payload_authorized(
        &self,
        entry: &ClipboardEntry,
        permit: &PrivacyReadPermit,
        now_ms: i64,
    ) -> PrivacyResult<ClipboardPayload> {
        permit.require_generated()?;
        self.privacy().check_read(permit, now_ms)?;
        if entry.id != 0 || !entry.tags.is_empty() || !matches!(entry.content_type.as_str(), "text" | "code" | "url" | "rich_text" | "html" | "image")
            || entry.content_type == "image" && !entry.content.starts_with("data:image/") {
            return Err(PrivacyError::SensitiveAccessDenied);
        }
        let result = payload(entry.clone(), crate::ports::ClipboardAuthorization::new(|| false))?;
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
        self.session_external_files_scoped(entry, None, permit, now_ms)
    }
    pub fn session_external_files_for_host_authorized(
        &self,
        entry: &ClipboardEntry,
        host_instance: &str,
        permit: &PrivacyReadPermit,
        now_ms: i64,
    ) -> PrivacyResult<crate::ports::AuthorizedFileExport> {
        self.session_external_files_scoped(entry, Some(host_instance), permit, now_ms)
    }
    fn session_external_files_scoped(
        &self,
        entry: &ClipboardEntry,
        host_instance: Option<&str>,
        permit: &PrivacyReadPermit,
        now_ms: i64,
    ) -> PrivacyResult<crate::ports::AuthorizedFileExport> {
        permit.require_copy()?;
        let entry = self.authorize_session_entry_scoped(entry, host_instance, permit, now_ms)?;
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
    pub fn generated_external_files_authorized(
        &self,
        entry: &ClipboardEntry,
        permit: &PrivacyReadPermit,
        now_ms: i64,
    ) -> PrivacyResult<crate::ports::AuthorizedFileExport> {
        permit.require_generated()?;
        self.privacy().check_read(permit, now_ms)?;
        if entry.id != 0 || !entry.tags.is_empty() { return Err(PrivacyError::SensitiveAccessDenied); }
        let owner = self.clone();
        let permission = permit.clone();
        let authorization = crate::ports::ClipboardAuthorization::new(move || {
            owner.privacy().check_read(&permission, chrono::Utc::now().timestamp_millis()).is_ok()
        });
        let result = external_files(entry.clone(), authorization)?;
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
    fn ocr_image_bytes_require_current_permission_for_ordinary_and_private_images() {
        let history = HistoryService::open(std::path::Path::new(":memory:")).unwrap();
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(1, 1)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let data = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(png.get_ref())
        );
        for sensitive in [false, true] {
            let mut image = entry(0, &data);
            image.content_type = "image".into();
            if sensitive {
                image.tags.push("sensitive".into());
            }
            let id = history.save(&image, None).unwrap();
            let capture = history.privacy().capture_ticket(1).unwrap();
            assert_eq!(
                history
                    .image_bytes_for_capture_authorized(id, &capture, 1)
                    .unwrap()
                    .unwrap(),
                *png.get_ref()
            );
            let denied = history
                .privacy()
                .authorize_selected_copy(
                    id,
                    None,
                    SensitiveAccess::Denied,
                    None,
                    1,
                )
                .unwrap();
            if sensitive {
                assert!(history.image_bytes_authorized(id, &denied, 1).is_err());
            } else {
                assert!(history
                    .image_bytes_authorized(id, &denied, 1)
                    .unwrap()
                    .is_some());
            }
            let permit = history
                .privacy()
                .authorize_selected_copy(
                    id,
                    None,
                    SensitiveAccess::ExplicitUserAction,
                    None,
                    1,
                )
                .unwrap();
            assert_eq!(
                history
                    .image_bytes_authorized(id, &permit, 1)
                    .unwrap()
                    .unwrap(),
                *png.get_ref()
            );
            history
                .privacy()
                .set_session(PrivacySessionCommand::On { duration_ms: None }, 2)
                .unwrap();
            assert_eq!(
                history.image_bytes_authorized(id, &permit, 2).unwrap_err(),
                PrivacyError::PrivacyEpochChanged
            );
            history
                .privacy()
                .set_session(PrivacySessionCommand::Off, 3)
                .unwrap();
            assert_eq!(
                history
                    .image_bytes_for_capture_authorized(id, &capture, 3)
                    .unwrap_err(),
                PrivacyError::PrivacyEpochChanged
            );
        }
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
            .authorize_selected_copy(
                id,
                None,
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
    fn selected_copy_cannot_read_another_id_or_bulk_and_generated_copy_cannot_read_history() {
        let history = HistoryService::open(std::path::Path::new(":memory:")).unwrap();
        let mut private = entry(0, "synthetic selected private");
        private.tags.push("sensitive".into());
        let selected = history.save(&private, None).unwrap();
        private.content = "synthetic other private".into();
        let other = history.save(&private, None).unwrap();
        let permit = history.privacy().authorize_selected_copy(selected, None, SensitiveAccess::ExplicitUserAction, None, 1).unwrap();
        assert_eq!(history.get_entry_authorized(selected, &permit, 1).unwrap().unwrap().content, "synthetic selected private");
        assert_eq!(history.get_entry_authorized(other, &permit, 1).unwrap_err(), PrivacyError::SensitiveAccessDenied);
        assert_eq!(history.query_authorized(&super::super::HistoryQuery {limit:1000,..Default::default()}, &permit, 1).unwrap_err(), PrivacyError::SensitiveAccessDenied);
        assert!(history.derived_text_payload_authorized(other, "other derived".into(), &permit, 1).is_err());
        assert!(matches!(history.derived_text_payload_authorized(selected, "selected derived".into(), &permit, 1).unwrap(), ClipboardPayload::Private {..}));
        assert!(history.privacy().authorize_read(PrivacyAccessIntent::SelectedCopy, SensitiveAccess::ExplicitUserAction, None, 1).is_err());
        let generated = history.privacy().authorize_read(PrivacyAccessIntent::GeneratedCopy, SensitiveAccess::Denied, None, 1).unwrap();
        assert!(history.get_entry_authorized(selected, &generated, 1).is_err());
        assert!(history.clipboard_payload_authorized(selected, &generated, 1).is_err());
        assert!(history.query_authorized(&super::super::HistoryQuery {limit:1,..Default::default()}, &generated, 1).is_err());
        assert!(matches!(history.generated_clipboard_payload_authorized(&entry(0,"new synthetic text"), &generated, 1).unwrap(), ClipboardPayload::Text(value) if value=="new synthetic text"));
        assert!(history.generated_clipboard_payload_authorized(&entry(selected,"same caller text"), &generated, 1).is_err());
        assert!(history.generated_clipboard_payload_authorized(&entry(0,"new text"), &permit, 1).is_err());
        history.privacy().set_session(PrivacySessionCommand::On {duration_ms:None}, 2).unwrap();
        assert_eq!(history.derived_text_payload_authorized(selected,"late".into(),&permit,2).unwrap_err(),PrivacyError::PrivacyEpochChanged);
        assert_eq!(history.generated_clipboard_payload_authorized(&entry(0,"late"),&generated,2).unwrap_err(),PrivacyError::PrivacyEpochChanged);
    }
    #[test]
    fn selected_session_copy_requires_original_host_id_and_epoch_for_payload_and_derivation() {
        let history = HistoryService::open(std::path::Path::new(":memory:")).unwrap();
        let host = uuid::Uuid::new_v4().to_string();
        let later_host = uuid::Uuid::new_v4().to_string();
        let mut selected = entry(-42,"synthetic Session private");
        selected.tags.push("sensitive".into());
        let permit = history.privacy().authorize_selected_copy(-42,Some(&host),SensitiveAccess::ExplicitUserAction,None,1).unwrap();
        assert!(history.privacy().authorize_selected_copy(-42,None,SensitiveAccess::ExplicitUserAction,None,1).is_err());
        assert!(history.privacy().authorize_selected_copy(0,Some(&host),SensitiveAccess::ExplicitUserAction,None,1).is_err());
        assert!(history.session_clipboard_payload_authorized(&selected,&permit,1).is_err());
        assert!(matches!(history.session_clipboard_payload_for_host_authorized(&selected,&host,&permit,1).unwrap(),ClipboardPayload::Private{..}));
        assert!(history.session_clipboard_payload_for_host_authorized(&selected,&later_host,&permit,1).is_err());
        let mut other = selected.clone();other.id=-43;
        assert!(history.session_clipboard_payload_for_host_authorized(&other,&host,&permit,1).is_err());
        assert!(history.session_derived_text_payload_for_host_authorized(&selected,&later_host,"other host".into(),&permit,1).is_err());
        assert!(matches!(history.session_derived_text_payload_for_host_authorized(&selected,&host,"original derived".into(),&permit,1).unwrap(),ClipboardPayload::Private{..}));
        history.privacy().set_session(PrivacySessionCommand::On{duration_ms:None},2).unwrap();
        assert_eq!(history.session_clipboard_payload_for_host_authorized(&selected,&host,&permit,2).unwrap_err(),PrivacyError::PrivacyEpochChanged);
    }
    #[test]
    fn internal_capture_image_permission_cannot_be_a_user_copy_policy_or_bulk_grant() {
        let history=HistoryService::open(std::path::Path::new(":memory:")).unwrap();
        let id=history.save(&entry(0,"synthetic internal target"),None).unwrap();
        let other=history.save(&entry(0,"synthetic other target"),None).unwrap();
        let ticket=history.privacy().capture_ticket(1).unwrap();
        let permit=history.privacy().capture_ocr_permit(id,&ticket,1).unwrap();
        assert!(permit.require_copy().is_err());
        assert!(permit.require_policy_change().is_err());
        assert!(history.privacy().native_read_lease(&permit).is_err());
        assert!(history.get_entry_authorized(other,&permit,1).is_err());
        assert!(history.query_authorized(&super::super::HistoryQuery{limit:1,..Default::default()},&permit,1).is_err());
        assert!(history.clipboard_payload_authorized(id,&permit,1).is_err());
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
        self.session_derived_text_payload_scoped(source, None, text, permit, now_ms)
    }
    pub fn session_derived_text_payload_for_host_authorized(
        &self,
        source: &ClipboardEntry,
        host_instance: &str,
        text: String,
        permit: &PrivacyReadPermit,
        now_ms: i64,
    ) -> PrivacyResult<ClipboardPayload> {
        self.session_derived_text_payload_scoped(source, Some(host_instance), text, permit, now_ms)
    }
    fn session_derived_text_payload_scoped(
        &self,
        source: &ClipboardEntry,
        host_instance: Option<&str>,
        text: String,
        permit: &PrivacyReadPermit,
        now_ms: i64,
    ) -> PrivacyResult<ClipboardPayload> {
        permit.require_copy()?;
        self.authorize_session_entry_scoped(source, host_instance, permit, now_ms)?;
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
