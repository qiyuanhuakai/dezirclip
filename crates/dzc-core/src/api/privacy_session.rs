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
    intent: PrivacyAccessIntent,
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
    pub(super) fn require_copy(&self) -> PrivacyResult<()> {
        if self.intent != PrivacyAccessIntent::SelectedCopy {
            return Err(PrivacyError::SensitiveAccessDenied);
        }
        Ok(())
    }
    fn allows_body(&self) -> bool {
        self.intent != PrivacyAccessIntent::RestrictedSummary
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

    pub fn toggle(&self, now_ms: i64) -> PrivacyResult<RuntimeSnapshot> {
        let mut conn = self.connection.lock().map_err(unavailable)?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(unavailable)?;
        let state = read_state(&tx, now_ms)?;
        tx.execute("UPDATE privacy_runtime_state SET session_enabled=?1,session_expires_at_ms=NULL,privacy_epoch=privacy_epoch+1 WHERE singleton=1",
            [!state.snapshot.session.active]).map_err(unavailable)?;
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
            intent,
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

fn session_entry_visible(entry: &ClipboardEntry) -> bool {
    let Ok(tags) = serde_json::to_string(&entry.tags) else {
        return false;
    };
    crate::infrastructure::repository::migrations::search_visible(
        Some(&tags),
        [
            Some(&entry.content),
            Some(&entry.preview),
            entry.html_content.as_deref(),
            entry.ocr_text.as_deref(),
        ],
    )
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
        if !permit.allows_body() {
            return Err(PrivacyError::SensitiveAccessDenied);
        }
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

    pub fn authorize_session_entry(
        &self,
        entry: &ClipboardEntry,
        permit: &PrivacyReadPermit,
        now_ms: i64,
    ) -> PrivacyResult<ClipboardEntry> {
        if !permit.allows_body() {
            return Err(PrivacyError::SensitiveAccessDenied);
        }
        self.privacy().check_read(permit, now_ms)?;
        if entry.id >= 0 || (!permit.includes_sensitive() && !session_entry_visible(entry)) {
            return Err(PrivacyError::SensitiveAccessDenied);
        }
        for field in [
            Some(entry.content.as_str()),
            Some(entry.preview.as_str()),
            entry.html_content.as_deref(),
            entry.ocr_text.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if matches!(
                crate::infrastructure::encryption::decode_stored_text(field),
                crate::infrastructure::encryption::StoredText::Failed
            ) {
                return Err(PrivacyError::SensitiveAccessDenied);
            }
        }
        let result = entry.clone();
        self.privacy().check_read(permit, now_ms)?;
        Ok(result)
    }

    pub fn summaries_for_session_authorized(
        &self,
        entries: &[ClipboardEntry],
        query: &SummaryQuery,
        permit: &PrivacyReadPermit,
        now_ms: i64,
    ) -> PrivacyResult<SearchResult> {
        query.validate().map_err(unavailable)?;
        self.privacy().check_read(permit, now_ms)?;
        let result = SearchResult {
            entries: entries
                .iter()
                .filter(|entry| {
                    entry.id < 0
                        && session_entry_visible(entry)
                        && query
                            .kind
                            .as_ref()
                            .is_none_or(|kind| &entry.content_type == kind)
                        && query
                            .tag
                            .as_ref()
                            .is_none_or(|tag| entry.tags.contains(tag))
                        && (!query.pinned || entry.is_pinned)
                })
                .skip(query.offset as usize)
                .take(query.limit as usize)
                .cloned()
                .map(super::summary::summarize)
                .collect(),
        };
        self.privacy().check_read(permit, now_ms)?;
        Ok(result)
    }

    /// Uses the same matcher and SQLite trigram tokenizer as persistent history, only in RAM.
    pub fn search_summaries_for_session_authorized(
        &self,
        entries: &[ClipboardEntry],
        request: &SearchRequest,
        permit: &PrivacyReadPermit,
        now_ms: i64,
    ) -> PrivacyResult<SearchResult> {
        request.validate().map_err(unavailable)?;
        self.privacy().check_read(permit, now_ms)?;
        let eligible: Vec<_> = entries
            .iter()
            .filter(|entry| entry.id < 0 && session_entry_visible(entry))
            .collect();
        let mut matches = std::collections::HashSet::new();
        if request.mode == super::SummarySearchMode::Fts5 {
            let conn = rusqlite::Connection::open_in_memory().map_err(unavailable)?;
            conn.execute_batch("CREATE VIRTUAL TABLE session_fts USING fts5(content,preview,source_app,content_kinds,ocr_text,tokenize='trigram')").map_err(unavailable)?;
            for entry in &eligible {
                conn.execute("INSERT INTO session_fts(rowid,content,preview,source_app,content_kinds,ocr_text) VALUES(?1,?2,?3,?4,?5,?6)", rusqlite::params![entry.id,entry.content,entry.preview,entry.source_app,serde_json::to_string(&entry.content_kinds).map_err(unavailable)?,entry.ocr_text]).map_err(unavailable)?;
            }
            let mut statement = conn
                .prepare("SELECT rowid FROM session_fts WHERE session_fts MATCH ?1")
                .map_err(unavailable)?;
            let rows = statement
                .query_map(
                    [crate::services::search::escape_fts5_term(&request.query)],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(unavailable)?;
            for id in rows {
                matches.insert(id.map_err(unavailable)?);
            }
        } else {
            let regex = if request.mode == super::SummarySearchMode::Regex {
                Some(regex::Regex::new(&request.query).map_err(unavailable)?)
            } else {
                None
            };
            let mode = if request.mode == super::SummarySearchMode::Fuzzy {
                "fuzzy"
            } else {
                "contains"
            };
            for entry in &eligible {
                if crate::infrastructure::repository::cli_query::matches_fields(
                    &request.query,
                    mode,
                    regex.as_ref(),
                    &entry.content,
                    &entry.source_app,
                    &serde_json::to_string(&entry.tags).map_err(unavailable)?,
                    entry.ocr_text.as_deref().unwrap_or(""),
                ) {
                    matches.insert(entry.id);
                }
            }
        }
        let selected: Vec<_> = eligible
            .into_iter()
            .filter(|entry| matches.contains(&entry.id))
            .cloned()
            .collect();
        self.summaries_for_session_authorized(&selected, &request.page, permit, now_ms)
    }

    /// Host callers obtain a fresh permission for each request, never from a client role string.
    pub fn get_entry_authorized(
        &self,
        id: i64,
        permit: &PrivacyReadPermit,
        now_ms: i64,
    ) -> PrivacyResult<Option<ClipboardEntry>> {
        if !permit.allows_body() {
            return Err(PrivacyError::SensitiveAccessDenied);
        }
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
    #[test]
    fn session_permissions_use_the_same_visibility_rule_before_paging_and_epoch_checks() {
        let history = memory();
        let permit = history
            .privacy()
            .authorize_read(
                PrivacyAccessIntent::AutomationRead,
                SensitiveAccess::Denied,
                None,
                1,
            )
            .unwrap();
        let mut entry: ClipboardEntry = serde_json::from_value(serde_json::json!({"id":-1,"content_type":"text","content":"ordinary","preview":"summary","source_app":"test","timestamp":1,"is_pinned":false})).unwrap();
        let ordinary = entry.clone();
        entry.tags = vec![" sensitive ".into()];
        let mut damaged = ordinary.clone();
        damaged.id = -2;
        damaged.ocr_text = Some("linux:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==".into());
        let entries = [entry.clone(), damaged, ordinary.clone()];
        let summary = history
            .summaries_for_session_authorized(
                &entries,
                &SummaryQuery {
                    limit: 1,
                    ..Default::default()
                },
                &permit,
                1,
            )
            .unwrap();
        assert_eq!(summary.entries.len(), 1);
        assert_eq!(summary.entries[0].preview.as_deref(), Some("summary"));
        assert_eq!(
            history
                .authorize_session_entry(&entry, &permit, 1)
                .unwrap_err(),
            PrivacyError::SensitiveAccessDenied
        );
        assert!(history
            .authorize_session_entry(&ordinary, &permit, 1)
            .is_ok());
        history
            .privacy()
            .set_session(PrivacySessionCommand::On { duration_ms: None }, 1)
            .unwrap();
        assert_eq!(
            history
                .summaries_for_session_authorized(&entries, &SummaryQuery::default(), &permit, 1)
                .unwrap_err(),
            PrivacyError::PrivacyEpochChanged
        );
    }
    #[test]
    fn restricted_summary_permit_cannot_be_reused_to_read_complete_bodies() {
        let history = memory();
        let permit = history
            .privacy()
            .authorize_read(
                PrivacyAccessIntent::RestrictedSummary,
                SensitiveAccess::Denied,
                None,
                1,
            )
            .unwrap();
        assert_eq!(
            history.get_entry_authorized(1, &permit, 1).unwrap_err(),
            PrivacyError::SensitiveAccessDenied
        );
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
                .unwrap_err(),
            PrivacyError::SensitiveAccessDenied
        );
        assert!(history
            .summaries_authorized(&SummaryQuery::default(), &permit, 1)
            .is_ok());
    }
    #[test]
    fn concurrent_toggle_requests_are_serialized_in_the_shared_database() {
        let directory = std::env::temp_dir().join(format!("dzc-toggle-{}", uuid::Uuid::new_v4()));
        let path = directory.join("synthetic.db");
        let first = HistoryService::open(&path).unwrap();
        let other = HistoryService::open(&path).unwrap();
        let first_service = first.privacy();
        let other_service = other.privacy();
        std::thread::scope(|scope| {
            let a = scope.spawn(|| first_service.toggle(1).unwrap());
            let b = scope.spawn(|| other_service.toggle(1).unwrap());
            a.join().unwrap();
            b.join().unwrap();
        });
        let snapshot = first.runtime_snapshot(1).unwrap();
        assert!(!snapshot.session.active);
        assert_eq!(snapshot.privacy_epoch, 2);
        drop((first_service, other_service, first, other));
        std::fs::remove_dir_all(directory).unwrap();
    }
}
