use super::{clipboard_repo::SqliteClipboardRepository, settings_repo::SqliteSettingsRepository};
use crate::infrastructure::encryption;
use crate::{
    api::{PrivacyDecisionReport, PrivacyRuleKind, PrivacyRuleSummary, PrivacyScanStatus},
    domain::models::ClipboardEntry,
    privacy::{
        EvidenceLevel, EvidenceSource, PrivacyDecision, PrivacyEvidence, PrivacyPolicy,
        PrivacyReason, ScanBudget,
    },
};
use rand::RngCore;
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
const MAX_RULES: usize = 128;
const MAX_RULE_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredRule {
    pub id: String,
    pub version: u8,
    pub kind: PrivacyRuleKind,
    pub source_app: Option<String>,
    pub salt: [u8; 16],
    pub digest: Option<[u8; 32]>,
    pub origin_entry_id: Option<i64>,
    pub originally_protected: bool,
    pub created_at_ms: i64,
}
fn unavailable<T>(_: T) -> String {
    "PrivacyRulesUnavailable".into()
}
fn source_key(value: &str) -> String {
    value.trim().to_lowercase()
}
fn source_matches(rule: &StoredRule, source: &str) -> bool {
    rule.source_app
        .as_deref()
        .is_none_or(|value| source_key(value) == source_key(source))
}
fn content_digest(entry: &ClipboardEntry) -> Result<[u8; 32], String> {
    if entry
        .content
        .len()
        .saturating_add(entry.html_content.as_deref().map_or(0, str::len))
        .saturating_add(entry.ocr_text.as_deref().map_or(0, str::len))
        > 48 * 1024 * 1024
    {
        return Err("PrivacyRulePayloadTooLarge".into());
    }
    let normalized = crate::infrastructure::protected_resources::normalize(
        &entry.content_type,
        &entry.content,
        entry.html_content.as_deref(),
        entry.is_external,
    )?;
    if normalized
        .content
        .len()
        .saturating_add(normalized.html.as_deref().map_or(0, str::len))
        .saturating_add(entry.ocr_text.as_deref().map_or(0, str::len))
        > 48 * 1024 * 1024
    {
        return Err("PrivacyRulePayloadTooLarge".into());
    }
    let mut hash = Sha256::new();
    hash.update(b"dzc-exact-content-v1");
    for value in [
        Some(entry.content_type.as_str()),
        Some(normalized.content.as_str()),
        normalized.html.as_deref(),
        entry.ocr_text.as_deref(),
    ] {
        match value {
            Some(value) => {
                hash.update([1]);
                hash.update((value.len() as u64).to_le_bytes());
                hash.update(value.as_bytes());
            }
            None => hash.update([0]),
        }
    }
    Ok(hash.finalize().into())
}
fn salted_digest(digest: &[u8; 32], salt: &[u8; 16]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"dzc-exact-exception-v1");
    hash.update(salt);
    hash.update(digest);
    hash.finalize().into()
}
fn fingerprint(entry: &ClipboardEntry, salt: &[u8; 16]) -> Result<[u8; 32], String> {
    Ok(salted_digest(&content_digest(entry)?, salt))
}
impl StoredRule {
    fn summary(&self) -> PrivacyRuleSummary {
        PrivacyRuleSummary {
            id: self.id.clone(),
            kind: self.kind,
            source_app: self.source_app.clone(),
            created_at_ms: self.created_at_ms,
        }
    }
    fn matches_entry(&self, entry: &ClipboardEntry) -> Result<bool, String> {
        Ok(self.kind == PrivacyRuleKind::ExactException
            && source_matches(self, &entry.source_app)
            && self.digest == Some(fingerprint(entry, &self.salt)?))
    }
    fn evidence(&self, reason: PrivacyReason) -> PrivacyEvidence {
        PrivacyEvidence {
            source: EvidenceSource::UserControl,
            level: EvidenceLevel::Explicit,
            reason,
            rule_id: Some(self.id.clone()),
        }
    }
}
fn load_rules(conn: &Connection) -> Result<Vec<StoredRule>, String> {
    let mut statement=conn.prepare("SELECT substr(id,1,37),substr(encrypted_rule,1,16385),created_at_ms FROM privacy_rules ORDER BY created_at_ms,id LIMIT 129").map_err(unavailable)?;
    let mut rows = statement.query([]).map_err(unavailable)?;
    let mut rules = Vec::new();
    while let Some(row) = rows.next().map_err(unavailable)? {
        if rules.len() >= MAX_RULES {
            return Err("PrivacyRuleLimitExceeded".into());
        }
        let id: String = row.get(0).map_err(unavailable)?;
        let cipher: String = row.get(1).map_err(unavailable)?;
        let created: i64 = row.get(2).map_err(unavailable)?;
        if cipher.len() > MAX_RULE_BYTES || !encryption::has_ciphertext_envelope(&cipher) {
            return Err(unavailable(()));
        }
        let plaintext = encryption::decode_for_mutation(&cipher).map_err(unavailable)?;
        let rule: StoredRule = serde_json::from_str(&plaintext).map_err(unavailable)?;
        if rule.version != 1
            || rule.id != id
            || uuid::Uuid::parse_str(&id).is_err()
            || rule.created_at_ms != created
            || created < 0
            || rule.source_app.as_ref().is_some_and(|value| {
                value.len() > 1024 || value.trim().is_empty() || value.chars().any(char::is_control)
            })
            || rule.kind != PrivacyRuleKind::ExactException
                && (rule.source_app.is_none() || rule.digest.is_some())
            || rule.kind == PrivacyRuleKind::ExactException && rule.digest.is_none()
        {
            return Err(unavailable(()));
        }
        rules.push(rule);
    }
    Ok(rules)
}
struct Settings {
    enabled: bool,
    kinds: Vec<String>,
    custom: Vec<String>,
    excluded: Vec<String>,
}
fn settings(conn: &Connection) -> Result<Settings, String> {
    let raw = |key| SqliteSettingsRepository::get_raw(conn, key).map_err(unavailable);
    let enabled = match raw("app.privacy_protection")?.as_deref() {
        None | Some("true") => true,
        Some("false") => false,
        _ => return Err("PrivacySettingsInvalid".into()),
    };
    let kinds = raw("app.privacy_protection_kinds")?
        .unwrap_or_else(|| "phone,idcard,email,secret,password".into())
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect();
    let custom = raw("app.privacy_protection_custom_rules")?
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .take(33)
        .map(str::to_owned)
        .collect();
    let excluded = crate::services::capture::source_exclusion::parse_excluded_source_apps(
        &raw("app.excluded_source_apps")?.unwrap_or_default(),
    );
    Ok(Settings {
        enabled,
        kinds,
        custom,
        excluded,
    })
}
fn source_evidence(
    settings: &Settings,
    rules: &[StoredRule],
    source: &str,
    path: Option<&str>,
    include_exclusion: bool,
) -> Vec<PrivacyEvidence> {
    let mut evidence = Vec::new();
    if include_exclusion
        && crate::services::capture::source_exclusion::source_app_is_excluded(
            &settings.excluded,
            source,
            path,
        )
    {
        evidence.push(PrivacyEvidence::new(
            EvidenceSource::SourceFilter,
            EvidenceLevel::Explicit,
            PrivacyReason::SourceExcluded,
        ));
    }
    for rule in rules {
        if !source_matches(rule, source) {
            continue;
        }
        match rule.kind {
            PrivacyRuleKind::IgnoreSource if include_exclusion => {
                evidence.push(rule.evidence(PrivacyReason::SourceExcluded))
            }
            PrivacyRuleKind::ProtectSource => {
                evidence.push(rule.evidence(PrivacyReason::SourceProtectionRequested))
            }
            _ => (),
        }
    }
    evidence
}
pub(crate) fn assess_source(
    conn: &Connection,
    source: &crate::capture::CaptureSource,
) -> Result<PrivacyDecision, String> {
    let settings = settings(conn)?;
    let rules = load_rules(conn)?;
    Ok(PrivacyPolicy.decide(source_evidence(
        &settings,
        &rules,
        &source.app_name,
        source.process_path.as_deref(),
        true,
    )))
}
pub(crate) fn assess_entry(
    conn: &Connection,
    entry: &ClipboardEntry,
    include_exclusion: bool,
) -> Result<PrivacyDecisionReport, String> {
    let settings = settings(conn)?;
    let rules = load_rules(conn)?;
    let mut evidence = source_evidence(
        &settings,
        &rules,
        &entry.source_app,
        entry.source_app_path.as_deref(),
        include_exclusion,
    );
    evidence.push(PrivacyEvidence::new(
        EvidenceSource::PlatformHint,
        EvidenceLevel::Unavailable,
        PrivacyReason::PrivacyHintsUnavailable,
    ));
    if crate::database::has_sensitive_tag(&entry.tags) {
        evidence.push(PrivacyEvidence::new(
            EvidenceSource::UserControl,
            EvidenceLevel::Explicit,
            PrivacyReason::UserRequestedProtection,
        ));
    }
    let stored_protected = entry.id > 0 && conn.query_row(
        "SELECT NOT dc_search_visible(tags,content,preview,html_content,ocr_text) FROM clipboard_history WHERE id=?1",
        [entry.id], |row| row.get::<_,bool>(0),
    ).optional().map_err(unavailable)?.unwrap_or(false);
    if stored_protected {
        evidence.push(PrivacyEvidence::new(
            EvidenceSource::ExistingRecord,
            EvidenceLevel::Explicit,
            PrivacyReason::ExistingRecordProtected,
        ));
    }
    let matching: Vec<_> = rules
        .iter()
        .filter(|rule| {
            rule.kind == PrivacyRuleKind::ExactException && source_matches(rule, &entry.source_app)
        })
        .collect();
    if !matching.is_empty() {
        let digest = content_digest(entry)?;
        for rule in matching {
            if rule.digest == Some(salted_digest(&digest, &rule.salt)) {
                evidence.push(rule.evidence(PrivacyReason::ExactExceptionMatched));
            }
        }
    }
    let mut fields = Vec::new();
    if crate::database::is_text_type(&entry.content_type) {
        fields.push(entry.content.as_str());
        if let Some(html) = &entry.html_content {
            fields.push(html);
        }
    }
    if let Some(ocr) = &entry.ocr_text {
        fields.push(ocr);
    }
    let total_bytes = fields
        .iter()
        .map(|value| value.len())
        .fold(0usize, usize::saturating_add);
    let mut scanned_bytes = 0usize;
    let mut completed = true;
    let scan_status = if !settings.enabled {
        PrivacyScanStatus::Disabled
    } else if total_bytes == 0 {
        PrivacyScanStatus::NotApplicable
    } else {
        let budget = ScanBudget::default();
        let started = std::time::Instant::now();
        for field in fields {
            let duration = budget.max_duration.saturating_sub(started.elapsed());
            let remaining = budget.max_bytes.saturating_sub(scanned_bytes);
            if remaining == 0 || duration.is_zero() {
                completed = false;
                break;
            }
            let report = crate::privacy::scan_content(
                field,
                &settings.kinds,
                &settings.custom,
                ScanBudget {
                    max_bytes: remaining,
                    max_duration: duration,
                    ..budget
                },
            );
            scanned_bytes = scanned_bytes.saturating_add(report.scanned_bytes);
            completed &= report.completed;
            evidence.extend(report.evidence);
        }
        if !completed || scanned_bytes < total_bytes {
            evidence.push(PrivacyEvidence::new(
                EvidenceSource::ConfiguredRule,
                EvidenceLevel::Incomplete,
                PrivacyReason::ScanIncomplete,
            ));
            PrivacyScanStatus::Incomplete
        } else {
            PrivacyScanStatus::Complete
        }
    };
    Ok(PrivacyDecisionReport {
        decision: PrivacyPolicy.decide(evidence),
        scan_status,
        scanned_bytes,
        total_bytes,
    })
}
impl SqliteClipboardRepository {
    pub(crate) fn policy_entry_authorized(
        &self,
        id: i64,
        permit: &crate::api::PrivacyReadPermit,
        now_ms: i64,
    ) -> Result<ClipboardEntry, String> {
        permit
            .require_interactive()
            .map_err(|error| error.to_string())?;
        let conn = self.conn.lock().map_err(unavailable)?;
        crate::api::privacy_session::check_read_with_conn(&conn, permit, now_ms)
            .map_err(|error| error.to_string())?;
        require_mutation_access(&conn, id, permit)?;
        strict_entry(self, &conn, id)
    }
    pub(crate) fn privacy_rule_summaries(&self) -> Result<Vec<PrivacyRuleSummary>, String> {
        let conn = self.conn.lock().map_err(unavailable)?;
        Ok(load_rules(&conn)?.iter().map(StoredRule::summary).collect())
    }
    pub(crate) fn assess_privacy_entry(
        &self,
        entry: &ClipboardEntry,
        include_exclusion: bool,
    ) -> Result<PrivacyDecisionReport, String> {
        let conn = self.conn.lock().map_err(unavailable)?;
        assess_entry(&conn, entry, include_exclusion)
    }
}

