//! Bounded offline format validation. Matching structure never asserts credential validity online.
use super::{EvidenceLevel, EvidenceSource, PrivacyEvidence, PrivacyReason};
use base64::Engine;
use regex::{Regex, RegexBuilder};
use sha2::{Digest, Sha256};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub(crate) struct ScanTimer(Instant);
impl ScanTimer {
    pub(crate) fn start() -> Self { Self(Instant::now()) }
    pub(crate) fn elapsed(&self) -> Duration {
        #[cfg(test)]
        if let Some(clock) = SCAN_CLOCK.with(std::cell::Cell::get) { return clock(); }
        self.0.elapsed()
    }
}
#[cfg(test)]
thread_local! { static SCAN_CLOCK: std::cell::Cell<Option<fn() -> Duration>> = const { std::cell::Cell::new(None) }; }
#[cfg(test)]
pub(crate) fn with_scan_clock<T>(clock: fn() -> Duration, run: impl FnOnce() -> T) -> T {
    struct Reset(Option<fn() -> Duration>);
    impl Drop for Reset { fn drop(&mut self) { SCAN_CLOCK.with(|value| value.set(self.0)); } }
    let _reset = Reset(SCAN_CLOCK.with(|value| value.replace(Some(clock))));
    run()
}

#[derive(Debug, Clone, Copy)]
pub struct ScanBudget {
    pub max_bytes: usize,
    pub max_custom_rules: usize,
    pub max_duration: Duration,
}
impl Default for ScanBudget {
    fn default() -> Self {
        Self {
            max_bytes: 64 * 1024,
            max_custom_rules: 32,
            max_duration: Duration::from_millis(5),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanReport {
    pub evidence: Vec<PrivacyEvidence>,
    pub scanned_bytes: usize,
    pub total_bytes: usize,
    pub completed: bool,
}

fn evidence(
    source: EvidenceSource,
    level: EvidenceLevel,
    reason: PrivacyReason,
    id: &str,
) -> PrivacyEvidence {
    PrivacyEvidence {
        source,
        level,
        reason,
        rule_id: Some(id.to_owned()),
    }
}

fn builtin(pattern: &'static str, cell: &'static OnceLock<Regex>) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("fixed privacy pattern is valid"))
}

/// Exact bytes, not a category-wide exception identifier. No content is returned or logged.
pub(crate) fn content_fingerprint(content: &[u8]) -> String {
    Sha256::digest(content)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn der_item(bytes: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, tail) = bytes.split_first()?;
    let (&first, mut data) = tail.split_first()?;
    let length = if first & 0x80 == 0 {
        first as usize
    } else {
        let count = (first & 0x7f) as usize;
        if count == 0 || count > 4 || data.len() < count || data[0] == 0 {
            return None;
        }
        let mut length = 0usize;
        for byte in &data[..count] {
            length = length.checked_mul(256)?.checked_add(*byte as usize)?;
        }
        if length < 128 {
            return None;
        }
        data = &data[count..];
        length
    };
    if data.len() < length {
        return None;
    }
    Some((tag, &data[..length], &data[length..]))
}

fn der_algorithm(bytes: &[u8]) -> bool {
    let Some((6, oid, rest)) = der_item(bytes) else { return false; };
    !oid.is_empty() && (rest.is_empty() || der_item(rest).is_some_and(|(_, _, tail)| tail.is_empty()))
}

fn der_attributes(mut bytes: &[u8]) -> bool {
    while !bytes.is_empty() {
        let Some((0x30, attribute, rest)) = der_item(bytes) else { return false; };
        let Some((6, oid, values)) = der_item(attribute) else { return false; };
        let Some((0x31, mut values, [])) = der_item(values) else { return false; };
        if oid.is_empty() || values.is_empty() { return false; }
        while !values.is_empty() {
            let Some((_, _, rest)) = der_item(values) else { return false; };
            values = rest;
        }
        bytes = rest;
    }
    true
}

fn private_key_info(fields: &[(u8, &[u8])]) -> bool {
    if !(3..=5).contains(&fields.len()) || fields[0].0 != 2
        || !matches!(fields[0].1, [0] | [1]) || fields[1].0 != 0x30
        || !der_algorithm(fields[1].1) || fields[2].0 != 4 || fields[2].1.len() < 16 {
        return false;
    }
    let mut next = 3;
    if fields.get(next).is_some_and(|(tag, _)| *tag == 0xa0) {
        if !der_attributes(fields[next].1) { return false; }
        next += 1;
    }
    // RFC 5958 uses IMPLICIT [1] BIT STRING and version INTEGER 1 when present.
    let public = fields.get(next).is_some_and(|(tag, _)| *tag == 0x81);
    if public {
        let bits = fields[next].1;
        if bits.len() < 2 || bits[0] > 7 || bits.last().is_none_or(|last| last & ((1u8 << bits[0]) - 1) != 0) {
            return false;
        }
        next += 1;
    }
    next == fields.len() && public == (fields[0].1 == [1])
}

fn der_private_key(bytes: &[u8], label: &str) -> bool {
    let Some((0x30, mut sequence, [])) = der_item(bytes) else {
        return false;
    };
    let mut fields = Vec::new();
    while !sequence.is_empty() && fields.len() < 12 {
        let Some((tag, body, rest)) = der_item(sequence) else {
            return false;
        };
        fields.push((tag, body));
        sequence = rest;
    }
    if !sequence.is_empty() {
        return false;
    }
    match label {
        "PRIVATE KEY" => private_key_info(&fields),
        "RSA PRIVATE KEY" => {
            fields.len() == 9
                && fields[0] == (2, &[0][..])
                && fields
                    .iter()
                    .all(|(tag, body)| *tag == 2 && !body.is_empty())
                && fields[1].1.len() >= 128
                && fields[2].1.len() <= 4
                && fields[2].1.last().is_some_and(|byte| byte & 1 == 1)
                && fields[3..].iter().all(|(_, body)| body.len() >= 16)
        }
        "EC PRIVATE KEY" => {
            fields.len() >= 3
                && fields[0] == (2, &[1][..])
                && fields[1].0 == 4
                && matches!(fields[1].1.len(), 32 | 48 | 66)
                && fields[2].0 == 0xa0
                && der_item(fields[2].1)
                    .is_some_and(|(tag, oid, rest)| tag == 6 && !oid.is_empty() && rest.is_empty())
        }
        "ENCRYPTED PRIVATE KEY" => {
            fields.len() == 2
                && fields[0].0 == 0x30
                && der_item(fields[0].1).is_some_and(|(tag, oid, _)| tag == 6 && !oid.is_empty())
                && fields[1].0 == 4
                && fields[1].1.len() >= 32
        }
        _ => false,
    }
}

fn validated_private_key(content: &str, expired: impl Fn() -> bool) -> (bool, bool) {
    for label in [
        "PRIVATE KEY",
        "RSA PRIVATE KEY",
        "EC PRIVATE KEY",
        "ENCRYPTED PRIVATE KEY",
    ] {
        let begin = format!("-----BEGIN {label}-----");
        let end = format!("-----END {label}-----");
        for (start, _) in content.match_indices(&begin) {
            if expired() {
                return (false, false);
            }
            let body = &content[start + begin.len()..];
            let Some(stop) = body.find(&end) else {
                continue;
            };
            // A malformed outer block cannot consume a later complete block of the same label.
            if body[..stop].contains(&begin) {
                continue;
            }
            let encoded: String = body[..stop]
                .chars()
                .filter(|c| !c.is_ascii_whitespace())
                .collect();
            if encoded.len() > 48 * 1024 {
                continue;
            }
            if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(encoded) {
                if der_private_key(&decoded, label) {
                    return (true, true);
                }
            }
        }
    }
    (false, true)
}

pub fn scan_content(
    content: &str,
    kinds: &[String],
    custom: &[String],
    budget: ScanBudget,
) -> ScanReport {
    let started = ScanTimer::start();
    let mut end = content.len().min(budget.max_bytes);
    while !content.is_char_boundary(end) {
        end -= 1;
    }
    let scanned = &content[..end];
    let mut result = ScanReport {
        evidence: Vec::new(),
        scanned_bytes: end,
        total_bytes: content.len(),
        completed: end == content.len(),
    };
    let expired = || started.elapsed() >= budget.max_duration;
    if expired() {
        result.completed = false;
    } else {
        for (index, rule) in custom
            .iter()
            .filter(|rule| !rule.trim().is_empty())
            .enumerate()
        {
            if index >= budget.max_custom_rules || expired() {
                result.completed = false;
                break;
            }
            if rule.len() > 8 * 1024 {
                result.completed = false;
                continue;
            }
            match RegexBuilder::new(rule)
                .size_limit(64 * 1024)
                .dfa_size_limit(512 * 1024)
                .build()
            {
                Ok(regex) if regex.is_match(scanned) => result.evidence.push(evidence(
                    EvidenceSource::ConfiguredRule,
                    EvidenceLevel::Matched,
                    PrivacyReason::ConfiguredRuleMatched,
                    &format!("custom_sha256:{}", content_fingerprint(rule.as_bytes())),
                )),
                Ok(_) => {}
                Err(_) => result.completed = false,
            }
        }
        let (private_key, pem_complete) = validated_private_key(scanned, expired);
        result.completed &= pem_complete;
        if private_key {
            result.evidence.push(evidence(
                EvidenceSource::CredentialStructure,
                EvidenceLevel::Verified,
                PrivacyReason::CredentialStructureVerified,
                "pem_private_key_der_v1",
            ));
        }
        static AWS_ID: OnceLock<Regex> = OnceLock::new();
        static AWS_SECRET: OnceLock<Regex> = OnceLock::new();
        let id = builtin(
            r#"(?m)(?:AWS_ACCESS_KEY_ID|aws_access_key_id)\s*[=:]\s*["']?A(?:KIA|SIA)[A-Z0-9]{16}(?:["'\s]|$)"#,
            &AWS_ID,
        );
        let secret = builtin(
            r#"(?m)(?:AWS_SECRET_ACCESS_KEY|aws_secret_access_key)\s*[=:]\s*["']?[A-Za-z0-9/+]{40}(?:["'\s]|$)"#,
            &AWS_SECRET,
        );
        if !expired() && id.is_match(scanned) && secret.is_match(scanned) {
            result.evidence.push(evidence(
                EvidenceSource::CredentialStructure,
                EvidenceLevel::Verified,
                PrivacyReason::CredentialStructureVerified,
                "aws_paired_credentials_structure_v1",
            ));
        }
        static SUSPECT: OnceLock<Regex> = OnceLock::new();
        let suspect = builtin(
            r"(?i)(?:password|passwd|api[_-]?key|secret|bearer)\s*[=: ]\s*\S{6,}|gh[pousr]_[A-Za-z0-9]{20,255}|-----BEGIN .*PRIVATE KEY-----|\beyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\b",
            &SUSPECT,
        );
        if !expired() && suspect.is_match(scanned) {
            result.evidence.push(evidence(
                EvidenceSource::ContextHeuristic,
                EvidenceLevel::Suspected,
                PrivacyReason::SuspectedSensitive,
                "credential_context_hint_v1",
            ));
        }
        if !expired()
            && !kinds.is_empty()
            && crate::services::capture::content::contains_sensitive_info(scanned, kinds, &[])
        {
            result.evidence.push(evidence(
                EvidenceSource::ContextHeuristic,
                EvidenceLevel::Suspected,
                PrivacyReason::SuspectedSensitive,
                "configured_classification_hint_v1",
            ));
        }
        if expired() {
            result.completed = false;
        }
    }
    if !result.completed {
        result.evidence.push(evidence(
            EvidenceSource::ConfiguredRule,
            EvidenceLevel::Incomplete,
            PrivacyReason::ScanIncomplete,
            "bounded_privacy_scan_v1",
        ));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::privacy::{PrivacyAction, PrivacyPolicy};
    fn budget() -> ScanBudget {
        ScanBudget {
            max_duration: Duration::from_secs(1),
            ..Default::default()
        }
    }
    #[test]
    fn ordinary_classification_and_unverified_tokens_do_not_force_encryption() {
        let kinds = ["email", "phone", "idcard", "password", "secret"].map(str::to_owned);
        for text in [
            "person@example.test",
            "13800138000",
            "Abcdef12345!",
            "password=synthetic-example",
            "ghp_abcdefghijklmnopqrstuvwxyz1234567890",
        ] {
            let report = scan_content(text, &kinds, &[], budget());
            assert!(report.completed);
            assert_eq!(
                PrivacyPolicy.decide(report.evidence).action,
                PrivacyAction::Allow
            );
        }
    }
    #[test]
    fn credentials_need_structure_and_custom_rule_identifiers_never_contain_the_pattern() {
        let mut der = vec![
            0x30, 46, 2, 1, 0, 0x30, 5, 6, 3, 0x2b, 0x65, 0x70, 4, 34, 4, 32,
        ];
        der.extend([7; 32]);
        let pem = format!(
            "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----",
            base64::engine::general_purpose::STANDARD.encode(der)
        );
        assert_eq!(
            PrivacyPolicy
                .decide(scan_content(&pem, &[], &[], budget()).evidence)
                .action,
            PrivacyAction::Encrypt
        );
        let fake = "-----BEGIN PRIVATE KEY-----\nY29udGVudA==\n-----END PRIVATE KEY-----";
        assert_eq!(
            PrivacyPolicy
                .decide(scan_content(fake, &[], &[], budget()).evidence)
                .action,
            PrivacyAction::Allow
        );
        let rule = "synthetic-private-pattern".to_owned();
        let report = scan_content(&rule, &[], &[rule.clone()], budget());
        assert_eq!(
            PrivacyPolicy.decide(report.evidence.clone()).action,
            PrivacyAction::Encrypt
        );
        assert!(!serde_json::to_string(&report.evidence)
            .unwrap()
            .contains(&rule));
    }
    #[test]
    fn pkcs8_public_key_variant_validates_version_order_and_bit_string() {
        fn key(version: u8, extra: &[u8]) -> Vec<u8> {
            let mut der = vec![0x30, (46 + extra.len()) as u8, 2, 1, version, 0x30, 5, 6, 3, 0x2b, 0x65, 0x70, 4, 34, 4, 32];
            der.extend([7; 32]);der.extend(extra);der
        }
        let mut public = vec![0x81, 33, 0];public.extend([9; 32]);
        let mut attributes_then_public = vec![0xa0, 0];attributes_then_public.extend(&public);
        for der in [key(0,&[]),key(1,&public),key(1,&attributes_then_public)] {
            assert!(der_private_key(&der,"PRIVATE KEY"));
            let pem=format!("-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----",base64::engine::general_purpose::STANDARD.encode(der));
            assert_eq!(PrivacyPolicy.decide(scan_content(&pem,&[],&[],budget()).evidence).action,PrivacyAction::Encrypt);
        }
        let mut bad_padding=public.clone();bad_padding[2]=1;
        let mut duplicate=public.clone();duplicate.extend(&public);
        let mut wrong_order=public.clone();wrong_order.extend([0xa0,0]);
        let mut explicit=public.clone();explicit[0]=0xa1;
        for der in [key(0,&public),key(1,&[]),key(2,&public),key(1,&[0x81,1,0]),key(1,&[0x81,2,8,0]),key(1,&bad_padding),key(1,&duplicate),key(1,&wrong_order),key(1,&explicit),key(1,&[0xa0,1,0])] {
            assert!(!der_private_key(&der,"PRIVATE KEY"));
        }
    }
    #[test]
    fn budgets_and_invalid_rules_report_incomplete_instead_of_safe() {
        for (text, custom, limit) in [
            ("界".repeat(100), vec![], 7),
            ("ordinary".into(), vec!["[".into()], 100),
        ] {
            let report = scan_content(
                &text,
                &[],
                &custom,
                ScanBudget {
                    max_bytes: limit,
                    ..budget()
                },
            );
            assert!(!report.completed);
            assert_eq!(
                PrivacyPolicy.decide(report.evidence).reason,
                PrivacyReason::ScanIncomplete
            );
        }
        let report = scan_content(
            "ordinary",
            &[],
            &[],
            ScanBudget {
                max_duration: Duration::ZERO,
                ..budget()
            },
        );
        assert!(!report.completed);
    }
}
