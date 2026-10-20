//! Persistent privacy state and opaque, epoch-bound permissions for trusted hosts.
use super::{
    ClipboardEntry, HistoryQuery, HistoryService, SearchRequest, SearchResult, SummaryQuery,
};
use rusqlite::{Connection, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

pub const MAX_PRIVACY_DURATION_MS: u64 = 30 * 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyError {
    PrivacyStateUnavailable,
    PrivacyEpochChanged,
    PrivacySessionActive,
    SensitiveAccessDenied,
    ManualPersistenceConfirmationRequired,
    PrivacyDurationInvalid,
}
impl PrivacyError {
    pub fn code(self) -> &'static str {
        match self {
            Self::PrivacyStateUnavailable => "PrivacyStateUnavailable",
            Self::PrivacyEpochChanged => "PrivacyEpochChanged",
            Self::PrivacySessionActive => "PrivacySessionActive",
            Self::SensitiveAccessDenied => "SensitiveAccessDenied",
            Self::ManualPersistenceConfirmationRequired => "ManualPersistenceConfirmationRequired",
            Self::PrivacyDurationInvalid => "PrivacyDurationInvalid",
        }
    }
}
impl std::fmt::Display for PrivacyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}
impl std::error::Error for PrivacyError {}
pub type PrivacyResult<T> = Result<T, PrivacyError>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivacySessionStatus {
    pub active: bool,
    pub expires_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeSnapshot {
    pub history_revision: u64,
    pub privacy_epoch: u64,
    pub session: PrivacySessionStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivacySessionCommand {
    On { duration_ms: Option<u64> },
    Off,
}

/// Constructed only after checking this database's current persistent state.
#[derive(Debug, Clone, Copy)]
pub struct CaptureTicket {
    state_id: uuid::Uuid,
    epoch: u64,
}
impl CaptureTicket {
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManualPersistenceIntent {
    Ordinary,
    ConfirmedDuringPrivacySession,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivacyAccessIntent {
    RestrictedSummary,
    AutomationRead,
    InteractiveRead,
    SelectedCopy,
}

/// A host chooses this only after authenticating and authorizing the request.
/// This type has no wire deserializer and grants no client role by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensitiveAccess {
    Denied,
    ExplicitUserAction,
}

#[derive(Debug, Clone)]
pub struct PrivacyReadPermit {
    ticket: CaptureTicket,
    sensitive: bool,
}
impl PrivacyReadPermit {
    pub fn epoch(&self) -> u64 {
        self.ticket.epoch
    }
    pub fn includes_sensitive(&self) -> bool {
        self.sensitive
    }
}

struct StoredState {
    snapshot: RuntimeSnapshot,
    state_id: uuid::Uuid,
    enabled: bool,
}

fn unavailable<T>(_: T) -> PrivacyError {
    PrivacyError::PrivacyStateUnavailable
}

fn read_state(conn: &Connection, now_ms: i64) -> PrivacyResult<StoredState> {
    if now_ms < 0 {
        return Err(PrivacyError::PrivacyStateUnavailable);
    }
    conn.query_row(
        "SELECT state_id,history_revision,privacy_epoch,session_enabled,session_expires_at_ms
        FROM privacy_runtime_state WHERE singleton=1",
        [],
        |row| {
            let state: String = row.get(0)?;
            let state_id = uuid::Uuid::parse_str(&state).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;
            let history: i64 = row.get(1)?;
            let epoch: i64 = row.get(2)?;
            let history_revision = u64::try_from(history)
                .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(1, history))?;
            let privacy_epoch = u64::try_from(epoch)
                .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(2, epoch))?;
            let enabled: bool = row.get(3)?;
            let expires_at_ms: Option<i64> = row.get(4)?;
            Ok(StoredState {
                state_id,
                enabled,
                snapshot: RuntimeSnapshot {
                    history_revision,
                    privacy_epoch,
                    session: PrivacySessionStatus {
                        active: enabled && expires_at_ms.is_none_or(|end| end > now_ms),
                        expires_at_ms,
                    },
                },
            })
        },
    )
    .map_err(unavailable)
}

#[derive(Clone)]
pub struct PrivacySessionService {
    connection: Arc<Mutex<Connection>>,
    invalidate_epoch: Arc<dyn Fn(u64) + Send + Sync>,
}

impl PrivacySessionService {
    pub(crate) fn new(
        connection: Arc<Mutex<Connection>>,
        invalidate_epoch: Arc<dyn Fn(u64) + Send + Sync>,
    ) -> Self {
        Self {
            connection,
            invalidate_epoch,
        }
    }

    /// Expiration is a persisted state transition; a restart keeps an indefinite session active.
    pub fn status(&self, now_ms: i64) -> PrivacyResult<RuntimeSnapshot> {
        let mut conn = self.connection.lock().map_err(unavailable)?;
        let state = read_state(&conn, now_ms)?;
        if !state.enabled || state.snapshot.session.active {
            let snapshot = state.snapshot;
            drop(conn);
            (self.invalidate_epoch)(snapshot.privacy_epoch);
            return Ok(snapshot);
        }
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(unavailable)?;
        let current = read_state(&tx, now_ms)?;
        if current.enabled && !current.snapshot.session.active {
            tx.execute(
                "UPDATE privacy_runtime_state SET session_enabled=0,session_expires_at_ms=NULL,
                privacy_epoch=privacy_epoch+1 WHERE singleton=1",
                [],
            )
            .map_err(unavailable)?;
        }
        let result = read_state(&tx, now_ms)?.snapshot;
        tx.commit().map_err(unavailable)?;
        drop(conn);
        (self.invalidate_epoch)(result.privacy_epoch);
        Ok(result)
    }

    pub fn set_session(
        &self,
        command: PrivacySessionCommand,
        now_ms: i64,
    ) -> PrivacyResult<RuntimeSnapshot> {
        if now_ms < 0 {
            return Err(PrivacyError::PrivacyDurationInvalid);
        }
        let (enabled, expires_at_ms) = match command {
            PrivacySessionCommand::Off => (false, None),
            PrivacySessionCommand::On { duration_ms: None } => (true, None),
            PrivacySessionCommand::On {
                duration_ms: Some(duration),
            } => {
                if duration == 0 || duration > MAX_PRIVACY_DURATION_MS {
                    return Err(PrivacyError::PrivacyDurationInvalid);
                }
                let end = now_ms
                    .checked_add(duration as i64)
                    .ok_or(PrivacyError::PrivacyDurationInvalid)?;
                (true, Some(end))
            }
        };
        let mut conn = self.connection.lock().map_err(unavailable)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(unavailable)?;
        let current = read_state(&tx, now_ms)?;
        if current.enabled != enabled || current.snapshot.session.expires_at_ms != expires_at_ms {
            tx.execute(
                "UPDATE privacy_runtime_state SET session_enabled=?1,session_expires_at_ms=?2,
                privacy_epoch=privacy_epoch+1 WHERE singleton=1",
                rusqlite::params![enabled, expires_at_ms],
            )
            .map_err(unavailable)?;
        }
        let result = read_state(&tx, now_ms)?.snapshot;
        tx.commit().map_err(unavailable)?;
        drop(conn);
        (self.invalidate_epoch)(result.privacy_epoch);
        Ok(result)
    }

    pub fn capture_ticket(&self, now_ms: i64) -> PrivacyResult<CaptureTicket> {
        self.status(now_ms)?;
        let conn = self.connection.lock().map_err(unavailable)?;
        let state = read_state(&conn, now_ms)?;
        if state.snapshot.session.active {
            return Err(PrivacyError::PrivacySessionActive);
        }
        Ok(CaptureTicket {
            state_id: state.state_id,
            epoch: state.snapshot.privacy_epoch,
        })
    }

    pub fn check_capture(&self, ticket: &CaptureTicket, now_ms: i64) -> PrivacyResult<()> {
        self.status(now_ms)?;
        let conn = self.connection.lock().map_err(unavailable)?;
        check_capture_with_conn(&conn, ticket, now_ms)
    }

    pub fn authorize_read(
        &self,
        intent: PrivacyAccessIntent,
        sensitive: SensitiveAccess,
        expected_epoch: Option<u64>,
        now_ms: i64,
    ) -> PrivacyResult<PrivacyReadPermit> {
        self.status(now_ms)?;
        let conn = self.connection.lock().map_err(unavailable)?;
        let state = read_state(&conn, now_ms)?;
        if expected_epoch.is_some_and(|epoch| epoch != state.snapshot.privacy_epoch) {
            return Err(PrivacyError::PrivacyEpochChanged);
        }
        let sensitive = match (intent, sensitive) {
            (
                PrivacyAccessIntent::InteractiveRead | PrivacyAccessIntent::SelectedCopy,
                SensitiveAccess::ExplicitUserAction,
            ) => true,
            (_, SensitiveAccess::ExplicitUserAction) => {
                return Err(PrivacyError::SensitiveAccessDenied)
            }
            (_, SensitiveAccess::Denied) => false,
        };
        Ok(PrivacyReadPermit {
            ticket: CaptureTicket {
                state_id: state.state_id,
                epoch: state.snapshot.privacy_epoch,
            },
            sensitive,
        })
    }

    pub fn check_read(&self, permit: &PrivacyReadPermit, now_ms: i64) -> PrivacyResult<()> {
        self.status(now_ms)?;
        let conn = self.connection.lock().map_err(unavailable)?;
        let state = read_state(&conn, now_ms)?;
        if permit.ticket.state_id != state.state_id
            || permit.ticket.epoch != state.snapshot.privacy_epoch
        {
            return Err(PrivacyError::PrivacyEpochChanged);
        }
        Ok(())
    }
}

pub(crate) fn check_manual_with_conn(
    conn: &Connection,
    intent: ManualPersistenceIntent,
    now_ms: i64,
) -> PrivacyResult<()> {
    let state = read_state(conn, now_ms)?;
    if state.snapshot.session.active
        && intent != ManualPersistenceIntent::ConfirmedDuringPrivacySession
    {
        return Err(PrivacyError::ManualPersistenceConfirmationRequired);
    }
    Ok(())
}

/// Called by automatic capture/OCR writers inside their existing write transaction.
pub(crate) fn check_capture_with_conn(
    conn: &Connection,
    ticket: &CaptureTicket,
    now_ms: i64,
) -> PrivacyResult<()> {
    let state = read_state(conn, now_ms)?;
    if state.snapshot.session.active {
        return Err(PrivacyError::PrivacySessionActive);
    }
    if ticket.state_id != state.state_id || ticket.epoch != state.snapshot.privacy_epoch {
        return Err(PrivacyError::PrivacyEpochChanged);
    }
    Ok(())
}

impl HistoryService {
    /// A trusted host maps an explicit user confirmation to this intent, never a client role.
    pub fn save_manual(
        &self,
        entry: &ClipboardEntry,
        directory: Option<&std::path::Path>,
        intent: ManualPersistenceIntent,
        now_ms: i64,
    ) -> super::CoreResult<i64> {
        self.privacy()
            .status(now_ms)
            .map_err(|error| super::CoreError::from(error.to_string()))?;
        self.repository
            .save_manual(entry, directory, intent, now_ms)
            .map_err(Into::into)
    }
    pub fn privacy(&self) -> PrivacySessionService {
        self.repository.privacy_service()
    }
    pub fn runtime_snapshot(&self, now_ms: i64) -> PrivacyResult<RuntimeSnapshot> {
        self.privacy().status(now_ms)
    }

    pub fn query_authorized(
        &self,
        query: &HistoryQuery<'_>,
        permit: &PrivacyReadPermit,
        now_ms: i64,
    ) -> PrivacyResult<Vec<ClipboardEntry>> {
        if !(1..=1000).contains(&query.limit) || query.offset < 0 {
            return Err(PrivacyError::SensitiveAccessDenied);
        }
        self.privacy().check_read(permit, now_ms)?;
        let mut restricted = query.clone();
        restricted.exclude_sensitive = query.exclude_sensitive || !permit.includes_sensitive();
        let result = self.query(&restricted).map_err(unavailable)?;
        self.privacy().check_read(permit, now_ms)?;
        Ok(result)
    }

    /// Host callers obtain a fresh permission for each request, never from a client role string.
    pub fn get_entry_authorized(
        &self,
        id: i64,
        permit: &PrivacyReadPermit,
        now_ms: i64,
    ) -> PrivacyResult<Option<ClipboardEntry>> {
        let privacy = self.privacy();
        privacy.check_read(permit, now_ms)?;
        let entries = self
            .query(&HistoryQuery {
                id: Some(id),
                limit: 1,
                exclude_sensitive: !permit.includes_sensitive(),
                ..Default::default()
            })
            .map_err(unavailable)?;
        privacy.check_read(permit, now_ms)?;
        Ok(entries.into_iter().next())
    }

    pub fn summaries_authorized(
        &self,
        query: &SummaryQuery,
        permit: &PrivacyReadPermit,
        now_ms: i64,
    ) -> PrivacyResult<SearchResult> {
        self.privacy().check_read(permit, now_ms)?;
        let result = self.summaries(query).map_err(unavailable)?;
        self.privacy().check_read(permit, now_ms)?;
        Ok(result)
    }

    pub fn search_summaries_authorized(
        &self,
        query: &SearchRequest,
        permit: &PrivacyReadPermit,
        now_ms: i64,
    ) -> PrivacyResult<SearchResult> {
        self.privacy().check_read(permit, now_ms)?;
        let result = self.search_summaries(query).map_err(unavailable)?;
        self.privacy().check_read(permit, now_ms)?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{HistoryService, SummaryQuery};
    use std::path::Path;

    fn memory() -> HistoryService {
        HistoryService::open(Path::new(":memory:")).unwrap()
    }

    #[test]
    fn persistent_state_survives_reopen_and_expiration_is_monotonic() {
        let dir = std::env::temp_dir().join(format!("dzc-privacy-state-{}", uuid::Uuid::new_v4()));
        let path = dir.join("synthetic.db");
        let first = HistoryService::open(&path).unwrap();
        let active = first
            .privacy()
            .set_session(PrivacySessionCommand::On { duration_ms: None }, 100)
            .unwrap();
        assert!(active.session.active);
        drop(first);
        let reopened = HistoryService::open(&path).unwrap();
        assert_eq!(reopened.runtime_snapshot(100_000).unwrap(), active);
        let timed = reopened
            .privacy()
            .set_session(
                PrivacySessionCommand::On {
                    duration_ms: Some(900_000),
                },
                100_000,
            )
            .unwrap();
        assert_eq!(timed.session.expires_at_ms, Some(1_000_000));
        assert!(reopened.runtime_snapshot(999_999).unwrap().session.active);
        let expired = reopened.runtime_snapshot(1_000_000).unwrap();
        assert!(!expired.session.active);
        assert_eq!(expired.session.expires_at_ms, None);
        assert_eq!(expired.privacy_epoch, timed.privacy_epoch + 1);
        assert_eq!(reopened.runtime_snapshot(1_000_001).unwrap(), expired);
        drop(reopened);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn independent_connections_share_epoch_and_revoke_capture_and_read_permissions() {
        let dir = std::env::temp_dir().join(format!("dzc-privacy-epoch-{}", uuid::Uuid::new_v4()));
        let path = dir.join("synthetic.db");
        let first = HistoryService::open(&path).unwrap();
        let other = HistoryService::open(&path).unwrap();
        let capture = first.privacy().capture_ticket(100).unwrap();
        let read = first
            .privacy()
            .authorize_read(
                PrivacyAccessIntent::RestrictedSummary,
                SensitiveAccess::Denied,
                Some(capture.epoch()),
                100,
            )
            .unwrap();
        other
            .privacy()
            .set_session(PrivacySessionCommand::On { duration_ms: None }, 100)
            .unwrap();
        assert_eq!(
            first.privacy().check_capture(&capture, 100),
            Err(PrivacyError::PrivacySessionActive)
        );
        assert_eq!(
            first.summaries_authorized(&SummaryQuery::default(), &read, 100),
            Err(PrivacyError::PrivacyEpochChanged)
        );
        other
            .privacy()
            .set_session(PrivacySessionCommand::Off, 101)
            .unwrap();
        assert_eq!(
            first.privacy().check_capture(&capture, 101),
            Err(PrivacyError::PrivacyEpochChanged)
        );
        let renewed = first.privacy().capture_ticket(101).unwrap();
        first.privacy().check_capture(&renewed, 101).unwrap();
        let unrelated = memory();
        assert_eq!(
            unrelated.privacy().check_capture(&renewed, 101),
            Err(PrivacyError::PrivacyEpochChanged)
        );
        drop((first, other));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn history_revision_and_privacy_settings_are_atomic_with_mutations() {
        let history = memory();
        let connection = history.privacy().connection.clone();
        let mut conn = connection.lock().unwrap();
        let before = read_state(&conn, 1).unwrap().snapshot;
        let transaction = conn.transaction().unwrap();
        transaction
            .execute(
                "INSERT INTO clipboard_history(content_type,content,preview,source_app,timestamp)
            VALUES('text','synthetic data','synthetic preview','test',1)",
                [],
            )
            .unwrap();
        transaction.execute("INSERT OR REPLACE INTO settings(key,value) VALUES('app.excluded_source_apps','fixture')", []).unwrap();
        let during = read_state(&transaction, 1).unwrap().snapshot;
        assert!(during.history_revision > before.history_revision);
        assert!(during.privacy_epoch > before.privacy_epoch);
        transaction.rollback().unwrap();
        assert_eq!(read_state(&conn, 1).unwrap().snapshot, before);
        conn.execute(
            "INSERT INTO clipboard_history(content_type,content,preview,source_app,timestamp)
            VALUES('text','committed synthetic','summary','test',1)",
            [],
        )
        .unwrap();
        assert!(read_state(&conn, 1).unwrap().snapshot.history_revision > before.history_revision);
    }

    #[test]
    fn failed_session_commit_cannot_report_protection_enabled() {
        let history = memory();
        let service = history.privacy();
        let before = service.status(1).unwrap();
        service
            .connection
            .lock()
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER reject_session BEFORE UPDATE ON privacy_runtime_state
            BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;",
            )
            .unwrap();
        assert_eq!(
            service.set_session(PrivacySessionCommand::On { duration_ms: None }, 1),
            Err(PrivacyError::PrivacyStateUnavailable)
        );
        assert_eq!(service.status(1).unwrap(), before);
        for duration in [0, MAX_PRIVACY_DURATION_MS + 1] {
            assert_eq!(
                service.set_session(
                    PrivacySessionCommand::On {
                        duration_ms: Some(duration)
                    },
                    1
                ),
                Err(PrivacyError::PrivacyDurationInvalid)
            );
        }
    }

    #[test]
    fn automation_never_self_grants_sensitive_access_and_state_loss_fails_closed() {
        let history = memory();
        assert_eq!(
            history
                .privacy()
                .authorize_read(
                    PrivacyAccessIntent::AutomationRead,
                    SensitiveAccess::ExplicitUserAction,
                    None,
                    1
                )
                .unwrap_err(),
            PrivacyError::SensitiveAccessDenied
        );
        assert_eq!(
            history
                .privacy()
                .authorize_read(
                    PrivacyAccessIntent::RestrictedSummary,
                    SensitiveAccess::ExplicitUserAction,
                    None,
                    1
                )
                .unwrap_err(),
            PrivacyError::SensitiveAccessDenied
        );
        let read = history
            .privacy()
            .authorize_read(
                PrivacyAccessIntent::InteractiveRead,
                SensitiveAccess::ExplicitUserAction,
                None,
                1,
            )
            .unwrap();
        assert!(read.includes_sensitive());
        history
            .privacy()
            .connection
            .lock()
            .unwrap()
            .execute("DELETE FROM privacy_runtime_state", [])
            .unwrap();
        assert_eq!(
            history.privacy().status(1),
            Err(PrivacyError::PrivacyStateUnavailable)
        );
        assert_eq!(
            history.privacy().check_read(&read, 1),
            Err(PrivacyError::PrivacyStateUnavailable)
        );
    }
    #[test]
    fn manual_persistence_needs_explicit_confirmation_without_disabling_the_session() {
        let history = memory();
        let entry: ClipboardEntry = serde_json::from_value(serde_json::json!({"id":0,"content_type":"text","content":"synthetic manual body","preview":"summary","source_app":"test","timestamp":1,"is_pinned":false})).unwrap();
        history
            .privacy()
            .set_session(PrivacySessionCommand::On { duration_ms: None }, 1)
            .unwrap();
        assert_eq!(
            history
                .save_manual(&entry, None, ManualPersistenceIntent::Ordinary, 1)
                .unwrap_err()
                .to_string(),
            "ManualPersistenceConfirmationRequired"
        );
        assert_eq!(history.get_count().unwrap(), 0);
        history
            .save_manual(
                &entry,
                None,
                ManualPersistenceIntent::ConfirmedDuringPrivacySession,
                1,
            )
            .unwrap();
        assert!(history.runtime_snapshot(1).unwrap().session.active);
        assert_eq!(history.get_count().unwrap(), 1);
        let permit = history
            .privacy()
            .authorize_read(
                PrivacyAccessIntent::AutomationRead,
                SensitiveAccess::Denied,
                None,
                1,
            )
            .unwrap();
        assert_eq!(
            history
                .query_authorized(
                    &HistoryQuery {
                        limit: 1,
                        ..Default::default()
                    },
                    &permit,
                    1
                )
                .unwrap()
                .len(),
            1
        );
        history
            .privacy()
            .set_session(PrivacySessionCommand::Off, 2)
            .unwrap();
        assert_eq!(
            history
                .query_authorized(
                    &HistoryQuery {
                        limit: 1,
                        ..Default::default()
                    },
                    &permit,
                    2
                )
                .unwrap_err(),
            PrivacyError::PrivacyEpochChanged
        );
    }
}