pub(super) fn strict_entry(
    repository: &SqliteClipboardRepository,
    conn: &Connection,
    id: i64,
) -> Result<ClipboardEntry, String> {
    let raw: (String, String, Option<String>, Option<String>, String) = conn
        .query_row(
            "SELECT content,preview,html_content,ocr_text,tags FROM clipboard_history WHERE id=?1",
            [id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()
        .map_err(unavailable)?
        .ok_or("ClipboardRecordUnavailable")?;
    for value in [
        Some(raw.0.as_str()),
        Some(raw.1.as_str()),
        raw.2.as_deref(),
        raw.3.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        encryption::decode_for_mutation(value).map_err(|error| error.to_string())?;
    }
    let tags =
        serde_json::from_str::<Vec<String>>(&raw.4).map_err(|_| "SensitivePersistenceRejected")?;
    let mut entry = repository
        .get_entry_by_id_with_conn(conn, id)?
        .ok_or("ClipboardRecordUnavailable")?;
    entry.tags = tags;
    Ok(entry)
}
fn sync_tags(conn: &Connection, id: i64, tags: &[String]) -> Result<(), String> {
    let json = serde_json::to_string(tags).map_err(unavailable)?;
    conn.execute(
        "UPDATE clipboard_history SET tags=?1 WHERE id=?2",
        rusqlite::params![json, id],
    )
    .map_err(unavailable)?;
    conn.execute("DELETE FROM entry_tags WHERE entry_id=?1", [id])
        .map_err(unavailable)?;
    for tag in tags {
        conn.execute(
            "INSERT INTO entry_tags(entry_id,tag) VALUES(?1,?2)",
            rusqlite::params![id, tag],
        )
        .map_err(unavailable)?;
    }
    Ok(())
}
fn protected(entry: &ClipboardEntry) -> bool {
    !super::migrations::search_visible(
        serde_json::to_string(&entry.tags).ok().as_deref(),
        [
            Some(&entry.content),
            Some(&entry.preview),
            entry.html_content.as_deref(),
            entry.ocr_text.as_deref(),
        ],
    )
}
pub(super) fn add_protection(entry: &mut ClipboardEntry) -> Result<(), String> {
    let normalized = crate::infrastructure::protected_resources::normalize(
        &entry.content_type,
        &entry.content,
        entry.html_content.as_deref(),
        entry.is_external,
    )?;
    entry.content = normalized.content;
    entry.html_content = normalized.html;
    entry.is_external = false;
    if !crate::database::has_sensitive_tag(&entry.tags) {
        entry.tags.push("sensitive".into());
    }
    Ok(())
}
fn directory(conn: &Connection) -> Option<std::path::PathBuf> {
    conn.path()
        .filter(|value| !value.is_empty())
        .and_then(|value| std::path::Path::new(value).parent())
        .map(std::path::Path::to_path_buf)
}
impl SqliteClipboardRepository {
    fn store_privacy_rule(&self, conn: &Connection, rule: &StoredRule) -> Result<(), String> {
        let plain = serde_json::to_string(rule).map_err(unavailable)?;
        let cipher = (self.encrypt_text)(&plain)
            .map_err(|error| error.to_string())?
            .into_string();
        if cipher.len() > MAX_RULE_BYTES {
            return Err("PrivacyRuleLimitExceeded".into());
        }
        conn.execute(
            "INSERT INTO privacy_rules(id,encrypted_rule,created_at_ms) VALUES(?1,?2,?3)",
            rusqlite::params![rule.id, cipher, rule.created_at_ms],
        )
        .map_err(unavailable)?;
        Ok(())
    }
    pub(crate) fn privacy_feedback(
        &self,
        id: i64,
        mut session: Option<&mut ClipboardEntry>,
        action: crate::api::PrivacyFeedbackAction,
        permit: &crate::api::PrivacyReadPermit,
        now_ms: i64,
    ) -> Result<crate::api::RuntimeSnapshot, String> {
        use crate::api::PrivacyFeedbackAction;
        permit
            .require_policy_change()
            .map_err(|error| error.to_string())?;
        let conn = self.conn.lock().map_err(unavailable)?;
        let tx =
            rusqlite::Transaction::new_unchecked(&conn, rusqlite::TransactionBehavior::Immediate)
                .map_err(unavailable)?;
        crate::api::privacy_session::check_read_with_conn(&tx, permit, now_ms)
            .map_err(|error| error.to_string())?;
        let mut entry = if let Some(entry) = session.as_deref() {
            if id >= 0 || entry.id != id {
                return Err("ClipboardRecordUnavailable".into());
            }
            entry.clone()
        } else {
            if id <= 0 {
                return Err("ClipboardRecordUnavailable".into());
            }
            strict_entry(self, &tx, id)?
        };
        let was_protected = if id > 0 {
            tx.query_row("SELECT NOT dc_search_visible(tags,content,preview,html_content,ocr_text) FROM clipboard_history WHERE id=?1",[id],|row|row.get(0)).map_err(unavailable)?
        } else {
            protected(&entry)
        };
        let mut rules = load_rules(&tx)?;
        let mut added = None;
        if action != PrivacyFeedbackAction::NotSensitive {
            let source = entry.source_app.trim();
            if source.is_empty() || source.len() > 1024 || source.chars().any(char::is_control) {
                return Err("PrivacyRuleSourceUnavailable".into());
            }
            let kind = match action {
                PrivacyFeedbackAction::ExactException => PrivacyRuleKind::ExactException,
                PrivacyFeedbackAction::IgnoreSource => PrivacyRuleKind::IgnoreSource,
                PrivacyFeedbackAction::ProtectSource => PrivacyRuleKind::ProtectSource,
                _ => unreachable!(),
            };
            if kind != PrivacyRuleKind::ExactException {
                for rule in rules.iter().filter(|rule| {
                    rule.kind != PrivacyRuleKind::ExactException && source_matches(rule, source)
                }) {
                    tx.execute("DELETE FROM privacy_rules WHERE id=?1", [&rule.id])
                        .map_err(unavailable)?;
                }
                rules.retain(|rule| {
                    rule.kind == PrivacyRuleKind::ExactException || !source_matches(rule, source)
                });
            }
            let mut matching = None;
            for rule in &rules {
                if rule.kind == kind
                    && source_matches(rule, source)
                    && (kind != PrivacyRuleKind::ExactException || rule.matches_entry(&entry)?)
                {
                    matching = Some(rule);
                    break;
                }
            }
            let rule = if let Some(rule) = matching {
                rule.clone()
            } else {
                if rules.len() >= MAX_RULES {
                    return Err("PrivacyRuleLimitExceeded".into());
                }
                let mut salt = [0; 16];
                rand::rngs::OsRng
                    .try_fill_bytes(&mut salt)
                    .map_err(|_| "EncryptionFailed")?;
                let rule = StoredRule {
                    id: uuid::Uuid::new_v4().to_string(),
                    version: 1,
                    kind,
                    source_app: Some(source.to_owned()),
                    salt,
                    digest: if kind == PrivacyRuleKind::ExactException {
                        Some(fingerprint(&entry, &salt)?)
                    } else {
                        None
                    },
                    origin_entry_id: (id > 0).then_some(id),
                    originally_protected: was_protected,
                    created_at_ms: now_ms,
                };
                self.store_privacy_rule(&tx, &rule)?;
                rule
            };
            added = Some(rule);
        }
        let protecting = action == PrivacyFeedbackAction::ProtectSource;
        let mut cleanup = if protecting && id > 0 {
            Some(super::sensitive_tags::AttachmentCleanup::for_entries(
                &tx,
                directory(&tx).as_deref(),
                &std::collections::HashSet::from([id]),
            )?)
        } else {
            None
        };
        if protecting {
            if id > 0 {
                // Encrypt before changing tags so the old visible FTS terms are removed atomically.
                self.encrypt_entry_with_conn(&tx, id)?;
            }
            add_protection(&mut entry)?;
            if id > 0 {
                sync_tags(&tx, id, &entry.tags)?;
            }
        } else if matches!(
            action,
            PrivacyFeedbackAction::NotSensitive | PrivacyFeedbackAction::ExactException
        ) {
            entry
                .tags
                .retain(|tag| !crate::database::has_sensitive_tag(std::slice::from_ref(tag)));
            if id > 0 {
                self.decrypt_entry_with_conn(&tx, id)?;
                sync_tags(&tx, id, &entry.tags)?;
                if let Some(rule) = added
                    .as_ref()
                    .filter(|rule| rule.kind == PrivacyRuleKind::ExactException)
                {
                    tx.execute("INSERT OR IGNORE INTO privacy_rule_entries(rule_id,entry_id) VALUES(?1,?2)",rusqlite::params![rule.id,id]).map_err(unavailable)?;
                }
            }
        }
        if let Some(cleanup) = &mut cleanup {
            cleanup.protect_surviving_references(&tx)?;
        }
        if session.is_some()
            && crate::api::privacy_session::snapshot_with_conn(&tx, now_ms)
                .map_err(|error| error.to_string())?
                .privacy_epoch
                == permit.epoch()
        {
            tx.execute(
                "UPDATE privacy_runtime_state SET privacy_epoch=privacy_epoch+1 WHERE singleton=1",
                [],
            )
            .map_err(unavailable)?;
        }
        let state = crate::api::privacy_session::snapshot_with_conn(&tx, now_ms)
            .map_err(|error| error.to_string())?;
        tx.commit().map_err(unavailable)?;
        if let Some(target) = session.as_mut() {
            **target = entry;
        }
        self.invalidate_caches();
        if cleanup.is_some_and(|cleanup| !cleanup.finish().is_empty()) {
            return Err("ProtectedAttachmentCleanupIncomplete".into());
        }
        Ok(state)
    }
    pub(crate) fn remove_privacy_rule(
        &self,
        id: &str,
        session: &mut [ClipboardEntry],
        permit: &crate::api::PrivacyReadPermit,
        now_ms: i64,
    ) -> Result<crate::api::RuntimeSnapshot, String> {
        permit
            .require_policy_change()
            .map_err(|error| error.to_string())?;
        let conn = self.conn.lock().map_err(unavailable)?;
        let tx =
            rusqlite::Transaction::new_unchecked(&conn, rusqlite::TransactionBehavior::Immediate)
                .map_err(unavailable)?;
        crate::api::privacy_session::check_read_with_conn(&tx, permit, now_ms)
            .map_err(|error| error.to_string())?;
        let rule = load_rules(&tx)?
            .into_iter()
            .find(|rule| rule.id == id)
            .ok_or("PrivacyRuleUnavailable")?;
        let ids: Vec<i64> = {
            let mut q = tx
                .prepare(
                    "SELECT entry_id FROM privacy_rule_entries WHERE rule_id=?1 ORDER BY entry_id",
                )
                .map_err(unavailable)?;
            let rows = q.query_map([id], |row| row.get(0)).map_err(unavailable)?;
            rows.collect::<rusqlite::Result<_>>().map_err(unavailable)?
        };
        let mut cleanup = if !ids.is_empty() {
            Some(super::sensitive_tags::AttachmentCleanup::for_entries(
                &tx,
                directory(&tx).as_deref(),
                &ids.iter().copied().collect(),
            )?)
        } else {
            None
        };
        let mut changed_session = Vec::new();
        if rule.kind == PrivacyRuleKind::ExactException {
            for entry_id in ids {
                let mut entry = strict_entry(self, &tx, entry_id)?;
                if !protected(&entry) {
                    self.encrypt_entry_with_conn(&tx, entry_id)?;
                    entry.tags.push("sensitive".into());
                    sync_tags(&tx, entry_id, &entry.tags)?;
                }
            }
            for (index, entry) in session.iter().enumerate() {
                if !protected(entry) && rule.matches_entry(entry)? {
                    let mut protected = entry.clone();
                    add_protection(&mut protected)?;
                    changed_session.push((index, protected));
                }
            }
        }
        tx.execute("DELETE FROM privacy_rules WHERE id=?1", [id])
            .map_err(unavailable)?;
        if let Some(cleanup) = &mut cleanup {
            cleanup.protect_surviving_references(&tx)?;
        }
        let state = crate::api::privacy_session::snapshot_with_conn(&tx, now_ms)
            .map_err(|error| error.to_string())?;
        tx.commit().map_err(unavailable)?;
        for (index, entry) in changed_session {
            session[index] = entry;
        }
        self.invalidate_caches();
        if cleanup.is_some_and(|cleanup| !cleanup.finish().is_empty()) {
            return Err("ProtectedAttachmentCleanupIncomplete".into());
        }
        Ok(state)
    }
}

pub(super) fn require_mutation_access(
    conn: &Connection,
    id: i64,
    permit: &crate::api::PrivacyReadPermit,
) -> Result<(), String> {
    let protected:bool=conn.query_row("SELECT NOT dc_search_visible(tags,content,preview,html_content,ocr_text) FROM clipboard_history WHERE id=?1",[id],|row|row.get(0)).optional().map_err(unavailable)?.ok_or("ClipboardRecordUnavailable")?;
    if protected && !permit.includes_sensitive() {
        return Err("SensitiveAccessDenied".into());
    }
    Ok(())
}

/// Re-evaluate at the repository boundary before any attachment or protected field is written.
pub(super) fn apply_to_entry(
    conn: &Connection,
    entry: &ClipboardEntry,
) -> Result<(ClipboardEntry, PrivacyDecision), String> {
    let report = assess_entry(conn, entry, true)?;
    if report.decision.action == crate::privacy::PrivacyAction::Skip {
        return Err("PrivacySourceExcluded".into());
    }
    let mut result = entry.clone();
    if report.decision.action == crate::privacy::PrivacyAction::Encrypt
        && !crate::database::has_sensitive_tag(&result.tags)
    {
        result.tags.push("sensitive".into());
    }
    Ok((result, report.decision))
}
pub(super) fn record_exception_use(
    conn: &Connection,
    id: i64,
    decision: &PrivacyDecision,
) -> Result<(), String> {
    if decision.action == crate::privacy::PrivacyAction::Allow
        && decision.reason == PrivacyReason::ExactExceptionMatched
    {
        if let Some(rule) = decision
            .evidence
            .iter()
            .find(|evidence| evidence.reason == PrivacyReason::ExactExceptionMatched)
            .and_then(|evidence| evidence.rule_id.as_deref())
        {
            conn.execute(
                "INSERT OR IGNORE INTO privacy_rule_entries(rule_id,entry_id) VALUES(?1,?2)",
                rusqlite::params![rule, id],
            )
            .map_err(unavailable)?;
        }
    }
    Ok(())
}
pub(super) fn transition_cleanup(
    conn: &Connection,
    entry: &ClipboardEntry,
) -> Result<Option<super::sensitive_tags::AttachmentCleanup>, String> {
    if entry.id <= 0 {
        return Ok(None);
    }
    if !crate::database::has_sensitive_tag(&entry.tags) {
        return Ok(None);
    }
    let visible:bool=conn.query_row("SELECT dc_search_visible(tags,content,preview,html_content,ocr_text) FROM clipboard_history WHERE id=?1",[entry.id],|row|row.get(0)).optional().map_err(unavailable)?.ok_or("ClipboardRecordUnavailable")?;
    if !visible {
        return Ok(None);
    }
    let cleanup = super::sensitive_tags::AttachmentCleanup::for_entries(
        conn,
        directory(conn).as_deref(),
        &std::collections::HashSet::from([entry.id]),
    )?;
    Ok((!cleanup.is_empty()).then_some(cleanup))
}

impl SqliteClipboardRepository {
    pub(crate) fn update_session_manual_authorized(
        &self,
        entry: &mut ClipboardEntry,
        content: &str,
        preview: &str,
        intent: crate::api::ManualPersistenceIntent,
        permit: &crate::api::PrivacyReadPermit,
        now_ms: i64,
    ) -> Result<(), String> {
        permit
            .require_interactive()
            .map_err(|error| error.to_string())?;
        if entry.id >= 0 {
            return Err("ClipboardRecordUnavailable".into());
        }
        let conn = self.conn.lock().map_err(unavailable)?;
        let tx =
            rusqlite::Transaction::new_unchecked(&conn, rusqlite::TransactionBehavior::Immediate)
                .map_err(unavailable)?;
        crate::api::privacy_session::check_read_with_conn(&tx, permit, now_ms)
            .map_err(|error| error.to_string())?;
        crate::api::privacy_session::check_manual_with_conn(&tx, intent, now_ms)
            .map_err(|error| error.to_string())?;
        if protected(entry) && !permit.includes_sensitive() {
            return Err("SensitiveAccessDenied".into());
        }
        let mut next = entry.clone();
        next.content = content.into();
        next.preview = preview.into();
        next.html_content = None;
        if next.content_type == "rich_text" {
            next.content_type = "text".into();
        }
        let (mut next, decision) = apply_to_entry(&tx, &next)?;
        if protected(entry) || decision.action == crate::privacy::PrivacyAction::Encrypt {
            add_protection(&mut next)?;
        }
        tx.execute(
            "UPDATE privacy_runtime_state SET privacy_epoch=privacy_epoch+1 WHERE singleton=1",
            [],
        )
        .map_err(unavailable)?;
        tx.commit().map_err(unavailable)?;
        *entry = next;
        self.invalidate_caches();
        Ok(())
    }
}

pub(super) fn prepare_entry_policy(
    conn: &Connection,
    entry: &ClipboardEntry,
    inherit_missing_ocr: bool,
) -> Result<(ClipboardEntry, PrivacyDecision), String> {
    if inherit_missing_ocr && entry.id > 0 && entry.ocr_text.is_none() {
        let old: Option<String> = conn
            .query_row(
                "SELECT ocr_text FROM clipboard_history WHERE id=?1",
                [entry.id],
                |row| row.get(0),
            )
            .optional()
            .map_err(unavailable)?
            .ok_or("ClipboardRecordUnavailable")?;
        let mut complete = entry.clone();
        complete.ocr_text = old
            .as_deref()
            .map(encryption::decode_for_mutation)
            .transpose()
            .map_err(|error| error.to_string())?;
        return apply_to_entry(conn, &complete);
    }
    apply_to_entry(conn, entry)
}
