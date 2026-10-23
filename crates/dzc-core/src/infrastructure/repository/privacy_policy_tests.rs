use super::clipboard_repo::{ClipboardRepository, SqliteClipboardRepository};
use super::privacy_policy::*;
use crate::api::PrivacyScanStatus;
use crate::api::{
    ManualPersistenceIntent, PrivacyAccessIntent, PrivacyFeedbackAction, PrivacyReadPermit,
    PrivacySessionCommand, SensitiveAccess,
};
use crate::domain::models::ClipboardEntry;
use crate::infrastructure::encryption;
use crate::infrastructure::encryption::EncryptionError;
use crate::privacy::{PrivacyAction, PrivacyReason};
use rusqlite::{types::Value, Connection};
use std::sync::{Arc, Mutex};

fn memory() -> SqliteClipboardRepository {
    let mut conn = Connection::open_in_memory().unwrap();
    super::migrations::run_migrations(&mut conn).unwrap();
    SqliteClipboardRepository::new(Arc::new(Mutex::new(conn)))
}

fn entry(text: &str) -> ClipboardEntry {
    serde_json::from_value(serde_json::json!({
        "id":0,"content_type":"text","content":text,"preview":"synthetic preview",
        "source_app":"Synthetic Editor","timestamp":1,"is_pinned":false,"tags":[]
    }))
    .unwrap()
}

fn permit(repo: &SqliteClipboardRepository) -> PrivacyReadPermit {
    repo.privacy_service()
        .authorize_read(
            PrivacyAccessIntent::InteractiveRead,
            SensitiveAccess::ExplicitUserAction,
            None,
            1,
        )
        .unwrap()
}

fn raw(repo: &SqliteClipboardRepository, id: i64) -> (String, String, Option<String>) {
    repo.conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT content,tags,ocr_text FROM clipboard_history WHERE id=?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap()
}

fn visible(repo: &SqliteClipboardRepository, id: i64) -> bool {
    repo.conn.lock().unwrap().query_row("SELECT dc_search_visible(tags,content,preview,html_content,ocr_text) FROM clipboard_history WHERE id=?1", [id], |row|row.get(0)).unwrap()
}

fn configure_custom(repo: &SqliteClipboardRepository) {
    repo.conn.lock().unwrap().execute("INSERT INTO settings(key,value) VALUES('app.privacy_protection_custom_rules','qvxcredential')", []).unwrap();
}

