use crate::infrastructure::encryption::EncryptionError;
use rusqlite::Connection;

pub fn validate_sensitive_record(
    content_type: &str,
    external: bool,
    html: Option<&str>,
) -> Result<(), String> {
    if external
        || matches!(content_type, "image" | "file" | "video")
        || html.is_some_and(super::sensitive_tags::has_unsupported_sensitive_resources)
    {
        return Err(EncryptionError::SensitivePersistenceRejected.to_string());
    }
    Ok(())
}

pub(super) fn atomic<T>(
    conn: &Connection,
    operation: impl FnOnce(&Connection) -> Result<T, String>,
) -> Result<T, String> {
    if conn.is_autocommit() {
        let transaction =
            rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)
                .map_err(|error| error.to_string())?;
        let value = operation(&transaction)?;
        transaction.commit().map_err(|error| error.to_string())?;
        return Ok(value);
    }
    let name = format!("dzc_protection_{}", uuid::Uuid::new_v4().simple());
    conn.execute_batch(&format!("SAVEPOINT {name}"))
        .map_err(|error| error.to_string())?;
    match operation(conn) {
        Ok(value) => match conn.execute_batch(&format!("RELEASE {name}")) {
            Ok(()) => Ok(value),
            Err(error) => {
                let _ = conn.execute_batch(&format!("ROLLBACK TO {name}; RELEASE {name}"));
                Err(error.to_string())
            }
        },
        Err(error) => {
            let _ = conn.execute_batch(&format!("ROLLBACK TO {name}; RELEASE {name}"));
            Err(error)
        }
    }
}

