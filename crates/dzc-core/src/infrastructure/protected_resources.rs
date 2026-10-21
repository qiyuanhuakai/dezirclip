//! RAM-only representations of protected resources. No plaintext attachment is materialized.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const FILES_PREFIX: &str = "dzc-files-v1:";
pub const MAX_RESOURCE_BYTES: usize = 16 * 1024 * 1024;
const MAX_TOTAL_BYTES: usize = 32 * 1024 * 1024;
const MAX_RESOURCES: usize = 64;

pub struct NormalizedResources {
    pub content: String,
    pub html: Option<String>,
}

pub struct DecodedFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileArchive {
    version: u8,
    files: Vec<ArchivedFile>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArchivedFile {
    name: String,
    data: String,
}

fn invalid() -> String {
    "SensitivePersistenceRejected".into()
}
fn budget() -> String {
    "AttachmentBudgetExceeded".into()
}

fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 1024
        && !matches!(name, "." | "..")
        && !name.contains(['/', '\\', ':'])
        && !name.chars().any(char::is_control)
}

fn read_resource(value: &str, maximum: usize) -> Result<Vec<u8>, String> {
    let path = crate::infrastructure::attachments::path_from_uri(value).map_err(|_| invalid())?;
    use std::io::Read;
    let file = crate::infrastructure::local_file::open_regular_nofollow(Path::new(&path))
        .map_err(|_| "AttachmentReadRejected".to_owned())?;
    let mut bytes = Vec::new();
    file.take(maximum as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "AttachmentReadRejected".to_owned())?;
    if bytes.len() > maximum {
        return Err(budget());
    }
    Ok(bytes)
}

fn image_bytes(value: &str) -> Result<Vec<u8>, String> {
    let bytes = if value.starts_with("data:image/") {
        let (header, data) = value.split_once(',').ok_or_else(invalid)?;
        if !header.ends_with(";base64") {
            return Err(invalid());
        }
        if data.len() > MAX_RESOURCE_BYTES.div_ceil(3) * 4 {
            return Err(budget());
        }
        STANDARD.decode(data).map_err(|_| invalid())?
    } else {
        read_resource(value, MAX_RESOURCE_BYTES)?
    };
    if bytes.len() > MAX_RESOURCE_BYTES {
        return Err(budget());
    }
    image::guess_format(&bytes).map_err(|_| invalid())?;
    Ok(bytes)
}