fn snapshot(repo: &SqliteClipboardRepository) -> Vec<Vec<Vec<Value>>> {
    let conn = repo.conn.lock().unwrap();
    [
        "clipboard_history",
        "entry_tags",
        "privacy_runtime_state",
        "privacy_rules",
        "privacy_rule_entries",
        "clipboard_fts_data",
        "clipboard_fts_idx",
        "clipboard_fts_docsize",
        "clipboard_fts_config",
    ]
    .into_iter()
    .map(|table| {
        let mut query = conn
            .prepare(&format!("SELECT * FROM {table} ORDER BY 1,2"))
            .unwrap();
        query
            .query_map([], |row| {
                (0..row.as_ref().column_count())
                    .map(|i| row.get(i))
                    .collect()
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<Vec<Value>>>>()
            .unwrap()
    })
    .collect()
}

#[test]
fn exact_exception_is_encrypted_scoped_to_complete_representation_and_reversible() {
    let repo = memory();
    configure_custom(&repo);
    let id = repo.save(&entry("qvxcredential complete"), None).unwrap();
    assert!(!visible(&repo, id));
    repo.privacy_feedback(
        id,
        None,
        PrivacyFeedbackAction::ExactException,
        &permit(&repo),
        1,
    )
    .unwrap();
    assert!(visible(&repo, id));
    let rules = repo.privacy_rule_summaries().unwrap();
    assert_eq!(rules.len(), 1);
    let conn = repo.conn.lock().unwrap();
    let cipher: String = conn
        .query_row("SELECT encrypted_rule FROM privacy_rules", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert!(encryption::has_ciphertext_envelope(&cipher));
    assert!(!cipher.contains("qvxcredential"));
    drop(conn);
    let allowed = entry("qvxcredential complete");
    let report = repo.assess_privacy_entry(&allowed, true).unwrap();
    assert_eq!(report.decision.reason, PrivacyReason::ExactExceptionMatched);
    let repeated = repo.save(&allowed, None).unwrap();
    assert!(visible(&repo, repeated));
    let changed = entry("qvxcredential complete!");
    assert_eq!(
        repo.assess_privacy_entry(&changed, true)
            .unwrap()
            .decision
            .action,
        PrivacyAction::Encrypt
    );
    let mut changed = allowed.clone();
    changed.source_app = "Different Editor".into();
    assert_eq!(
        repo.assess_privacy_entry(&changed, true)
            .unwrap()
            .decision
            .action,
        PrivacyAction::Encrypt
    );
    let mut changed = allowed.clone();
    changed.html_content = Some("different HTML".into());
    assert_eq!(
        repo.assess_privacy_entry(&changed, true)
            .unwrap()
            .decision
            .action,
        PrivacyAction::Encrypt
    );
    let mut changed = allowed;
    changed.ocr_text = Some("different OCR".into());
    assert_eq!(
        repo.assess_privacy_entry(&changed, true)
            .unwrap()
            .decision
            .action,
        PrivacyAction::Encrypt
    );
    repo.remove_privacy_rule(&rules[0].id, &mut [], &permit(&repo), 1)
        .unwrap();
    assert!(!visible(&repo, id));
    assert!(!visible(&repo, repeated));
    assert!(raw(&repo, repeated).1.contains("sensitive"));
    assert!(repo.privacy_rule_summaries().unwrap().is_empty());
}

#[test]
fn removing_an_ordinary_exact_exception_preserves_visible_history_and_session() {
    let repo = memory();
    let item = entry("ordinaryexactrevocationunique");
    let id = repo.save(&item, None).unwrap();
    repo.privacy_feedback(id, None, PrivacyFeedbackAction::ExactException, &permit(&repo), 1).unwrap();
    let rule = repo.privacy_rule_summaries().unwrap().remove(0);
    let repeated = repo.save(&item, None).unwrap();
    let mut session = item.clone();
    session.id = -41;
    let mut session = [session];
    let before_session = serde_json::to_value(&session).unwrap();
    repo.remove_privacy_rule(&rule.id, &mut session, &permit(&repo), 1).unwrap();
    for entry_id in [id, repeated] {
        assert!(visible(&repo, entry_id));
        assert_eq!(raw(&repo, entry_id).0, item.content);
    }
    assert_eq!(serde_json::to_value(&session).unwrap(), before_session);
    assert!(repo.privacy_rule_summaries().unwrap().is_empty());
}

#[test]
fn ordinary_exception_removal_applies_current_policy_after_removing_the_rule() {
    let repo = memory();
    let item = entry("qvxcredential newly protected after ordinary exception");
    let id = repo.save(&item, None).unwrap();
    assert!(visible(&repo, id));
    repo.privacy_feedback(id, None, PrivacyFeedbackAction::ExactException, &permit(&repo), 1).unwrap();
    let rule = repo.privacy_rule_summaries().unwrap().remove(0);
    configure_custom(&repo);
    let repeated = repo.save(&item, None).unwrap();
    assert!(visible(&repo, repeated));
    let mut session = item;
    session.id = -42;
    let mut session = [session];
    repo.remove_privacy_rule(&rule.id, &mut session, &permit(&repo), 1).unwrap();
    assert!(!visible(&repo, id));
    assert!(!visible(&repo, repeated));
    assert!(crate::database::has_sensitive_tag(&session[0].tags));
}

#[test]
fn removing_a_previously_protected_exact_exception_preserves_manual_protection_intent() {
    let repo = memory();
    let mut item = entry("manual protection without configured detector");
    item.tags.push("sensitive".into());
    let id = repo.save(&item, None).unwrap();
    repo.privacy_feedback(id, None, PrivacyFeedbackAction::ExactException, &permit(&repo), 1).unwrap();
    let rule = repo.privacy_rule_summaries().unwrap().remove(0);
    assert!(visible(&repo, id));
    repo.remove_privacy_rule(&rule.id, &mut [], &permit(&repo), 1).unwrap();
    assert!(!visible(&repo, id));
    assert!(encryption::has_ciphertext_envelope(&raw(&repo, id).0));
}

#[test]
fn a_reused_exact_exception_restores_each_records_own_protection() {
    for protected_first in [false, true] {
        let repo = memory();
        let ordinary = entry("per association manual protection without detector");
        let mut sensitive = ordinary.clone();
        sensitive.tags.push("sensitive".into());
        let first = repo.save(if protected_first { &sensitive } else { &ordinary }, None).unwrap();
        repo.privacy_feedback(first, None, PrivacyFeedbackAction::ExactException, &permit(&repo), 1).unwrap();
        let second = repo.save(if protected_first { &ordinary } else { &sensitive }, None).unwrap();
        assert_ne!(first, second);
        repo.privacy_feedback(second, None, PrivacyFeedbackAction::ExactException, &permit(&repo), 1).unwrap();
        repo.privacy_feedback(second, None, PrivacyFeedbackAction::ExactException, &permit(&repo), 1).unwrap();
        assert!(visible(&repo, first) && visible(&repo, second));
        let rule = repo.privacy_rule_summaries().unwrap().remove(0);
        repo.remove_privacy_rule(&rule.id, &mut [], &permit(&repo), 1).unwrap();
        assert_eq!(visible(&repo, first), !protected_first);
        assert_eq!(visible(&repo, second), protected_first);
    }
}

#[test]
fn reused_session_exceptions_restore_only_the_protected_association() {
    for protected_first in [false, true] {
        let repo = memory();
        let mut first = entry("session association manual protection without detector");
        first.id = -51;
        let mut second = first.clone();
        second.id = -52;
        if protected_first { first.tags.push("sensitive".into()); }
        else { second.tags.push("sensitive".into()); }
        for item in [&mut first, &mut second] {
            repo.privacy_feedback(item.id, Some(item), PrivacyFeedbackAction::ExactException, &permit(&repo), 1).unwrap();
        }
        let persistent = repo.save(&entry(&first.content), None).unwrap();
        let mut items = [first, second];
        let rule = repo.privacy_rule_summaries().unwrap().remove(0);
        repo.remove_privacy_rule(&rule.id, &mut items, &permit(&repo), 1).unwrap();
        assert_eq!(protected(&items[0]), protected_first);
        assert_eq!(protected(&items[1]), !protected_first);
        assert!(visible(&repo, persistent));
    }
}

fn replace_rule_json(repo: &SqliteClipboardRepository, change: impl FnOnce(&mut serde_json::Value)) {
    let conn = repo.conn.lock().unwrap();
    let cipher: String = conn.query_row("SELECT encrypted_rule FROM privacy_rules", [], |row| row.get(0)).unwrap();
    let mut value = serde_json::from_str(&encryption::decode_for_mutation(&cipher).unwrap()).unwrap();
    change(&mut value);
    let cipher = encryption::encrypt_sensitive(&serde_json::to_string(&value).unwrap()).unwrap().into_string();
    conn.execute("UPDATE privacy_rules SET encrypted_rule=?1", [cipher]).unwrap();
}

#[test]
fn session_protection_does_not_follow_an_id_from_another_host_instance() {
    let repo = memory();
    let mut item = entry("session instance association without detector");
    item.id = -61;
    item.tags.push("sensitive".into());
    repo.privacy_feedback(item.id, Some(&mut item), PrivacyFeedbackAction::ExactException, &permit(&repo), 1).unwrap();
    replace_rule_json(&repo, |rule| rule["association_protection"]["session_instance"] = uuid::Uuid::new_v4().to_string().into());
    let rule = repo.privacy_rule_summaries().unwrap().remove(0);
    let mut items = [item];
    repo.remove_privacy_rule(&rule.id, &mut items, &permit(&repo), 1).unwrap();
    assert!(!protected(&items[0]));
}

#[test]
fn deleted_history_ids_do_not_inherit_a_previous_associations_manual_protection() {
    let repo = memory();
    let mut item = entry("reused persistent id without detector");
    item.tags.push("sensitive".into());
    let id = repo.save(&item, None).unwrap();
    repo.privacy_feedback(id, None, PrivacyFeedbackAction::ExactException, &permit(&repo), 1).unwrap();
    repo.conn.lock().unwrap().execute("DELETE FROM clipboard_history WHERE id=?1", [id]).unwrap();
    item.tags.clear();
    let reused = repo.save(&item, None).unwrap();
    assert_eq!(id, reused);
    let rule = repo.privacy_rule_summaries().unwrap().remove(0);
    repo.remove_privacy_rule(&rule.id, &mut [], &permit(&repo), 1).unwrap();
    assert!(visible(&repo, reused));
}

#[test]
fn ambiguous_legacy_associations_refuse_feedback_and_revocation_atomically() {
    let repo = memory();
    let ordinary = entry("legacy association without recoverable provenance");
    let first = repo.save(&ordinary, None).unwrap();
    repo.privacy_feedback(first, None, PrivacyFeedbackAction::ExactException, &permit(&repo), 1).unwrap();
    let mut sensitive = ordinary.clone();
    sensitive.tags.push("sensitive".into());
    let second = repo.save(&sensitive, None).unwrap();
    repo.privacy_feedback(second, None, PrivacyFeedbackAction::ExactException, &permit(&repo), 1).unwrap();
    replace_rule_json(&repo, |rule| { rule.as_object_mut().unwrap().remove("association_protection"); });
    let rule = repo.privacy_rule_summaries().unwrap().remove(0);
    let mut session = [ordinary];
    session[0].id = -71;
    let before = snapshot(&repo);
    let before_session = serde_json::to_value(&session).unwrap();
    assert_eq!(repo.remove_privacy_rule(&rule.id, &mut session, &permit(&repo), 1).unwrap_err(), "PrivacyRuleProvenanceUnavailable");
    assert_eq!(snapshot(&repo), before);
    assert_eq!(serde_json::to_value(&session).unwrap(), before_session);
    assert_eq!(repo.privacy_feedback(first, None, PrivacyFeedbackAction::ExactException, &permit(&repo), 1).unwrap_err(), "PrivacyRuleProvenanceUnavailable");
    assert_eq!(snapshot(&repo), before);
}

#[test]
fn an_unambiguous_legacy_origin_retains_its_original_protection() {
    for was_protected in [false, true] {
        let repo = memory();
        let mut item = entry("legacy origin with recoverable provenance");
        if was_protected { item.tags.push("sensitive".into()); }
        let id = repo.save(&item, None).unwrap();
        repo.privacy_feedback(id, None, PrivacyFeedbackAction::ExactException, &permit(&repo), 1).unwrap();
        replace_rule_json(&repo, |rule| { rule.as_object_mut().unwrap().remove("association_protection"); });
        let rule = repo.privacy_rule_summaries().unwrap().remove(0);
        repo.remove_privacy_rule(&rule.id, &mut [], &permit(&repo), 1).unwrap();
        assert_eq!(visible(&repo, id), !was_protected);
    }
}

#[test]
fn association_payload_budget_refuses_declassification_atomically() {
    let repo = memory();
    let ordinary = entry("bounded association payload without detector");
    let id = repo.save(&ordinary, None).unwrap();
    repo.privacy_feedback(id, None, PrivacyFeedbackAction::ExactException, &permit(&repo), 1).unwrap();
    let mut sensitive = ordinary;
    sensitive.tags.push("sensitive".into());
    let id = repo.save(&sensitive, None).unwrap();
    let mut failing = repo.clone();
    failing.encrypt_text = |_| encryption::encrypt_sensitive(&"x".repeat(16 * 1024));
    let before = snapshot(&repo);
    assert_eq!(failing.privacy_feedback(id, None, PrivacyFeedbackAction::ExactException, &permit(&repo), 1).unwrap_err(), "PrivacyRuleLimitExceeded");
    assert_eq!(snapshot(&repo), before);
    assert!(!visible(&repo, id));
}

#[test]
fn a_real_near_limit_association_payload_rejects_another_protected_session() {
    let repo = memory();
    let ordinary = entry("actual bounded provenance without detector");
    let id = repo.save(&ordinary, None).unwrap();
    repo.privacy_feedback(id, None, PrivacyFeedbackAction::ExactException, &permit(&repo), 1).unwrap();
    let conn = repo.conn.lock().unwrap();
    let cipher: String = conn.query_row("SELECT encrypted_rule FROM privacy_rules", [], |row| row.get(0)).unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&encryption::decode_for_mutation(&cipher).unwrap()).unwrap();
    let mut low = 0;
    let mut high = 2048;
    let mut bounded = cipher;
    while low < high {
        let count = (low + high + 1) / 2;
        value["association_protection"]["session"] = serde_json::json!((0..count).map(|n| -1_000_000_000i64 - n as i64).collect::<Vec<_>>());
        let candidate = encryption::encrypt_sensitive(&serde_json::to_string(&value).unwrap()).unwrap().into_string();
        if candidate.len() <= 16 * 1024 { low = count; bounded = candidate; }
        else { high = count - 1; }
    }
    assert!(low > 0 && bounded.len() <= 16 * 1024);
    conn.execute("UPDATE privacy_rules SET encrypted_rule=?1", [bounded]).unwrap();
    drop(conn);
    assert_eq!(repo.privacy_rule_summaries().unwrap().len(), 1);
    let mut session = ordinary;
    session.id = i64::MIN;
    session.tags.push("sensitive".into());
    let before = snapshot(&repo);
    let before_session = serde_json::to_value(&session).unwrap();
    assert_eq!(repo.privacy_feedback(session.id, Some(&mut session), PrivacyFeedbackAction::ExactException, &permit(&repo), 1).unwrap_err(), "PrivacyRuleLimitExceeded");
    assert_eq!(snapshot(&repo), before);
    assert_eq!(serde_json::to_value(&session).unwrap(), before_session);
}

#[test]
fn exact_associations_keep_persistent_and_session_protection_independent() {
    for persistent_protected in [false, true] {
        let repo = memory();
        let mut persistent = entry("mixed persistent session exact association");
        let mut session = persistent.clone();
        session.id = -81;
        if persistent_protected { persistent.tags.push("sensitive".into()); }
        else { session.tags.push("sensitive".into()); }
        let id = repo.save(&persistent, None).unwrap();
        repo.privacy_feedback(id, None, PrivacyFeedbackAction::ExactException, &permit(&repo), 1).unwrap();
        repo.privacy_feedback(session.id, Some(&mut session), PrivacyFeedbackAction::ExactException, &permit(&repo), 1).unwrap();
        let rule = repo.privacy_rule_summaries().unwrap().remove(0);
        repo.remove_privacy_rule(&rule.id, std::slice::from_mut(&mut session), &permit(&repo), 1).unwrap();
        assert_eq!(visible(&repo, id), !persistent_protected);
        assert_eq!(protected(&session), !persistent_protected);
    }
}

#[test]
fn the_rule_count_budget_rejects_new_feedback_without_declassification() {
    let repo = memory();
    let mut item = entry("bounded rule count protected feedback");
    item.tags.push("sensitive".into());
    let id = repo.save(&item, None).unwrap();
    let conn = repo.conn.lock().unwrap();
    for n in 0..128 {
        let rule = StoredRule {
            id: uuid::Uuid::new_v4().to_string(), version: 1,
            kind: crate::api::PrivacyRuleKind::ProtectSource,
            source_app: Some(format!("Unrelated Synthetic {n}")), content_type: None,
            salt: [0; 16], digest: None, origin_entry_id: None, originally_protected: false,
            association_protection: None, created_at_ms: 1,
        };
        let cipher = encryption::encrypt_sensitive(&serde_json::to_string(&rule).unwrap()).unwrap().into_string();
        conn.execute("INSERT INTO privacy_rules VALUES(?1,?2,1)", rusqlite::params![rule.id, cipher]).unwrap();
    }
    drop(conn);
    let before = snapshot(&repo);
    assert_eq!(repo.privacy_feedback(id, None, PrivacyFeedbackAction::ExactException, &permit(&repo), 1).unwrap_err(), "PrivacyRuleLimitExceeded");
    assert_eq!(snapshot(&repo), before);
    assert!(!visible(&repo, id));
}

#[test]
fn one_record_feedback_does_not_create_a_future_exception_and_requires_explicit_permission() {
    let repo = memory();
    configure_custom(&repo);
    let id = repo.save(&entry("qvxcredential"), None).unwrap();
    let ordinary = repo
        .privacy_service()
        .authorize_read(
            PrivacyAccessIntent::InteractiveRead,
            SensitiveAccess::Denied,
            None,
            1,
        )
        .unwrap();
    let before = snapshot(&repo);
    assert_eq!(
        repo.privacy_feedback(id, None, PrivacyFeedbackAction::NotSensitive, &ordinary, 1)
            .unwrap_err(),
        "SensitiveAccessDenied"
    );
    assert_eq!(snapshot(&repo), before);
    repo.privacy_feedback(
        id,
        None,
        PrivacyFeedbackAction::NotSensitive,
        &permit(&repo),
        1,
    )
    .unwrap();
    assert!(visible(&repo, id));
    assert!(repo.privacy_rule_summaries().unwrap().is_empty());
    let new_id = repo.save(&entry("qvxcredential"), None).unwrap();
    assert!(!visible(&repo, new_id));
}

#[test]
fn source_ignore_is_pre_read_and_source_protect_does_not_declassify_existing_records() {
    let repo = memory();
    let id = repo.save(&entry("ordinary"), None).unwrap();
    repo.privacy_feedback(
        id,
        None,
        PrivacyFeedbackAction::ProtectSource,
        &permit(&repo),
        1,
    )
    .unwrap();
    assert!(!visible(&repo, id));
    assert_eq!(
        repo.assess_privacy_entry(&entry("another"), true)
            .unwrap()
            .decision
            .action,
        PrivacyAction::Encrypt
    );
    repo.privacy_feedback(
        id,
        None,
        PrivacyFeedbackAction::IgnoreSource,
        &permit(&repo),
        1,
    )
    .unwrap();
    assert!(!visible(&repo, id));
    let source = crate::capture::CaptureSource {
        app_name: " synthetic editor ".into(),
        process_path: None,
    };
    assert_eq!(
        assess_source(&repo.conn.lock().unwrap(), &source)
            .unwrap()
            .action,
        PrivacyAction::Skip
    );
    assert_eq!(
        repo.save(&entry("another"), None).unwrap_err(),
        "PrivacySourceExcluded"
    );
    let rule = repo.privacy_rule_summaries().unwrap().remove(0);
    repo.remove_privacy_rule(&rule.id, &mut [], &permit(&repo), 1)
        .unwrap();
    assert!(!visible(&repo, id));
    let mut old = {
        let conn = repo.conn.lock().unwrap();
        repo.get_entry_by_id_with_conn(&conn, id).unwrap().unwrap()
    };
    old.tags.clear();
    repo.save(&old, None).unwrap();
    assert!(!visible(&repo, id));
}

#[test]
fn removing_an_exception_does_not_reprotect_an_independently_edited_record() {
    let repo = memory();
    configure_custom(&repo);
    let id = repo.save(&entry("qvxcredential"), None).unwrap();
    repo.privacy_feedback(
        id,
        None,
        PrivacyFeedbackAction::ExactException,
        &permit(&repo),
        1,
    )
    .unwrap();
    let rule = repo.privacy_rule_summaries().unwrap().remove(0);
    repo.update_entry_content(id, "independently edited ordinary", "edited")
        .unwrap();
    repo.remove_privacy_rule(&rule.id, &mut [], &permit(&repo), 1)
        .unwrap();
    assert!(visible(&repo, id));
    assert_eq!(raw(&repo, id).0, "independently edited ordinary");
}

#[test]
fn removing_a_text_exception_does_not_read_unrelated_missing_session_files() {
    let repo = memory();
    configure_custom(&repo);
    let id = repo.save(&entry("qvxcredential"), None).unwrap();
    repo.privacy_feedback(
        id,
        None,
        PrivacyFeedbackAction::ExactException,
        &permit(&repo),
        1,
    )
    .unwrap();
    let rule = repo.privacy_rule_summaries().unwrap().remove(0);
    let mut unrelated = entry("synthetic-file-that-does-not-exist");
    unrelated.id = -1;
    unrelated.content_type = "file".into();
    repo.remove_privacy_rule(
        &rule.id,
        std::slice::from_mut(&mut unrelated),
        &permit(&repo),
        1,
    )
    .unwrap();
    assert!(!visible(&repo, id));
    assert!(unrelated.tags.is_empty());
}

#[test]
fn failed_rule_or_field_write_rolls_back_epoch_provenance_fts_and_cached_body() {
    let repo = memory();
    configure_custom(&repo);
    let id = repo.save(&entry("qvxcredential"), None).unwrap();
    let expected = repo.get_entry_content_with_html(id).unwrap();
    for trigger in [
        "CREATE TRIGGER synthetic_failure BEFORE INSERT ON privacy_rules BEGIN SELECT RAISE(ABORT,'synthetic rule failure'); END;",
        "CREATE TRIGGER synthetic_failure BEFORE UPDATE ON clipboard_history BEGIN SELECT RAISE(ABORT,'synthetic field failure'); END;",
        "CREATE TRIGGER synthetic_failure BEFORE INSERT ON privacy_rule_entries BEGIN SELECT RAISE(ABORT,'synthetic provenance failure'); END;"
    ] {
        repo.conn.lock().unwrap().execute_batch(trigger).unwrap();
        let before=snapshot(&repo);
        assert!(repo.privacy_feedback(id,None,PrivacyFeedbackAction::ExactException,&permit(&repo),1).is_err());
        assert_eq!(snapshot(&repo),before);
        assert_eq!(repo.get_entry_content_with_html(id).unwrap(),expected);
        repo.conn.lock().unwrap().execute_batch("DROP TRIGGER synthetic_failure").unwrap();
    }
    let mut failing = repo.clone();
    failing.encrypt_text = |_| Err(EncryptionError::KeyUnavailable);
    let before = snapshot(&repo);
    assert_eq!(
        failing
            .privacy_feedback(
                id,
                None,
                PrivacyFeedbackAction::ExactException,
                &permit(&repo),
                1
            )
            .unwrap_err(),
        "KeyUnavailable"
    );
    assert_eq!(snapshot(&repo), before);
}

#[test]
fn edited_content_and_late_ocr_are_protected_before_any_fts_or_resource_commit() {
    let repo = memory();
    configure_custom(&repo);
    let id = repo.save(&entry("ordinary uniqueqxv"), None).unwrap();
    repo.update_entry_content(id, "qvxcredential edited", "new preview")
        .unwrap();
    assert!(!visible(&repo, id));
    assert!(encryption::has_ciphertext_envelope(&raw(&repo, id).0));
    let id2 = repo.save(&entry("ordinary OCR source"), None).unwrap();
    let ticket = repo.privacy_service().capture_ticket(1).unwrap();
    repo.update_ocr_authorized(id2, "qvxcredential OCR", "done", &ticket, 1)
        .unwrap();
    let fields = raw(&repo, id2);
    assert!(!visible(&repo, id2));
    assert!(encryption::has_ciphertext_envelope(&fields.0));
    assert!(encryption::has_ciphertext_envelope(
        fields.2.as_deref().unwrap()
    ));
    let conn = repo.conn.lock().unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM clipboard_fts WHERE clipboard_fts MATCH 'qvxcredential'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    drop(conn);
    let stale = repo.privacy_service().capture_ticket(1).unwrap();
    repo.privacy_service()
        .set_session(PrivacySessionCommand::On { duration_ms: None }, 2)
        .unwrap();
    repo.privacy_service()
        .set_session(PrivacySessionCommand::Off, 3)
        .unwrap();
    let before = snapshot(&repo);
    assert_eq!(
        repo.update_ocr_authorized(id2, "late ordinary", "done", &stale, 4)
            .unwrap_err(),
        "PrivacyEpochChanged"
    );
    assert_eq!(snapshot(&repo), before);
}

#[test]
fn late_ocr_policy_failure_keeps_status_body_index_and_epoch_unchanged() {
    let repo = memory();
    configure_custom(&repo);
    let id = repo.save(&entry("ordinary late OCR"), None).unwrap();
    let ticket = repo.privacy_service().capture_ticket(1).unwrap();
    repo.conn.lock().unwrap().execute_batch("CREATE TRIGGER reject_ocr BEFORE UPDATE ON clipboard_history WHEN new.ocr_status='done' BEGIN SELECT RAISE(ABORT,'synthetic OCR SQL failure'); END;").unwrap();
    let before = snapshot(&repo);
    assert!(repo
        .update_ocr_authorized(id, "qvxcredential", "done", &ticket, 1)
        .is_err());
    assert_eq!(snapshot(&repo), before);
    repo.conn
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER reject_ocr")
        .unwrap();
    let mut no_key = repo.clone();
    no_key.encrypt_text = |_| Err(EncryptionError::KeyUnavailable);
    assert_eq!(
        no_key
            .update_ocr_authorized(id, "qvxcredential", "done", &ticket, 1)
            .unwrap_err(),
        "KeyUnavailable"
    );
    assert_eq!(snapshot(&repo), before);
    repo.update_ocr_authorized(id, "qvxcredential", "done", &ticket, 1)
        .unwrap();
    assert!(!visible(&repo, id));
}

#[test]
fn exact_file_exceptions_bind_complete_bytes_and_preserve_original_user_files() {
    let repo = memory();
    let directory = std::env::temp_dir().join(format!("dzc-rule-files-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("synthetic.txt");
    std::fs::write(&path, b"synthetic complete file").unwrap();
    let mut file = entry(&path.to_string_lossy());
    file.content_type = "file".into();
    file.tags.push("sensitive".into());
    let id = repo.save(&file, None).unwrap();
    repo.privacy_feedback(
        id,
        None,
        PrivacyFeedbackAction::ExactException,
        &permit(&repo),
        1,
    )
    .unwrap();
    file.id = 0;
    file.tags.clear();
    assert_eq!(
        repo.assess_privacy_entry(&file, true)
            .unwrap()
            .decision
            .reason,
        PrivacyReason::ExactExceptionMatched
    );
    std::fs::write(&path, b"different complete file").unwrap();
    assert_ne!(
        repo.assess_privacy_entry(&file, true)
            .unwrap()
            .decision
            .reason,
        PrivacyReason::ExactExceptionMatched
    );
    assert!(path.exists());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn feedback_protection_purges_physical_fts_terms_without_global_maintenance() {
    let repo = memory();
    let id = repo.save(&entry("qvx sharedpublictoken"), None).unwrap();
    repo.save(&entry("sharedpublictoken ordinary"), None)
        .unwrap();
    let shadow = |conn: &Connection| -> Vec<u8> {
        conn.prepare("SELECT block FROM clipboard_fts_data WHERE id>10 ORDER BY id")
            .unwrap()
            .query_map([], |row| row.get::<_, Vec<u8>>(0))
            .unwrap()
            .flat_map(Result::unwrap)
            .collect()
    };
    let conn = repo.conn.lock().unwrap();
    assert!(shadow(&conn).windows(b"qvx".len()).any(|w| w == b"qvx"));
    conn.execute_batch(
        "CREATE VIRTUAL TABLE fixture_vocabulary USING fts5vocab(clipboard_fts,'row');",
    )
    .unwrap();
    drop(conn);
    repo.privacy_feedback(
        id,
        None,
        PrivacyFeedbackAction::ProtectSource,
        &permit(&repo),
        1,
    )
    .unwrap();
    let conn = repo.conn.lock().unwrap();
    assert!(!shadow(&conn).windows(b"qvx".len()).any(|w| w == b"qvx"));
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM fixture_vocabulary WHERE term='qvx'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM clipboard_fts WHERE clipboard_fts MATCH 'sharedpublictoken'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    conn.execute(
        "INSERT INTO clipboard_fts(clipboard_fts,rank) VALUES('integrity-check',1)",
        [],
    )
    .unwrap();
}

#[test]
fn opaque_manual_permission_does_not_turn_confirmation_into_sensitive_access() {
    let repo = memory();
    let id = repo.save(&entry("ordinary"), None).unwrap();
    repo.privacy_service()
        .set_session(PrivacySessionCommand::On { duration_ms: None }, 1)
        .unwrap();
    let permit = permit(&repo);
    assert_eq!(
        repo.save_manual_authorized(
            &entry("manual"),
            None,
            ManualPersistenceIntent::Ordinary,
            &permit,
            1
        )
        .unwrap_err(),
        "ManualPersistenceConfirmationRequired"
    );
    let saved = repo
        .save_manual_authorized(
            &entry("manual"),
            None,
            ManualPersistenceIntent::ConfirmedDuringPrivacySession,
            &permit,
            1,
        )
        .unwrap();
    assert!(saved > id);
    repo.update_manual_authorized(
        id,
        "confirmed edit",
        "preview",
        ManualPersistenceIntent::ConfirmedDuringPrivacySession,
        &permit,
        1,
    )
    .unwrap();
    assert_eq!(raw(&repo, id).0, "confirmed edit");
    let mut item = entry("session");
    item.id = -1;
    repo.update_session_manual_authorized(
        &mut item,
        "confirmed session edit",
        "preview",
        ManualPersistenceIntent::ConfirmedDuringPrivacySession,
        &permit,
        1,
    )
    .unwrap();
    assert_eq!(item.content, "confirmed session edit");
    assert_eq!(
        repo.update_manual_authorized(
            id,
            "stale",
            "preview",
            ManualPersistenceIntent::ConfirmedDuringPrivacySession,
            &permit,
            1
        )
        .unwrap_err(),
        "PrivacyEpochChanged"
    );
    repo.privacy_service()
        .set_session(PrivacySessionCommand::Off, 2)
        .unwrap();
    let mut sensitive = entry("protected");
    sensitive.tags.push("sensitive".into());
    let protected_id = repo.save(&sensitive, None).unwrap();
    let ordinary = repo
        .privacy_service()
        .authorize_read(
            PrivacyAccessIntent::InteractiveRead,
            SensitiveAccess::Denied,
            None,
            2,
        )
        .unwrap();
    let before = snapshot(&repo);
    assert_eq!(
        repo.update_manual_authorized(
            protected_id,
            "denied",
            "preview",
            ManualPersistenceIntent::Ordinary,
            &ordinary,
            2
        )
        .unwrap_err(),
        "SensitiveAccessDenied"
    );
    assert_eq!(snapshot(&repo), before);
}

#[test]
fn invalid_rule_ciphertext_is_fail_closed_and_rule_metadata_never_contains_body() {
    let repo = memory();
    let id = repo.save(&entry("private qvxbody"), None).unwrap();
    repo.privacy_feedback(
        id,
        None,
        PrivacyFeedbackAction::ProtectSource,
        &permit(&repo),
        1,
    )
    .unwrap();
    let metadata = serde_json::to_string(&repo.privacy_rule_summaries().unwrap()).unwrap();
    assert!(!metadata.contains("qvxbody"));
    assert!(!metadata.contains("digest"));
    repo.conn
        .lock()
        .unwrap()
        .execute(
            "UPDATE privacy_rules SET encrypted_rule='ordinary invalid rule'",
            [],
        )
        .unwrap();
    let before = snapshot(&repo);
    assert_eq!(
        repo.save(&entry("unrelated"), None).unwrap_err(),
        "PrivacyRulesUnavailable"
    );
    assert_eq!(snapshot(&repo), before);
}

#[test]
fn bounded_scan_is_explicitly_incomplete_and_suspicions_are_not_a_blanket_encryption_rule() {
    let repo = memory();
    let report = repo
        .assess_privacy_entry(
            &entry("email@example.test +1 555 123 4567 c0mplex!Pass"),
            true,
        )
        .unwrap();
    assert_eq!(report.decision.action, PrivacyAction::Allow);
    let report = repo
        .assess_privacy_entry(&entry(&"x".repeat(128 * 1024)), true)
        .unwrap();
    assert_eq!(report.scan_status, PrivacyScanStatus::Incomplete);
    assert!(report.scanned_bytes <= 64 * 1024);
    let conn = repo.conn.lock().unwrap();
    for n in 0..129 {
        let id = uuid::Uuid::new_v4().to_string();
        let rule = StoredRule {
            id: id.clone(),
            version: 1,
            kind: crate::api::PrivacyRuleKind::ProtectSource,
            source_app: Some(format!("Synthetic {n}")),
            content_type: None,
            salt: [0; 16],
            digest: None,
            origin_entry_id: None,
            originally_protected: false,
            association_protection: None,
            created_at_ms: 1,
        };
        let cipher = encryption::encrypt_sensitive(&serde_json::to_string(&rule).unwrap())
            .unwrap()
            .into_string();
        conn.execute(
            "INSERT INTO privacy_rules VALUES(?1,?2,1)",
            rusqlite::params![id, cipher],
        )
        .unwrap();
    }
    drop(conn);
    assert_eq!(
        repo.privacy_rule_summaries().unwrap_err(),
        "PrivacyRuleLimitExceeded"
    );
}

#[test]
fn explanations_remain_flat_and_old_untagged_or_unreadable_cipher_is_never_reported_public() {
    let repo = memory();
    let id = repo
        .save(&entry("ordinary protected legacy"), None)
        .unwrap();
    let cipher = encryption::encrypt_sensitive("legacy protected body")
        .unwrap()
        .into_string();
    repo.conn
        .lock()
        .unwrap()
        .execute(
            "UPDATE clipboard_history SET content=?1,tags='[]' WHERE id=?2",
            rusqlite::params![cipher, id],
        )
        .unwrap();
    let permitted = permit(&repo);
    let candidate = repo.policy_entry_authorized(id, &permitted, 1).unwrap();
    let report = repo.assess_privacy_entry(&candidate, true).unwrap();
    assert_eq!(
        report.decision.reason,
        PrivacyReason::ExistingRecordProtected
    );
    let encoded = serde_json::to_value(&report).unwrap();
    assert_eq!(encoded["action"], "encrypt");
    assert!(encoded.get("decision").is_none());
    assert!(!encoded.to_string().contains("legacy protected body"));
    let damaged = format!(
        "linux:{}",
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, [255u8; 28])
    );
    repo.conn
        .lock()
        .unwrap()
        .execute(
            "UPDATE clipboard_history SET content=?1 WHERE id=?2",
            rusqlite::params![damaged, id],
        )
        .unwrap();
    assert_eq!(
        repo.policy_entry_authorized(id, &permit(&repo), 1)
            .unwrap_err(),
        "ProtectedRecordUnavailable"
    );
}

#[test]
fn session_feedback_and_rule_removal_publish_only_after_sql_commit() {
    let repo = memory();
    configure_custom(&repo);
    let mut session = entry("qvxcredential session");
    session.id = -1;
    session.tags.push("sensitive".into());
    repo.conn.lock().unwrap().execute_batch("CREATE TRIGGER reject_rule BEFORE INSERT ON privacy_rules BEGIN SELECT RAISE(ABORT,'synthetic rule rejection'); END;").unwrap();
    let original = serde_json::to_string(&session).unwrap();
    let before = snapshot(&repo);
    assert!(repo
        .privacy_feedback(
            -1,
            Some(&mut session),
            PrivacyFeedbackAction::ExactException,
            &permit(&repo),
            1
        )
        .is_err());
    assert_eq!(serde_json::to_string(&session).unwrap(), original);
    assert_eq!(snapshot(&repo), before);
    repo.conn
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER reject_rule;")
        .unwrap();
    repo.privacy_feedback(
        -1,
        Some(&mut session),
        PrivacyFeedbackAction::ExactException,
        &permit(&repo),
        1,
    )
    .unwrap();
    assert!(!crate::database::has_sensitive_tag(&session.tags));
    let rule = repo.privacy_rule_summaries().unwrap().remove(0);
    let mut items = [session];
    repo.remove_privacy_rule(&rule.id, &mut items, &permit(&repo), 1)
        .unwrap();
    assert!(crate::database::has_sensitive_tag(&items[0].tags));
}

#[test]
fn session_metadata_persistence_keeps_protection_without_granting_body_access() {
    let repo = memory();
    repo.privacy_service()
        .set_session(PrivacySessionCommand::On { duration_ms: None }, 1)
        .unwrap();
    let ordinary = repo
        .privacy_service()
        .authorize_read(
            PrivacyAccessIntent::InteractiveRead,
            SensitiveAccess::Denied,
            None,
            1,
        )
        .unwrap();
    let mut item = entry("metadatasecretunique");
    item.id = -7;
    item.tags = vec!["sensitive".into()];
    item.ocr_text = Some("metadataocrsecretunique".into());
    let original = serde_json::to_value(&item).unwrap();
    let before = snapshot(&repo);
    assert_eq!(
        repo.persist_session_metadata_authorized(
            &item,
            Some(true),
            None,
            ManualPersistenceIntent::Ordinary,
            &ordinary,
            1,
        )
        .unwrap_err(),
        "ManualPersistenceConfirmationRequired"
    );
    assert_eq!(snapshot(&repo), before);
    assert_eq!(
        repo.persist_session_metadata_authorized(
            &item,
            None,
            Some(vec!["work".into()]),
            ManualPersistenceIntent::ConfirmedDuringPrivacySession,
            &ordinary,
            1,
        )
        .unwrap_err(),
        "SensitiveAccessDenied"
    );
    assert_eq!(snapshot(&repo), before);
    let id = repo
        .persist_session_metadata_authorized(
            &item,
            Some(true),
            Some(vec!["sensitive".into(), "work".into()]),
            ManualPersistenceIntent::ConfirmedDuringPrivacySession,
            &ordinary,
            1,
        )
        .unwrap();
    let stored = raw(&repo, id);
    assert_ne!(stored.0, item.content);
    assert_ne!(stored.2, item.ocr_text);
    assert_eq!(
        encryption::decode_for_mutation(&stored.0).unwrap(),
        item.content
    );
    assert_eq!(
        stored
            .2
            .as_deref()
            .map(encryption::decode_for_mutation)
            .transpose()
            .unwrap(),
        item.ocr_text
    );
    assert!(!visible(&repo, id));
    assert!(repo
        .conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT is_pinned FROM clipboard_history WHERE id=?1",
            [id],
            |row| row.get::<_, bool>(0),
        )
        .unwrap());
    assert_eq!(
        repo.policy_entry_authorized(id, &ordinary, 1).unwrap_err(),
        "SensitiveAccessDenied"
    );
    assert!(!ordinary.includes_sensitive());
    assert_eq!(serde_json::to_value(&item).unwrap(), original);
    assert_eq!(repo.conn.lock().unwrap().query_row(
        "SELECT count(*) FROM clipboard_fts WHERE clipboard_fts MATCH 'metadatasecretunique OR metadataocrsecretunique'",
        [], |row| row.get::<_, i64>(0),
    ).unwrap(), 0);
}

#[test]
fn session_metadata_persistence_rejects_stale_permissions_and_rolls_back_sql_failure() {
    let repo = memory();
    let ordinary = repo
        .privacy_service()
        .authorize_read(
            PrivacyAccessIntent::InteractiveRead,
            SensitiveAccess::Denied,
            None,
            1,
        )
        .unwrap();
    let mut item = entry("original session text");
    item.id = -2;
    repo.privacy_service()
        .set_session(PrivacySessionCommand::On { duration_ms: None }, 1)
        .unwrap();
    repo.privacy_service()
        .set_session(PrivacySessionCommand::Off, 1)
        .unwrap();
    let before = snapshot(&repo);
    assert_eq!(
        repo.persist_session_metadata_authorized(
            &item,
            Some(true),
            None,
            ManualPersistenceIntent::Ordinary,
            &ordinary,
            1,
        )
        .unwrap_err(),
        "PrivacyEpochChanged"
    );
    assert_eq!(snapshot(&repo), before);
    let restricted = repo
        .privacy_service()
        .authorize_read(
            PrivacyAccessIntent::AutomationRead,
            SensitiveAccess::Denied,
            None,
            1,
        )
        .unwrap();
    assert_eq!(
        repo.persist_session_metadata_authorized(
            &item,
            Some(true),
            None,
            ManualPersistenceIntent::Ordinary,
            &restricted,
            1,
        )
        .unwrap_err(),
        "SensitiveAccessDenied"
    );
    let current = repo
        .privacy_service()
        .authorize_read(
            PrivacyAccessIntent::InteractiveRead,
            SensitiveAccess::Denied,
            None,
            1,
        )
        .unwrap();
    repo.conn.lock().unwrap().execute_batch(
        "CREATE TRIGGER fail_session_metadata BEFORE INSERT ON entry_tags BEGIN SELECT RAISE(ABORT,'synthetic metadata rollback'); END;",
    ).unwrap();
    assert!(repo
        .persist_session_metadata_authorized(
            &item,
            Some(true),
            Some(vec!["work".into()]),
            ManualPersistenceIntent::Ordinary,
            &current,
            1,
        )
        .is_err());
    assert_eq!(snapshot(&repo), before);
    assert_eq!(item.id, -2);
    repo.conn
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER fail_session_metadata")
        .unwrap();
    let id = repo
        .persist_session_metadata_authorized(
            &item,
            Some(true),
            Some(vec!["work".into()]),
            ManualPersistenceIntent::Ordinary,
            &current,
            1,
        )
        .unwrap();
    assert_eq!(raw(&repo, id).0, item.content);
    assert!(visible(&repo, id));
}

#[test]
fn session_metadata_persistence_preserves_untagged_encrypted_fields() {
    let repo = memory();
    let ordinary = repo
        .privacy_service()
        .authorize_read(
            PrivacyAccessIntent::InteractiveRead,
            SensitiveAccess::Denied,
            None,
            1,
        )
        .unwrap();
    let mut item = entry("unused");
    item.id = -9;
    item.content = encryption::encrypt_sensitive("legacyciphersecret")
        .unwrap()
        .into_string();
    assert!(item.tags.is_empty());
    let id = repo
        .persist_session_metadata_authorized(
            &item,
            Some(true),
            None,
            ManualPersistenceIntent::Ordinary,
            &ordinary,
            1,
        )
        .unwrap();
    let stored = raw(&repo, id);
    assert_eq!(
        encryption::decode_for_mutation(&stored.0).unwrap(),
        "legacyciphersecret"
    );
    assert!(crate::database::has_sensitive_tag(
        &serde_json::from_str::<Vec<String>>(&stored.1).unwrap()
    ));
    assert!(!visible(&repo, id));
}

#[test]
fn persisted_metadata_keeps_body_permission_denied_and_rejects_unprotection() {
    let repo = memory();
    let mut item = entry("persistedmetadatasecret");
    item.tags = vec!["sensitive".into()];
    let id = repo.save(&item, None).unwrap();
    let denied = repo
        .privacy_service()
        .authorize_read(
            PrivacyAccessIntent::InteractiveRead,
            SensitiveAccess::Denied,
            None,
            1,
        )
        .unwrap();
    let before_body = raw(&repo, id).0;
    repo.update_entry_metadata_authorized(id, Some(true), None, &denied, 1)
        .unwrap();
    assert_eq!(raw(&repo, id).0, before_body);
    repo.update_entry_metadata_authorized(
        id,
        None,
        Some(vec!["sensitive".into(), "work".into()]),
        &denied,
        1,
    )
    .unwrap();
    assert!(!visible(&repo, id));
    assert_eq!(
        repo.policy_entry_authorized(id, &denied, 1).unwrap_err(),
        "SensitiveAccessDenied"
    );
    let before = snapshot(&repo);
    assert_eq!(
        repo.update_entry_metadata_authorized(
            id,
            Some(false),
            Some(vec!["work".into()]),
            &denied,
            1,
        )
        .unwrap_err(),
        "SensitiveAccessDenied"
    );
    assert_eq!(snapshot(&repo), before);
    let plain = repo
        .save(&entry("newlyprotectedmetadatatoken"), None)
        .unwrap();
    repo.update_entry_metadata_authorized(
        plain,
        None,
        Some(vec![" sensitive ".into()]),
        &denied,
        1,
    )
    .unwrap();
    assert!(!visible(&repo, plain));
    assert_ne!(raw(&repo, plain).0, "newlyprotectedmetadatatoken");
}

#[test]
fn persisted_metadata_sql_failure_and_epoch_change_preserve_original_state() {
    let repo = memory();
    let id = repo.save(&entry("metadata original"), None).unwrap();
    let denied = repo
        .privacy_service()
        .authorize_read(
            PrivacyAccessIntent::InteractiveRead,
            SensitiveAccess::Denied,
            None,
            1,
        )
        .unwrap();
    repo.conn.lock().unwrap().execute_batch(
        "CREATE TRIGGER fail_metadata_tags BEFORE INSERT ON entry_tags BEGIN SELECT RAISE(ABORT,'synthetic metadata failure'); END;",
    ).unwrap();
    let before = snapshot(&repo);
    assert!(repo
        .update_entry_metadata_authorized(
            id,
            Some(true),
            Some(vec!["sensitive".into()]),
            &denied,
            1,
        )
        .is_err());
    assert_eq!(snapshot(&repo), before);
    assert!(visible(&repo, id));
    repo.conn
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER fail_metadata_tags")
        .unwrap();
    repo.privacy_service()
        .set_session(PrivacySessionCommand::On { duration_ms: None }, 1)
        .unwrap();
    let after = snapshot(&repo);
    assert_eq!(
        repo.update_entry_metadata_authorized(id, Some(true), None, &denied, 1,)
            .unwrap_err(),
        "PrivacyEpochChanged"
    );
    assert_eq!(snapshot(&repo), after);
}