/// Rebuild discards obsolete FTS segments without changing the FTS file format.
pub(super) fn rebuild_search(conn: &Connection) -> Result<(), String> {
    conn.execute_batch("PRAGMA secure_delete = ON;")
        .map_err(|error| error.to_string())?;
    conn.execute(
        "INSERT INTO clipboard_fts(clipboard_fts) VALUES('rebuild')",
        [],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v19_retries_legacy_alignment_once_and_keeps_future_success_marker() {
        let mut conn = Connection::open_in_memory().unwrap();
        super::super::migrations::run_migrations(&mut conn).unwrap();
        conn.execute("INSERT INTO settings(key,value) VALUES('db.sensitive_alignment_done','true') ON CONFLICT(key) DO UPDATE SET value='true'", []).unwrap();
        conn.execute("DELETE FROM schema_migrations WHERE version=19", [])
            .unwrap();
        super::super::migrations::run_migrations(&mut conn).unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT count(*) FROM settings WHERE key='db.sensitive_alignment_done'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        conn.execute(
            "INSERT INTO settings(key,value) VALUES('db.sensitive_alignment_done','true')",
            [],
        )
        .unwrap();
        super::super::migrations::run_migrations(&mut conn).unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT value FROM settings WHERE key='db.sensitive_alignment_done'",
                [],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
            "true"
        );
    }

    #[test]
    fn sensitive_resources_are_rejected_before_any_persistence() {
        for kind in ["image", "file", "video"] {
            assert_eq!(
                validate_sensitive_record(kind, false, None).unwrap_err(),
                "SensitivePersistenceRejected"
            );
        }
        for html in [
            "<img src='file:///private.png'>",
            "<p style='background:url(private.png)'>text</p>",
            "<!--TIEZ_RICH_IMAGE:data:image/png;base64,AAAA-->",
            "<img src='data:image/png;base64,AAAA'>",
            "<svg><image href='private.png'/></svg>",
            "<!--> <img src='private.png'> -->",
        ] {
            assert_eq!(
                validate_sensitive_record("rich_text", false, Some(html)).unwrap_err(),
                "SensitivePersistenceRejected"
            );
        }
        assert!(
            validate_sensitive_record("rich_text", false, Some("<p>private <b>text</b></p>"))
                .is_ok()
        );
        assert!(validate_sensitive_record("text", false, None).is_ok());
        assert!(validate_sensitive_record("text", true, None).is_err());
    }

    #[test]
    fn nested_atomic_failure_rolls_back_without_committing_the_outer_transaction() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE entries(value TEXT); INSERT INTO entries VALUES('original');",
        )
        .unwrap();
        let transaction = conn.transaction().unwrap();
        let result: Result<(), String> = atomic(&transaction, |connection| {
            connection
                .execute("UPDATE entries SET value='intermediate'", [])
                .map_err(|error| error.to_string())?;
            Err("EncryptionFailed".to_string())
        });
        assert_eq!(result.unwrap_err(), "EncryptionFailed");
        let value: String = transaction
            .query_row("SELECT value FROM entries", [], |row| row.get(0))
            .unwrap();
        assert_eq!(value, "original");
        transaction
            .execute("UPDATE entries SET value='outer change'", [])
            .unwrap();
        drop(transaction);
        let value: String = conn
            .query_row("SELECT value FROM entries", [], |row| row.get(0))
            .unwrap();
        assert_eq!(value, "original");
    }
    #[test]
    fn version_nineteen_rebuild_omits_legacy_sensitive_terms_and_preserves_plain_prefix_text() {
        let mut conn = Connection::open_in_memory().unwrap();
        super::super::migrations::run_migrations(&mut conn).unwrap();
        conn.execute_batch(r#"INSERT INTO clipboard_history(id,content_type,content,preview,source_app,timestamp,tags,ocr_text)
            VALUES(1,'text','privateprobe','summary','test',1,'["sensitive"]','privateocr'),
                  (2,'text','linux: ordinaryprefix','dpapi: plainpreview','test',2,'[]',NULL);
            INSERT INTO clipboard_fts(rowid,content,preview,source_app,content_kinds,ocr_text)
                VALUES(1,'privateprobe','summary','test','[]','privateocr');
            DELETE FROM schema_migrations WHERE version=19;"#).unwrap();
        let before: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM clipboard_fts WHERE clipboard_fts MATCH 'privateprobe'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(before, 1);
        super::super::migrations::run_migrations(&mut conn).unwrap();
        for term in ["privateprobe", "privateocr"] {
            let matches: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM clipboard_fts WHERE clipboard_fts MATCH ?1",
                    [term],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(matches, 0);
        }
        let prefix: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM clipboard_fts WHERE clipboard_fts MATCH 'ordinaryprefix'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(prefix, 1);
        let retained: String = conn
            .query_row(
                "SELECT content FROM clipboard_history WHERE id=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(retained, "privateprobe");
        conn.execute(
            "INSERT INTO clipboard_fts(clipboard_fts,rank) VALUES('integrity-check',1)",
            [],
        )
        .unwrap();
        let format: i64 = conn
            .query_row(
                "SELECT v FROM clipboard_fts_config WHERE k='version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(format, 4);
    }

    #[test]
    fn visibility_function_is_safe_in_untrusted_schema_and_handles_escaped_tags() {
        let mut conn = Connection::open_in_memory().unwrap();
        super::super::migrations::run_migrations(&mut conn).unwrap();
        conn.execute_batch("PRAGMA trusted_schema=OFF;").unwrap();
        for tags in [
            r#"["sensitive"]"#,
            r#"["SENSITIVE"]"#,
            r#"["\u0073ensitive"]"#,
            r#"["密码"]"#,
            "not JSON",
        ] {
            let allowed: bool = conn
                .query_row(
                    "SELECT dc_search_visible(?1,'body','preview',NULL,NULL)",
                    [tags],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(!allowed, "protected tags must be excluded");
        }
        let allowed: bool = conn
            .query_row(
                "SELECT dc_search_visible('[]','linux: ordinary','preview',NULL,NULL)",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(allowed);
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM clipboard_search_content", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
    }
}

#[cfg(test)]
mod batch_tests {
    use super::super::clipboard_repo::SqliteClipboardRepository;
    use super::*;
    use crate::infrastructure::encryption::{EncryptedPayload, EncryptionError};
    use std::cell::Cell;
    use std::sync::{Arc, Mutex};

    fn seal(value: &str) -> Result<EncryptedPayload, EncryptionError> {
        Ok(EncryptedPayload::for_test(format!("sealed:{value}")))
    }

    fn reject_second(value: &str) -> Result<EncryptedPayload, EncryptionError> {
        if value == "probe2" {
            return Err(EncryptionError::KeyUnavailable);
        }
        seal(value)
    }

    fn fixture() -> (SqliteClipboardRepository, Arc<Mutex<Connection>>) {
        let mut conn = Connection::open_in_memory().unwrap();
        super::super::migrations::run_migrations(&mut conn).unwrap();
        let transaction = conn.transaction().unwrap();
        for id in 1..=1020 {
            let tags = if id <= 20 { r#"["sensitive"]"# } else { "[]" };
            transaction.execute("INSERT INTO clipboard_history(id,content_type,content,preview,html_content,source_app,timestamp,tags,ocr_text) VALUES(?1,'rich_text',?2,'summary','<p>plain HTML</p>','fixture',?1,?3,'plain OCR')",
                rusqlite::params![id, format!("probe{id}"), tags]).unwrap();
        }
        transaction.commit().unwrap();
        let shared = Arc::new(Mutex::new(conn));
        let mut repo = SqliteClipboardRepository::new(shared.clone());
        repo.encrypt_text = seal;
        (repo, shared)
    }

    fn snapshot(conn: &Connection) -> Vec<(i64, String, String, String, String)> {
        conn.prepare("SELECT id,content,preview,html_content,ocr_text FROM clipboard_history WHERE id<=20 ORDER BY id").unwrap()
            .query_map([], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?))).unwrap()
            .collect::<rusqlite::Result<_>>().unwrap()
    }

    #[test]
    fn startup_batch_rebuilds_the_actual_index_once_and_single_record_still_rebuilds() {
        let (repo, shared) = fixture();
        let conn = shared.lock().unwrap();
        let ids: Vec<i64> = (1..=20).collect();
        let rebuilds = Cell::new(0usize);
        let start = std::time::Instant::now();
        repo.encrypt_entries_with_rebuild(&conn, &ids, |connection| {
            rebuild_search(connection)?;
            rebuilds.set(rebuilds.get() + 1);
            Ok(())
        })
        .unwrap();
        eprintln!(
            "startup batch: 1020 rows, 20 protected records, {} actual FTS rebuild, elapsed {:?}",
            rebuilds.get(),
            start.elapsed()
        );
        assert_eq!(rebuilds.get(), 1);
        assert!(snapshot(&conn)
            .iter()
            .all(
                |(_, content, preview, html, ocr)| [content, preview, html, ocr]
                    .iter()
                    .all(|value| value.starts_with("sealed:"))
            ));
        conn.execute(
            "INSERT INTO clipboard_fts(clipboard_fts,rank) VALUES('integrity-check',1)",
            [],
        )
        .unwrap();
        let (baseline, baseline_shared) = fixture();
        let baseline_conn = baseline_shared.lock().unwrap();
        let baseline_rebuilds = Cell::new(0usize);
        let baseline_start = std::time::Instant::now();
        for id in &ids {
            baseline
                .encrypt_entries_with_rebuild(&baseline_conn, &[*id], |connection| {
                    rebuild_search(connection)?;
                    baseline_rebuilds.set(baseline_rebuilds.get() + 1);
                    Ok(())
                })
                .unwrap();
        }
        eprintln!("per-record comparison: 1020 rows, 20 protected records, {} actual FTS rebuilds, elapsed {:?}", baseline_rebuilds.get(), baseline_start.elapsed());
        assert_eq!(baseline_rebuilds.get(), 20);
        // Disable update triggers so this failure can only come from the single-record cleanup call.
        conn.execute_batch("DROP TRIGGER clipboard_history_ai; DROP TRIGGER clipboard_history_ad; DROP TRIGGER clipboard_history_au;
            DROP TABLE clipboard_fts; CREATE TABLE clipboard_fts(clipboard_fts TEXT CHECK(clipboard_fts!='rebuild'));").unwrap();
        assert!(repo.encrypt_entry_with_conn(&conn, 21).is_err());
        let content: String = conn
            .query_row(
                "SELECT content FROM clipboard_history WHERE id=21",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(content, "probe21");
        repo.encrypt_entries_with_rebuild(&conn, &[], |_| {
            panic!("an empty batch must not rebuild")
        })
        .unwrap();
    }

    #[test]
    fn batch_sql_and_key_failures_roll_back_every_field_before_cleanup() {
        for key_failure in [false, true] {
            let (mut repo, shared) = fixture();
            if key_failure {
                repo.encrypt_text = reject_second;
            }
            let conn = shared.lock().unwrap();
            if !key_failure {
                conn.execute_batch("CREATE TRIGGER reject_second BEFORE UPDATE ON clipboard_history WHEN new.id=2 BEGIN SELECT RAISE(ABORT,'injected batch SQL failure'); END;").unwrap();
            }
            let before = snapshot(&conn);
            let rebuilt = Cell::new(0);
            assert!(repo
                .encrypt_entries_with_rebuild(&conn, &[1, 2], |connection| {
                    rebuilt.set(rebuilt.get() + 1);
                    rebuild_search(connection)
                })
                .is_err());
            assert_eq!(rebuilt.get(), 0);
            assert_eq!(snapshot(&conn), before);
            assert_eq!(
                conn.query_row(
                    "SELECT COUNT(*) FROM settings WHERE key='db.sensitive_alignment_done'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
                0
            );
            conn.execute(
                "INSERT INTO clipboard_fts(clipboard_fts,rank) VALUES('integrity-check',1)",
                [],
            )
            .unwrap();
        }
    }

    #[test]
    fn batch_cleanup_failure_rolls_back_all_protected_records_and_index_changes() {
        let (repo, shared) = fixture();
        let conn = shared.lock().unwrap();
        let before = snapshot(&conn);
        let result = repo.encrypt_entries_with_rebuild(&conn, &[1, 2], |connection| {
            rebuild_search(connection)?;
            Err("InjectedCleanupFailure".to_string())
        });
        assert_eq!(result.unwrap_err(), "InjectedCleanupFailure");
        assert_eq!(snapshot(&conn), before);
        conn.execute(
            "INSERT INTO clipboard_fts(clipboard_fts,rank) VALUES('integrity-check',1)",
            [],
        )
        .unwrap();
    }
}
