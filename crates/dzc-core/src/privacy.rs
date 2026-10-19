//! Decisions consume explicit evidence; evidence levels are not statistical probabilities.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSource { UserControl, SourceFilter, ConfiguredRule, ExistingRecord, PlatformHint }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceLevel { Explicit, Matched, Unavailable, Incomplete }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyReason {
    CapturePaused,
    SourceExcluded,
    ConfiguredRuleMatched,
    ExistingRecordProtected,
    UserRequestedProtection,
    NoProtectionMatch,
    PrivacyHintsUnavailable,
    ScanIncomplete,
    SessionOnlyRequested,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivacyEvidence {
    pub source: EvidenceSource,
    pub level: EvidenceLevel,
    pub reason: PrivacyReason,
    /// A configured rule identifier or source identifier; clipboard payloads do not belong here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<String>,
}

impl PrivacyEvidence {
    pub fn new(source: EvidenceSource, level: EvidenceLevel, reason: PrivacyReason) -> Self {
        Self { source, level, reason, rule_id: None }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyAction { Allow, Encrypt, Skip, SessionOnly }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivacyDecision {
    pub action: PrivacyAction,
    pub reason: PrivacyReason,
    pub evidence: Vec<PrivacyEvidence>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PrivacyPolicy;

impl PrivacyPolicy {
    /// Preserve the current configured detector's meaning; this foundation adds no new detector.
    pub fn configured_rule(matched: bool) -> Option<PrivacyEvidence> {
        matched.then(|| PrivacyEvidence::new(EvidenceSource::ConfiguredRule, EvidenceLevel::Matched,
            PrivacyReason::ConfiguredRuleMatched))
    }

    pub fn decide(&self, evidence: Vec<PrivacyEvidence>) -> PrivacyDecision {
        use PrivacyReason::*;
        let applicable = |reason| evidence.iter().any(|item| item.reason == reason
            && matches!(item.level, EvidenceLevel::Explicit | EvidenceLevel::Matched));
        let (action, reason) = if applicable(CapturePaused) {
            (PrivacyAction::Skip, CapturePaused)
        } else if applicable(SourceExcluded) {
            (PrivacyAction::Skip, SourceExcluded)
        } else if applicable(SessionOnlyRequested) {
            (PrivacyAction::SessionOnly, SessionOnlyRequested)
        } else if applicable(ExistingRecordProtected) {
            (PrivacyAction::Encrypt, ExistingRecordProtected)
        } else if applicable(UserRequestedProtection) {
            (PrivacyAction::Encrypt, UserRequestedProtection)
        } else if applicable(ConfiguredRuleMatched) {
            (PrivacyAction::Encrypt, ConfiguredRuleMatched)
        } else {
            (PrivacyAction::Allow, NoProtectionMatch)
        };
        PrivacyDecision { action, reason, evidence }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_or_incomplete_evidence_does_not_claim_successful_scanning() {
        let evidence = vec![PrivacyEvidence::new(EvidenceSource::PlatformHint, EvidenceLevel::Unavailable,
            PrivacyReason::PrivacyHintsUnavailable), PrivacyEvidence::new(EvidenceSource::ConfiguredRule,
            EvidenceLevel::Incomplete, PrivacyReason::ScanIncomplete)];
        let decision = PrivacyPolicy.decide(evidence.clone());
        assert_eq!(decision.action, PrivacyAction::Allow);
        assert_eq!(decision.evidence, evidence);
        assert!(serde_json::to_string(&decision).unwrap().contains("scan_incomplete"));
    }
    #[test]
    fn skipped_sources_take_precedence_and_existing_protection_never_downgrades() {
        let encrypted = PrivacyEvidence::new(EvidenceSource::ExistingRecord, EvidenceLevel::Explicit,
            PrivacyReason::ExistingRecordProtected);
        assert_eq!(PrivacyPolicy.decide(vec![encrypted.clone()]).action, PrivacyAction::Encrypt);
        let excluded = PrivacyEvidence::new(EvidenceSource::SourceFilter, EvidenceLevel::Explicit,
            PrivacyReason::SourceExcluded);
        assert_eq!(PrivacyPolicy.decide(vec![encrypted, excluded]).action, PrivacyAction::Skip);
        let detected = PrivacyPolicy::configured_rule(true).unwrap();
        assert_eq!(PrivacyPolicy.decide(vec![detected]).action, PrivacyAction::Encrypt);
        assert!(PrivacyPolicy::configured_rule(false).is_none());
    }
}
