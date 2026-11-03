use super::{CoreError, CoreResult, HistoryQuery, HistoryService};
use crate::services::backup;
use std::path::Path;

pub use backup::{ExportEntry, ImportMode};

#[derive(Clone)]
pub struct BackupService {
    history: HistoryService,
}
impl BackupService {
    pub fn new(history: HistoryService) -> Self {
        Self { history }
    }
    pub fn export_bytes(&self, passphrase: Option<&str>) -> CoreResult<(Vec<u8>, usize)> {
        let ticket = self.history.privacy().capture_ticket(chrono::Utc::now().timestamp_millis())
            .map_err(|error| CoreError::from(error.to_string()))?;
        self.export_bytes_authorized(passphrase, &ticket)
    }

    fn export_bytes_authorized(&self, passphrase: Option<&str>, ticket: &super::CaptureTicket) -> CoreResult<(Vec<u8>, usize)> {
        self.check_ticket(ticket)?;
        if passphrase.is_some_and(|value| value.chars().count() < 12) {
            return Err(CoreError::validation(
                "export passphrase must contain at least 12 characters",
            ));
        }
        if passphrase.is_none() {
            let protected = self
                .history
                .repository
                .has_protected_records()
                .map_err(CoreError::from)?;
            if protected {
                return Err(CoreError::from(
                    "SensitiveBackupPassphraseRequired".to_owned(),
                ));
            }
        }
        let entries = self.history.query_for_export(&HistoryQuery {
            limit: i32::MAX,
            ..Default::default()
        })?;
        if passphrase.is_none()
            && entries.iter().any(|entry| {
                crate::database::has_sensitive_tag(&entry.tags)
                    || entry
                        .content
                        .starts_with(crate::infrastructure::protected_resources::FILES_PREFIX)
            })
        {
            return Err(CoreError::from(
                "SensitiveBackupPassphraseRequired".to_owned(),
            ));
        }
        let entries = entries
            .iter()
            .map(backup::prepare_export_entry)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| CoreError::from(error.to_string()))?;
        let count = entries.len();
        let bytes = match passphrase {
            Some(passphrase) => backup::export_to_encrypted(entries, passphrase),
            None => backup::export_to_json(entries).map(String::into_bytes),
        }
        .map_err(|error| CoreError::from(error.to_string()))?;
        self.history
            .privacy()
            .check_capture(&ticket, chrono::Utc::now().timestamp_millis())
            .map_err(|error| CoreError::from(error.to_string()))?;
        Ok((bytes, count))
    }
    pub fn export_file(&self, destination: &Path, passphrase: Option<&str>) -> CoreResult<usize> {
        let ticket = self.history.privacy().capture_ticket(chrono::Utc::now().timestamp_millis())
            .map_err(|error| CoreError::from(error.to_string()))?;
        self.export_file_authorized(destination, passphrase, &ticket)
    }

    fn check_ticket(&self, ticket: &super::CaptureTicket) -> CoreResult<()> {
        self.history.privacy().check_capture(ticket, chrono::Utc::now().timestamp_millis())
            .map_err(|error| CoreError::from(error.to_string()))
    }

    pub fn preview_file(&self, path: &Path, ticket: &super::CaptureTicket) -> CoreResult<Vec<String>> {
        self.check_ticket(ticket)?;
        let bytes = crate::filesystem::read_regular_nofollow(path).map_err(|error| CoreError::from(error.to_string()))?;
        let data = std::str::from_utf8(&bytes).map_err(|_| CoreError::validation("expected a JSON backup"))?;
        let entries = backup::entries_from_json(data).map_err(|error| CoreError::validation(error.to_string()))?;
        let rows = entries.into_iter().filter(|entry| !crate::database::has_sensitive_tag(&entry.tags))
            .take(10).map(|entry| entry.content.chars().take(500).collect()).collect();
        self.check_ticket(ticket)?;
        Ok(rows)
    }

    pub fn import_file_authorized(&self, path: &Path, passphrase: Option<&str>, mode: ImportMode, ticket: &super::CaptureTicket) -> CoreResult<usize> {
        self.check_ticket(ticket)?;
        let bytes = crate::filesystem::read_regular_nofollow(path).map_err(|error| CoreError::from(error.to_string()))?;
        let plain = std::str::from_utf8(&bytes).is_ok_and(|text| text.trim_start().starts_with('{'));
        let password = if plain || bytes.len() < 28 { None } else {
            Some(passphrase.filter(|value| !value.is_empty()).ok_or_else(|| CoreError::validation("encrypted import requires a passphrase"))?)
        };
        self.import_bytes_authorized(&bytes, password, mode, ticket)
    }

    pub fn export_file_authorized(&self, destination: &Path, passphrase: Option<&str>, ticket: &super::CaptureTicket) -> CoreResult<usize> {
        self.history.ensure_export_destination(destination)?;
        let (bytes, count) = self.export_bytes_authorized(passphrase, ticket)?;
        crate::infrastructure::attachments::write_backup_checked(destination, &bytes, || self.check_ticket(ticket).map_err(String::from))
            .map_err(|error| CoreError::from(error.to_string()))?;
        Ok(count)
    }
    pub fn import_bytes(
        &self,
        bytes: &[u8],
        passphrase: Option<&str>,
        mode: ImportMode,
    ) -> CoreResult<usize> {
        let ticket = self
            .history
            .privacy()
            .capture_ticket(chrono::Utc::now().timestamp_millis())
            .map_err(|error| CoreError::from(error.to_string()))?;
        self.import_bytes_authorized(bytes, passphrase, mode, &ticket)
    }
    pub fn import_bytes_authorized(
        &self,
        bytes: &[u8],
        passphrase: Option<&str>,
        mode: ImportMode,
        ticket: &super::CaptureTicket,
    ) -> CoreResult<usize> {
        self.history
            .privacy()
            .check_capture(ticket, chrono::Utc::now().timestamp_millis())
            .map_err(|error| CoreError::from(error.to_string()))?;
        let json = match passphrase {
            Some(passphrase) => backup::decrypt_to_json(bytes, passphrase)
                .map_err(|error| CoreError::from(error.to_string()))?,
            None => String::from_utf8(bytes.to_vec())
                .map_err(|_| CoreError::validation("backup is not valid UTF-8"))?,
        };
        let entries =
            backup::entries_from_json(&json).map_err(|error| CoreError::from(error.to_string()))?;
        self.apply_authorized(&entries, mode, ticket)?;
        Ok(entries.len())
    }
    pub fn apply_authorized(
        &self,
        entries: &[ExportEntry],
        mode: ImportMode,
        ticket: &super::CaptureTicket,
    ) -> CoreResult<()> {
        backup::apply_entries_authorized(&self.history.repository, entries, mode, ticket)
            .map_err(CoreError::from)
    }
    pub fn apply(&self, entries: &[ExportEntry], mode: ImportMode) -> CoreResult<()> {
        backup::apply_entries(&self.history.repository, entries, mode).map_err(CoreError::from)
    }
}
