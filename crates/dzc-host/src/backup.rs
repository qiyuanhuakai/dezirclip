use crate::{router::{now_ms, privacy_error}, Host, Session};
use dzc_core::api::{BackupService, CaptureTicket, CoreError, CoreErrorCode, ImportMode};
use dzc_protocol::wire::*;
use serde_json::Value;
use std::path::Path;

fn failure(error: CoreError) -> WireErrorCode {
    match error.code {
        CoreErrorCode::PrivacyEpochChanged | CoreErrorCode::PrivacySessionActive |
        CoreErrorCode::PrivacyStateUnavailable => privacy_error(error),
        CoreErrorCode::Validation => WireErrorCode::InvalidRequest,
        _ if error.to_string() == "SensitiveBackupPassphraseRequired" => WireErrorCode::SensitiveAccessDenied,
        _ => WireErrorCode::Unavailable,
    }
}

impl Host {
    fn backup_ticket(&self, session: &Session, instance: &str, epoch: u64, path: &str) -> Result<CaptureTicket, WireErrorCode> {
        self.desktop_permission(session, instance)?;
        if !Path::new(path).is_absolute() { return Err(WireErrorCode::InvalidRequest); }
        let ticket = self.history.privacy().capture_ticket(now_ms()?).map_err(privacy_error)?;
        if ticket.epoch() != epoch { return Err(WireErrorCode::PrivacyEpochChanged); }
        Ok(ticket)
    }

    pub(crate) fn backup(&self, session: &Session, method: Method, params: Value) -> Result<Value, WireErrorCode> {
        let _operation = self.backup_operation.try_lock().map_err(|_| WireErrorCode::Busy)?;
        if self.is_stopping() { return Err(WireErrorCode::Busy); }
        let service = BackupService::new(self.history.clone());
        let invalid = |_| WireErrorCode::InvalidRequest;
        match method {
            Method::DesktopBackupPreview => {
                let p: BackupPreviewParams = serde_json::from_value(params).map_err(invalid)?;
                let ticket = self.backup_ticket(session, &p.service_instance_id, p.expected_epoch, &p.path)?;
                let rows = service.preview_file(Path::new(&p.path), &ticket).map_err(failure)?;
                let entries = rows.into_iter().map(|content| BackupPreviewRow { content }).collect();
                serde_json::to_value(BackupPreview { entries })
            }
            Method::DesktopBackupImport => {
                let p: BackupImportParams = serde_json::from_value(params).map_err(invalid)?;
                let mode = match p.mode.as_str() {
                    "merge" => ImportMode::Merge,
                    "replace" => ImportMode::Replace,
                    _ => return Err(WireErrorCode::InvalidRequest),
                };
                let ticket = self.backup_ticket(session, &p.service_instance_id, p.expected_epoch, &p.path)?;
                let count = service.import_file_authorized(Path::new(&p.path), p.passphrase.as_deref(), mode, &ticket).map_err(failure)?;
                self.publish(EventKind::HistoryReset, EventData { entry_id: None, status: None })?;
                serde_json::to_value(BackupImportResult { imported: count as u64, skipped: 0, mode: p.mode })
            }
            Method::DesktopBackupExport => {
                let p: BackupExportParams = serde_json::from_value(params).map_err(invalid)?;
                let password = match p.format.as_str() {
                    "json" => None,
                    "encrypted" => Some(p.passphrase.as_deref().ok_or(WireErrorCode::InvalidRequest)?),
                    _ => return Err(WireErrorCode::InvalidRequest),
                };
                let ticket = self.backup_ticket(session, &p.service_instance_id, p.expected_epoch, &p.path)?;
                let count = service.export_file_authorized(Path::new(&p.path), password, &ticket).map_err(failure)?;
                serde_json::to_value(BackupExportResult { count: count as u64, encrypted: password.is_some(), format: p.format, path: p.path })
            }
            _ => return Err(WireErrorCode::InvalidRequest),
        }.map_err(|_| WireErrorCode::Internal)
    }
}