pub fn image_data_url(value: &str) -> Result<String, String> {
    let bytes = image_bytes(value)?;
    let mime = image::guess_format(&bytes)
        .map_err(|_| invalid())?
        .to_mime_type();
    Ok(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

pub fn decode_files(content: &str) -> Result<Vec<DecodedFile>, String> {
    if content.len() > MAX_TOTAL_BYTES.div_ceil(3) * 4 + MAX_RESOURCES * 2048 {
        return Err(budget());
    }
    let json = content.strip_prefix(FILES_PREFIX).ok_or_else(invalid)?;
    let archive: FileArchive = serde_json::from_str(json).map_err(|_| invalid())?;
    if archive.version != 1 || archive.files.is_empty() || archive.files.len() > MAX_RESOURCES {
        return Err(invalid());
    }
    let mut total = 0usize;
    archive
        .files
        .into_iter()
        .map(|file| {
            if !safe_name(&file.name) {
                return Err(invalid());
            }
            if file.data.len() > MAX_RESOURCE_BYTES.div_ceil(3) * 4 {
                return Err(budget());
            }
            let bytes = STANDARD.decode(file.data).map_err(|_| invalid())?;
            total = total.checked_add(bytes.len()).ok_or_else(budget)?;
            if bytes.len() > MAX_RESOURCE_BYTES || total > MAX_TOTAL_BYTES {
                return Err(budget());
            }
            Ok(DecodedFile {
                name: file.name,
                bytes,
            })
        })
        .collect()
}

pub(crate) fn archive_files(content: &str) -> Result<String, String> {
    if content.starts_with(FILES_PREFIX) {
        decode_files(content)?;
        return Ok(content.to_owned());
    }
    let values: Vec<String> = if content.trim_start().starts_with('[') {
        serde_json::from_str(content).map_err(|_| invalid())?
    } else {
        content
            .lines()
            .filter(|path| !path.is_empty() && !path.starts_with('#'))
            .map(str::to_owned)
            .collect()
    };
    let paths: Vec<&str> = values.iter().map(String::as_str).collect();
    if paths.is_empty() || paths.len() > MAX_RESOURCES {
        return Err(invalid());
    }
    let mut total = 0usize;
    let files = paths
        .into_iter()
        .map(|value| {
            let path =
                crate::infrastructure::attachments::path_from_uri(value).map_err(|_| invalid())?;
            let name = Path::new(&path)
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(invalid)?
                .to_owned();
            if !safe_name(&name) {
                return Err(invalid());
            }
            let bytes = read_resource(
                value,
                MAX_RESOURCE_BYTES.min(MAX_TOTAL_BYTES.saturating_sub(total)),
            )?;
            total += bytes.len();
            Ok(ArchivedFile {
                name,
                data: STANDARD.encode(bytes),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let json = serde_json::to_string(&FileArchive { version: 1, files }).map_err(|_| invalid())?;
    Ok(format!("{FILES_PREFIX}{json}"))
}

pub fn normalize_html(html: &str) -> Result<String, String> {
    let (values, complete) = super::repository::sensitive_tags::resource_references(html);
    if !complete || values.len() > MAX_RESOURCES {
        return Err(invalid());
    }
    let mut output = html.to_owned();
    let mut total = 0usize;
    for value in values {
        let bytes = image_bytes(&value)?;
        total = total.checked_add(bytes.len()).ok_or_else(budget)?;
        if total > MAX_TOTAL_BYTES {
            return Err(budget());
        }
        let mime = image::guess_format(&bytes)
            .map_err(|_| invalid())?
            .to_mime_type();
        output = output.replace(
            &value,
            &format!("data:{mime};base64,{}", STANDARD.encode(bytes)),
        );
    }
    let (values, complete) = super::repository::sensitive_tags::resource_references(&output);
    if !complete || values.iter().any(|value| !value.starts_with("data:image/")) {
        return Err(invalid());
    }
    Ok(output)
}

pub fn normalize(
    kind: &str,
    content: &str,
    html: Option<&str>,
    external: bool,
) -> Result<NormalizedResources, String> {
    let content = match kind {
        "image" => image_data_url(content)?,
        "file" | "files" | "video" => archive_files(content)?,
        _ if external => return Err(invalid()),
        _ => content.to_owned(),
    };
    let html = html.map(normalize_html).transpose()?;
    Ok(NormalizedResources { content, html })
}

pub fn thumbnail_data_url(value: &str, maximum: u32) -> Result<String, String> {
    let bytes = image_bytes(value)?;
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| invalid())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(16384);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|_| invalid())?
        .thumbnail(maximum.clamp(64, 1024), maximum.clamp(64, 1024));
    let mut output = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut output, image::ImageFormat::Png)
        .map_err(|_| invalid())?;
    Ok(format!(
        "data:image/png;base64,{}",
        STANDARD.encode(output.into_inner())
    ))
}
pub fn thumbnail_html(html: &str, maximum: u32) -> Result<String, String> {
    let normalized = normalize_html(html)?;
    let (references, complete) =
        super::repository::sensitive_tags::resource_references(&normalized);
    if !complete {
        return Err(invalid());
    }
    let mut output = normalized;
    for reference in references {
        output = output.replace(&reference, &thumbnail_data_url(&reference, maximum)?);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{
        BackupService, ClipboardEntry, HistoryService, ImportMode, PrivacyAccessIntent,
        PrivacyError, PrivacySessionCommand, SensitiveAccess,
    };
    use crate::infrastructure::repository::migrations;
    struct Fixture {
        directory: std::path::PathBuf,
        history: HistoryService,
        conn: rusqlite::Connection,
    }
    impl Fixture {
        fn new() -> Self {
            let directory =
                std::env::temp_dir().join(format!("dzc-protected-bytes-{}", uuid::Uuid::new_v4()));
            let path = directory.join("synthetic.db");
            let history = HistoryService::open(&path).unwrap();
            let conn = rusqlite::Connection::open(path).unwrap();
            migrations::configure_privacy_writer(&conn).unwrap();
            Self {
                directory,
                history,
                conn,
            }
        }
        fn entry(&self, kind: &str, content: String) -> ClipboardEntry {
            serde_json::from_value(serde_json::json!({"id":0,"content_type":kind,"content":content,"preview":"private summary","source_app":"Synthetic","timestamp":1,"is_pinned":false,"tags":["sensitive"]})).unwrap()
        }
        fn permit(&self) -> crate::api::PrivacyReadPermit {
            self.history
                .privacy()
                .authorize_read(
                    PrivacyAccessIntent::SelectedCopy,
                    SensitiveAccess::ExplicitUserAction,
                    None,
                    1,
                )
                .unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }
    fn png() -> String {
        let image = image::DynamicImage::new_rgb8(2, 2);
        let mut out = std::io::Cursor::new(Vec::new());
        image.write_to(&mut out, image::ImageFormat::Png).unwrap();
        format!(
            "data:image/png;base64,{}",
            STANDARD.encode(out.into_inner())
        )
    }
    #[test]
    fn image_html_and_file_bytes_are_encrypted_without_materializing_plaintext_assets() {
        let fixture = Fixture::new();
        let source = fixture.directory.join("synthetic.txt");
        std::fs::write(&source, b"qvxprivatecompletefile").unwrap();
        let mut html = fixture.entry("rich_text", "qvxprivatetext".into());
        html.html_content = Some(format!("<img src='{}'>", png()));
        let mut ids = Vec::new();
        for entry in [
            fixture.entry("image", png()),
            html,
            fixture.entry("file", source.to_string_lossy().into_owned()),
        ] {
            ids.push(
                fixture
                    .history
                    .save(&entry, Some(&fixture.directory))
                    .unwrap(),
            );
        }
        for id in &ids {
            let (content,preview,html,external):(String,String,Option<String>,bool)=fixture.conn.query_row("SELECT content,preview,html_content,is_external FROM clipboard_history WHERE id=?1",[id],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).unwrap();
            assert!(!external);
            assert!(crate::infrastructure::encryption::has_ciphertext_envelope(
                &content
            ));
            assert!(crate::infrastructure::encryption::has_ciphertext_envelope(
                &preview
            ));
            if let Some(html) = html {
                assert!(crate::infrastructure::encryption::has_ciphertext_envelope(
                    &html
                ));
                assert!(!html.contains("data:image"));
            }
        }
        assert_eq!(std::fs::read(&source).unwrap(), b"qvxprivatecompletefile");
        assert!(!fixture.directory.join("attachments").exists());
        assert_eq!(
            fixture
                .conn
                .query_row("SELECT count(*) FROM clipboard_fts_docsize", [], |row| row
                    .get::<_, i64>(
                    0
                ))
                .unwrap(),
            0
        );
        let permit = fixture.permit();
        let preview = fixture
            .history
            .get_entry_for_preview_authorized(ids[2], &permit, 1)
            .unwrap()
            .unwrap();
        assert_eq!(preview.content, "");
        assert_eq!(preview.preview, "synthetic.txt");
        let crate::ports::ClipboardPayload::ProtectedFiles {
            files,
            authorization,
        } = fixture
            .history
            .clipboard_payload_authorized(ids[2], &permit, 1)
            .unwrap()
        else {
            panic!("protected files required")
        };
        assert_eq!(files[0].bytes, b"qvxprivatecompletefile");
        assert!(authorization.is_current());
        fixture
            .history
            .privacy()
            .set_session(PrivacySessionCommand::On { duration_ms: None }, 2)
            .unwrap();
        fixture
            .history
            .privacy()
            .set_session(PrivacySessionCommand::Off, 3)
            .unwrap();
        assert!(!authorization.is_current());
    }
    #[test]
    fn late_ocr_uses_original_ticket_encrypts_fields_and_rejects_after_on_off() {
        let fixture = Fixture::new();
        let ticket = fixture.history.privacy().capture_ticket(1).unwrap();
        let id = fixture
            .history
            .save(&fixture.entry("image", png()), None)
            .unwrap();
        fixture
            .history
            .update_ocr_authorized(id, "qvxlateprivateocr", "done", &ticket, 1)
            .unwrap();
        let before: (String, String) = fixture
            .conn
            .query_row(
                "SELECT ocr_text,ocr_status FROM clipboard_history WHERE id=?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!(crate::infrastructure::encryption::has_ciphertext_envelope(
            &before.0
        ));
        assert!(!before.0.contains("qvxlateprivateocr"));
        assert_eq!(fixture.conn.query_row("SELECT count(*) FROM clipboard_fts WHERE clipboard_fts MATCH 'qvxlateprivateocr'",[],|row|row.get::<_,i64>(0)).unwrap(),0);
        fixture
            .history
            .privacy()
            .set_session(PrivacySessionCommand::On { duration_ms: None }, 2)
            .unwrap();
        fixture
            .history
            .privacy()
            .set_session(PrivacySessionCommand::Off, 3)
            .unwrap();
        let error = fixture
            .history
            .update_ocr_authorized(id, "cancelled secret", "done", &ticket, 3)
            .unwrap_err();
        assert!(error.to_string().contains("PrivacyEpochChanged"));
        assert_eq!(
            fixture
                .conn
                .query_row(
                    "SELECT ocr_text,ocr_status FROM clipboard_history WHERE id=?1",
                    [id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                )
                .unwrap(),
            before
        );
    }
    #[test]
    fn protected_backup_requires_passphrase_and_v2_roundtrip_preserves_complete_bytes() {
        let fixture = Fixture::new();
        let source = fixture.directory.join("backup.txt");
        std::fs::write(&source, b"full protected backup file").unwrap();
        let id = fixture
            .history
            .save(
                &fixture.entry("file", source.to_string_lossy().into_owned()),
                None,
            )
            .unwrap();
        let backup = BackupService::new(fixture.history.clone());
        assert!(backup
            .export_bytes(None)
            .unwrap_err()
            .to_string()
            .contains("SensitiveBackupPassphraseRequired"));
        let (bytes, count) = backup
            .export_bytes(Some("synthetic backup passphrase"))
            .unwrap();
        assert_eq!(count, 1);
        assert!(!bytes
            .windows(26)
            .any(|value| value == b"full protected backup file"));
        let restored = Fixture::new();
        BackupService::new(restored.history.clone())
            .import_bytes(
                &bytes,
                Some("synthetic backup passphrase"),
                ImportMode::Replace,
            )
            .unwrap();
        let entry = restored
            .history
            .get_entry_authorized(id, &restored.permit(), 1)
            .unwrap()
            .unwrap();
        assert_eq!(
            decode_files(&entry.content).unwrap()[0].bytes,
            b"full protected backup file"
        );
    }
    #[test]
    fn protection_transition_revokes_read_and_capture_permits_and_gui_has_only_placeholders() {
        let fixture = Fixture::new();
        let mut entry = fixture.entry("text", "ordinary before protection".into());
        entry.tags.clear();
        let id = fixture.history.save(&entry, None).unwrap();
        let permit = fixture.permit();
        let ticket = fixture.history.privacy().capture_ticket(1).unwrap();
        fixture
            .history
            .update_entry_tags(id, vec!["sensitive".into()])
            .unwrap();
        assert_eq!(
            fixture
                .history
                .privacy()
                .check_read(&permit, 1)
                .unwrap_err(),
            PrivacyError::PrivacyEpochChanged
        );
        assert_eq!(
            fixture
                .history
                .privacy()
                .check_capture(&ticket, 1)
                .unwrap_err(),
            PrivacyError::PrivacyEpochChanged
        );
        let mut entries = vec![fixture.history.get_entry_by_id(id).unwrap().unwrap()];
        fixture
            .history
            .entries_for_gui(&mut entries, true, 1)
            .unwrap();
        assert_eq!(entries[0].content, "");
        assert_eq!(entries[0].preview, "");
        assert!(entries[0].ocr_text.is_none());
        assert!(entries[0].html_content.is_none());
    }
    #[test]
    fn incomplete_or_oversized_archives_reject_without_filename_or_plaintext_error_detail() {
        for content in [
            "dzc-files-v1:{}",
            "dzc-files-v1:{\"version\":1,\"files\":[{\"name\":\"../secret\",\"data\":\"YQ==\"}]}",
        ]
        .iter()
        {
            assert_eq!(
                decode_files(content).err().unwrap(),
                "SensitivePersistenceRejected"
            );
        }
    }
    #[test]
    fn import_rechecks_original_epoch_before_replace_and_protected_plain_exports_are_rejected() {
        let fixture = Fixture::new();
        let mut ordinary = fixture.entry("text", "keep original row".into());
        ordinary.tags.clear();
        let id = fixture.history.save(&ordinary, None).unwrap();
        let ticket = fixture.history.privacy().capture_ticket(1).unwrap();
        let mut restored = crate::services::backup::prepare_export_entry(&ordinary).unwrap();
        restored.id = 9;
        fixture
            .history
            .privacy()
            .set_session(PrivacySessionCommand::On { duration_ms: None }, 2)
            .unwrap();
        fixture
            .history
            .privacy()
            .set_session(PrivacySessionCommand::Off, 3)
            .unwrap();
        let error = BackupService::new(fixture.history.clone())
            .apply_authorized(&[restored.clone()], ImportMode::Replace, &ticket)
            .unwrap_err();
        assert!(error.to_string().contains("PrivacyEpochChanged"));
        assert_eq!(
            fixture
                .conn
                .query_row(
                    "SELECT content FROM clipboard_history WHERE id=?1",
                    [id],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "keep original row"
        );
        assert_eq!(
            fixture
                .conn
                .query_row("SELECT count(*) FROM clipboard_history", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        restored.tags = vec!["sensitive".into()];
        assert!(
            crate::services::backup::export_to_json(vec![restored.clone()])
                .unwrap_err()
                .to_string()
                .contains("SensitiveBackupPassphraseRequired")
        );
        assert!(crate::services::backup::export_to_encrypted(
            vec![restored],
            "synthetic passphrase"
        )
        .is_ok());
    }

    #[test]
    fn late_ocr_sql_failure_does_not_commit_ciphertext_status_revision_or_epoch() {
        let fixture = Fixture::new();
        let id = fixture
            .history
            .save(&fixture.entry("image", png()), None)
            .unwrap();
        let ticket = fixture.history.privacy().capture_ticket(1).unwrap();
        let before:(Option<String>,Option<String>,i64,i64)=fixture.conn.query_row("SELECT ocr_text,ocr_status,history_revision,privacy_epoch FROM clipboard_history,privacy_runtime_state WHERE id=?1",[id],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).unwrap();
        fixture.conn.execute_batch("CREATE TRIGGER reject_synthetic_ocr BEFORE UPDATE OF ocr_status ON clipboard_history WHEN NEW.ocr_status='done' BEGIN SELECT RAISE(ABORT,'synthetic OCR rejection'); END").unwrap();
        assert!(fixture
            .history
            .update_ocr_authorized(id, "qvxsqlfailsecret", "done", &ticket, 1)
            .is_err());
        let after:(Option<String>,Option<String>,i64,i64)=fixture.conn.query_row("SELECT ocr_text,ocr_status,history_revision,privacy_epoch FROM clipboard_history,privacy_runtime_state WHERE id=?1",[id],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).unwrap();
        assert_eq!(after, before);
        assert_eq!(fixture.conn.query_row("SELECT count(*) FROM clipboard_fts WHERE clipboard_fts MATCH 'qvxsqlfailsecret'",[],|row|row.get::<_,i64>(0)).unwrap(),0);
        fixture
            .conn
            .execute_batch("DROP TRIGGER reject_synthetic_ocr")
            .unwrap();
        fixture
            .history
            .update_ocr_authorized(id, "qvxretrysecret", "done", &ticket, 1)
            .unwrap();
    }

    #[test]
    fn conversion_removes_only_unshared_managed_plaintext_resources() {
        let fixture = Fixture::new();
        let mut image = fixture.entry("image", png());
        image.tags.clear();
        let first = fixture
            .history
            .save(&image, Some(&fixture.directory))
            .unwrap();
        let path = fixture
            .history
            .get_entry_by_id(first)
            .unwrap()
            .unwrap()
            .content;
        assert!(std::path::Path::new(&path).is_file());
        let mut shared = image.clone();
        shared.content = path.clone();
        shared.is_external = true;
        let second = fixture.history.save(&shared, None).unwrap();
        fixture
            .history
            .update_entry_tags(first, vec!["sensitive".into()])
            .unwrap();
        assert!(std::path::Path::new(&path).is_file());
        fixture
            .history
            .update_entry_tags(second, vec!["sensitive".into()])
            .unwrap();
        assert!(!std::path::Path::new(&path).exists());
        for id in [first, second] {
            let stored: (String, bool) = fixture
                .conn
                .query_row(
                    "SELECT content,is_external FROM clipboard_history WHERE id=?1",
                    [id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert!(!stored.1);
            assert!(crate::infrastructure::encryption::has_ciphertext_envelope(
                &stored.0
            ));
        }
    }

    #[test]
    fn gui_projection_adds_sensitive_marker_to_untagged_envelopes_without_changing_user_tags() {
        let fixture = Fixture::new();
        let id = fixture
            .history
            .save(&fixture.entry("text", "qvxuntaggedsecret".into()), None)
            .unwrap();
        fixture
            .conn
            .execute("UPDATE clipboard_history SET tags='[]' WHERE id=?1", [id])
            .unwrap();
        let mut entries = vec![fixture.history.get_entry_by_id(id).unwrap().unwrap()];
        assert!(entries[0].tags.is_empty());
        fixture
            .history
            .entries_for_gui(&mut entries, true, 1)
            .unwrap();
        assert_eq!(entries[0].tags, vec!["sensitive"]);
        assert!(entries[0].content.is_empty());
        assert!(entries[0].preview.is_empty());
        assert_eq!(
            fixture
                .conn
                .query_row(
                    "SELECT tags FROM clipboard_history WHERE id=?1",
                    [id],
                    |row| row.get::<_, String>(0)
                )
                .unwrap(),
            "[]"
        );
        let mut session = fixture.entry(
            "text",
            crate::infrastructure::encryption::encrypt_value("qvxsessionsecret").unwrap(),
        );
        session.id = -1;
        session.tags.clear();
        let mut session = vec![session];
        fixture
            .history
            .entries_for_gui(&mut session, true, 1)
            .unwrap();
        assert_eq!(session[0].tags, vec!["sensitive"]);
        assert!(session[0].content.is_empty());
    }

    #[test]
    fn derived_text_keeps_source_protection_even_when_the_new_text_has_no_detector_match() {
        let fixture = Fixture::new();
        let id = fixture
            .history
            .save(&fixture.entry("text", "source secret".into()), None)
            .unwrap();
        let denied = fixture
            .history
            .privacy()
            .authorize_read(
                PrivacyAccessIntent::SelectedCopy,
                SensitiveAccess::Denied,
                None,
                1,
            )
            .unwrap();
        assert_eq!(
            fixture
                .history
                .derived_text_payload_authorized(id, "innocent output".into(), &denied, 1)
                .unwrap_err(),
            PrivacyError::SensitiveAccessDenied
        );
        let permit = fixture.permit();
        let crate::ports::ClipboardPayload::Private {
            payload,
            authorization,
        } = fixture
            .history
            .derived_text_payload_authorized(id, "innocent output".into(), &permit, 1)
            .unwrap()
        else {
            panic!("derived payload lost protection")
        };
        assert!(
            matches!(*payload,crate::ports::ClipboardPayload::Text(ref value) if value=="innocent output")
        );
        let lease = authorization.native_lease().unwrap();
        assert!(lease.is_current());
        fixture
            .history
            .privacy()
            .set_session(PrivacySessionCommand::On { duration_ms: None }, 2)
            .unwrap();
        fixture
            .history
            .privacy()
            .set_session(PrivacySessionCommand::Off, 3)
            .unwrap();
        assert!(!authorization.is_current());
        assert!(!lease.is_current());
        assert_eq!(
            fixture
                .history
                .derived_text_payload_authorized(id, "late output".into(), &permit, 3)
                .unwrap_err(),
            PrivacyError::PrivacyEpochChanged
        );
    }

    #[test]
    fn generated_capture_inherits_source_protection_before_preparation_even_with_detector_disabled()
    {
        use crate::capture::*;
        let fixture = Fixture::new();
        let session = std::sync::Mutex::new(std::collections::VecDeque::new());
        let context = CaptureContext {
            source: CaptureSource {
                app_name: "Synthetic Derived".into(),
                process_path: None,
            },
            timestamp: 1,
            settings: CaptureSettings {
                persistent: true,
                ..Default::default()
            },
            session: &session,
            paused: false,
            sequential_echo: None,
        };
        let store = fixture.history.capture_store(&fixture.directory);
        let frame = CaptureFrame {
            data: CaptureData::Text("qvxderivedordinary".into()),
            metadata: CaptureReadMetadata {
                privacy_ticket: Some(fixture.history.privacy().capture_ticket(1).unwrap()),
                inherited_protection: true,
                ..Default::default()
            },
        };
        let CaptureOutcome::Stored(stored) =
            CaptureEngine::capture_frame(&context, frame, &store, &mut ())
        else {
            panic!("derived capture not stored")
        };
        assert!(crate::database::has_sensitive_tag(&stored.entry.tags));
        let content: String = fixture
            .conn
            .query_row(
                "SELECT content FROM clipboard_history WHERE id=?1",
                [stored.entry.id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(crate::infrastructure::encryption::has_ciphertext_envelope(
            &content
        ));
        assert!(!content.contains("qvxderivedordinary"));
        assert_eq!(
            fixture
                .conn
                .query_row("SELECT count(*) FROM clipboard_fts_docsize", [], |row| row
                    .get::<_, i64>(
                    0
                ))
                .unwrap(),
            0
        );
    }
}
