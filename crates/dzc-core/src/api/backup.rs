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
        if passphrase.is_some_and(|value| value.chars().count() < 12) {
            return Err(CoreError::validation(
                "export passphrase must contain at least 12 characters",
            ));
        }
        let entries = self.history.query_for_export(&HistoryQuery {
            limit: i32::MAX,
            ..Default::default()
        })?;
        let entries = entries
            .iter()
            .map(backup::prepare_export_entry)
            .collect::<Result<Vec<_>, _>>()
            .map_err(CoreError::from)?;
        let count = entries.len();
        let bytes = match passphrase {
            Some(passphrase) => backup::export_to_encrypted(entries, passphrase),
            None => backup::export_to_json(entries).map(String::into_bytes),
        }
        .map_err(|error| CoreError::from(error.to_string()))?;
        Ok((bytes, count))
    }
    pub fn export_file(&self, destination: &Path, passphrase: Option<&str>) -> CoreResult<usize> {
        self.history.ensure_export_destination(destination)?;
        let (bytes, count) = self.export_bytes(passphrase)?;
        crate::infrastructure::attachments::write_backup(destination, &bytes)
            .map_err(CoreError::from)?;
        Ok(count)
    }
    pub fn import_bytes(
        &self,
        bytes: &[u8],
        passphrase: Option<&str>,
        mode: ImportMode,
    ) -> CoreResult<usize> {
        let json = match passphrase {
            Some(passphrase) => backup::decrypt_to_json(bytes, passphrase)
                .map_err(|error| CoreError::from(error.to_string()))?,
            None => String::from_utf8(bytes.to_vec())
                .map_err(|_| CoreError::validation("backup is not valid UTF-8"))?,
        };
        let entries =
            backup::entries_from_json(&json).map_err(|error| CoreError::from(error.to_string()))?;
        self.apply(&entries, mode)?;
        Ok(entries.len())
    }
    pub fn apply(&self, entries: &[ExportEntry], mode: ImportMode) -> CoreResult<()> {
        backup::apply_entries(&self.history.repository, entries, mode).map_err(CoreError::from)
    }
}
