//! Content-policy controls expose rule metadata and explanations, never record bodies or keys.
use super::{ClipboardEntry, CoreError, CoreResult, HistoryService, PrivacyReadPermit};
use crate::privacy::PrivacyDecision;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyRuleKind {
    ExactException,
    IgnoreSource,
    ProtectSource,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyFeedbackAction {
    NotSensitive,
    ExactException,
    IgnoreSource,
    ProtectSource,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivacyRuleSummary {
    pub id: String,
    pub kind: PrivacyRuleKind,
    pub source_app: Option<String>,
    pub created_at_ms: i64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyScanStatus {
    Complete,
    Incomplete,
    Disabled,
    NotApplicable,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivacyDecisionReport {
    /// This is the current policy's assessment, not an assertion about a past scanner run.
    #[serde(flatten)]
    pub decision: PrivacyDecision,
    pub scan_status: PrivacyScanStatus,
    pub scanned_bytes: usize,
    pub total_bytes: usize,
}
#[derive(Clone)]
pub struct PrivacyPolicyService {
    pub(super) history: HistoryService,
}
impl HistoryService {
    pub fn privacy_policy(&self) -> PrivacyPolicyService {
        PrivacyPolicyService {
            history: self.clone(),
        }
    }
    pub fn save_manual_authorized(
        &self,
        entry: &ClipboardEntry,
        directory: Option<&std::path::Path>,
        intent: super::ManualPersistenceIntent,
        permit: &PrivacyReadPermit,
        now_ms: i64,
    ) -> CoreResult<i64> {
        permit
            .require_interactive()
            .map_err(|error| CoreError::from(error.to_string()))?;
        self.privacy()
            .check_read(permit, now_ms)
            .map_err(|error| CoreError::from(error.to_string()))?;
        self.repository
            .save_manual_authorized(entry, directory, intent, permit, now_ms)
            .map_err(CoreError::from)
    }
    /// Persists a Host-owned session entry with metadata changes only; grants no body read.
    pub fn persist_session_metadata_authorized(
        &self,
        entry: &ClipboardEntry,
        pinned: Option<bool>,
        tags: Option<Vec<String>>,
        intent: super::ManualPersistenceIntent,
        permit: &PrivacyReadPermit,
        now_ms: i64,
    ) -> CoreResult<i64> {
        self.repository
            .persist_session_metadata_authorized(entry, pinned, tags, intent, permit, now_ms)
            .map_err(CoreError::from)
    }

    /// The owning Host holds its session-entry lock throughout this operation.
    pub fn update_session_manual_authorized(
        &self,
        entry: &mut ClipboardEntry,
        content: &str,
        preview: &str,
        intent: super::ManualPersistenceIntent,
        permit: &PrivacyReadPermit,
        now_ms: i64,
    ) -> CoreResult<()> {
        self.repository
            .update_session_manual_authorized(entry, content, preview, intent, permit, now_ms)
            .map_err(CoreError::from)
    }
    pub fn update_manual_authorized(
        &self,
        id: i64,
        content: &str,
        preview: &str,
        intent: super::ManualPersistenceIntent,
        permit: &PrivacyReadPermit,
        now_ms: i64,
    ) -> CoreResult<()> {
        permit
            .require_interactive()
            .map_err(|error| CoreError::from(error.to_string()))?;
        self.privacy()
            .check_read(permit, now_ms)
            .map_err(|error| CoreError::from(error.to_string()))?;
        self.repository
            .update_manual_authorized(id, content, preview, intent, permit, now_ms)
            .map_err(CoreError::from)
    }
}
impl PrivacyPolicyService {
    pub fn feedback(
        &self,
        id: i64,
        action: PrivacyFeedbackAction,
        permit: &PrivacyReadPermit,
        now_ms: i64,
    ) -> CoreResult<super::RuntimeSnapshot> {
        self.history
            .repository
            .privacy_feedback(id, None, action, permit, now_ms)
            .map_err(CoreError::from)
    }
    /// The owning client holds its session-entry lock for this complete operation.
    pub fn feedback_for_session(
        &self,
        entry: &mut ClipboardEntry,
        action: PrivacyFeedbackAction,
        permit: &PrivacyReadPermit,
        now_ms: i64,
    ) -> CoreResult<super::RuntimeSnapshot> {
        let id = entry.id;
        self.history
            .repository
            .privacy_feedback(id, Some(entry), action, permit, now_ms)
            .map_err(CoreError::from)
    }
    pub fn remove_rule(
        &self,
        id: &str,
        permit: &PrivacyReadPermit,
        now_ms: i64,
    ) -> CoreResult<super::RuntimeSnapshot> {
        self.remove_rule_with_session(id, &mut [], permit, now_ms)
    }
    /// The owning client holds the session lock; changes publish only after the SQL commit.
    pub fn remove_rule_with_session(
        &self,
        id: &str,
        entries: &mut [ClipboardEntry],
        permit: &PrivacyReadPermit,
        now_ms: i64,
    ) -> CoreResult<super::RuntimeSnapshot> {
        self.history
            .repository
            .remove_privacy_rule(id, entries, permit, now_ms)
            .map_err(CoreError::from)
    }
    pub fn rules(&self) -> CoreResult<Vec<PrivacyRuleSummary>> {
        self.history
            .repository
            .privacy_rule_summaries()
            .map_err(CoreError::from)
    }
    pub fn decision(
        &self,
        id: i64,
        permit: &PrivacyReadPermit,
        now_ms: i64,
    ) -> CoreResult<PrivacyDecisionReport> {
        let entry = self
            .history
            .repository
            .policy_entry_authorized(id, permit, now_ms)
            .map_err(CoreError::from)?;
        self.decision_for_entry(&entry, permit, now_ms)
    }
    pub fn decision_for_session_entry(
        &self,
        entry: &ClipboardEntry,
        permit: &PrivacyReadPermit,
        now_ms: i64,
    ) -> CoreResult<PrivacyDecisionReport> {
        let entry = self
            .history
            .authorize_session_entry(entry, permit, now_ms)
            .map_err(|error| CoreError::from(error.to_string()))?;
        self.decision_for_entry(&entry, permit, now_ms)
    }
    fn decision_for_entry(
        &self,
        entry: &ClipboardEntry,
        permit: &PrivacyReadPermit,
        now_ms: i64,
    ) -> CoreResult<PrivacyDecisionReport> {
        let mut report = self
            .history
            .repository
            .assess_privacy_entry(entry, true)
            .map_err(CoreError::from)?;
        if self
            .history
            .runtime_snapshot(now_ms)
            .map_err(|error| CoreError::from(error.to_string()))?
            .session
            .active
        {
            report
                .decision
                .evidence
                .push(crate::privacy::PrivacyEvidence::new(
                    crate::privacy::EvidenceSource::UserControl,
                    crate::privacy::EvidenceLevel::Explicit,
                    crate::privacy::PrivacyReason::PrivacySessionActive,
                ));
            report.decision = crate::privacy::PrivacyPolicy.decide(report.decision.evidence);
        }
        self.history
            .privacy()
            .check_read(permit, now_ms)
            .map_err(|error| CoreError::from(error.to_string()))?;
        Ok(report)
    }
}
