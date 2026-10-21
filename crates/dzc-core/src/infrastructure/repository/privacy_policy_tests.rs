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
            salt: [0; 16],
            digest: None,
            origin_entry_id: None,
            originally_protected: false,
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
