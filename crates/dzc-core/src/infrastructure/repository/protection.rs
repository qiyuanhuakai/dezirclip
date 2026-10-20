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

/// One-time alignment discards obsolete FTS segments from legacy history.
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

/// Check before a visible record is changed. FTS5 removes postings incrementally;
/// SQLite core secure_delete also erases bytes freed by those index page writes.
pub(super) fn require_secure_search(conn: &Connection) -> Result<(), String> {
    conn.execute_batch("PRAGMA secure_delete = ON;")
        .map_err(|error| error.to_string())?;
    let enabled: bool = conn.query_row(
        "SELECT COALESCE((SELECT v=1 FROM clipboard_fts_config WHERE k='secure-delete'),0)",
        [], |row| row.get(0),
    ).map_err(|error| error.to_string())?;
    if !enabled {
        return Err("SensitiveIndexProtectionUnavailable".into());
    }
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
        conn.execute("DELETE FROM schema_migrations WHERE version>=19", [])
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
            DELETE FROM schema_migrations WHERE version>=19;"#).unwrap();
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
    fn startup_batch_rebuilds_once_and_single_record_requires_protected_index() {
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
        // Disable update triggers so failure comes from the pre-write index check.
        conn.execute_batch("DROP TRIGGER clipboard_history_ai; DROP TRIGGER clipboard_history_ad; DROP TRIGGER clipboard_history_au;
            DROP TABLE clipboard_fts; CREATE TABLE clipboard_fts(clipboard_fts TEXT CHECK(clipboard_fts!='optimize'));").unwrap();
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

#[cfg(test)]
mod transition_tests {
    use super::super::{
        clipboard_repo::{ClipboardRepository, SqliteClipboardRepository},
        migrations,
    };
    use super::*;
    use crate::{domain::models::ClipboardEntry, infrastructure::encryption};
    use std::sync::{Arc, Mutex};

    fn seal(value: &str) -> Result<encryption::EncryptedPayload, EncryptionError> {
        encryption::encrypt_with_key(
            value,
            || Ok([37; 32]),
            |nonce| {
                nonce.fill(17);
                Ok(())
            },
        )
        .map(encryption::EncryptedPayload::for_test)
    }
    fn reject_maintenance(_: &Connection) -> Result<(), String> {
        Err("UnexpectedIndexMaintenance".into())
    }
    fn prepare_then_fail(conn: &Connection) -> Result<(), String> {
        require_secure_search(conn)?;
        conn.execute("INSERT INTO missing_cleanup_table VALUES(1)", [])
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    fn repository() -> SqliteClipboardRepository {
        let mut conn = Connection::open_in_memory().unwrap();
        migrations::run_migrations(&mut conn).unwrap();
        let mut repo = SqliteClipboardRepository::new(Arc::new(Mutex::new(conn)));
        repo.encrypt_text = seal;
        repo
    }
    fn entry() -> ClipboardEntry {
        serde_json::from_value(serde_json::json!({
            "id": 0,
            "content_type": "text",
            "content": "manualprotected",
            "preview": "summary",
            "source_app": "fixture",
            "timestamp": 1,
            "is_pinned": false,
            "tags": ["sensitive"],
        }))
        .unwrap()
    }
    fn seed(repo: &SqliteClipboardRepository, tags: &str) {
        let conn = repo.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO clipboard_history(id,content_type,content,preview,html_content,source_app,timestamp,tags,ocr_text)
             VALUES(1,'text','manualprotected','summary','<p>body</p>','fixture',1,?1,'qvx')",
            [tags],
        )
        .unwrap();
    }
    fn shadow(conn: &Connection) -> Vec<(String, i64, Vec<u8>)> {
        let mut result = Vec::new();
        for (table, id, column) in [
            ("clipboard_fts_data", "id", "block"),
            ("clipboard_fts_idx", "segid", "term"),
        ] {
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {id},{column} FROM {table} ORDER BY {id},{column}"
                ))
                .unwrap();
            result.extend(
                stmt.query_map([], |r| Ok((table.to_owned(), r.get(0)?, r.get(1)?)))
                    .unwrap()
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .unwrap(),
            );
        }
        result
    }
    fn has_secret(conn: &Connection) -> bool {
        shadow(conn)
            .iter()
            .any(|(_, _, b)| b.windows(3).any(|w| w == b"qvx"))
    }
    fn fields(conn: &Connection) -> (String, String, Option<String>, String, Option<String>) {
        conn.query_row(
            "SELECT content,preview,html_content,tags,ocr_text FROM clipboard_history WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap()
    }
    #[test]
    fn visibility_planning_matches_the_five_field_sql_function() {
        let repo = repository();
        let conn = repo.conn.lock().unwrap();
        let cipher = seal("value").unwrap().into_string();
        for tags in [
            None,
            Some("[]"),
            Some(r#"[" sensitive "]"#),
            Some(r#"["\u0073ensitive"]"#),
            Some("invalid"),
            Some(r#"["密码"]"#),
        ] {
            for protected_column in 0..5 {
                let mut f = [
                    Some("linux: ordinary"),
                    Some("dpapi: ordinary"),
                    None,
                    Some("plain OCR"),
                ];
                if protected_column < 4 {
                    f[protected_column] = Some(&cipher);
                }
                let planned = migrations::search_visible(tags, f);
                let actual: bool = conn
                    .query_row(
                        "SELECT dc_search_visible(?1,?2,?3,?4,?5)",
                        rusqlite::params![tags, f[0], f[1], f[2], f[3]],
                        |r| r.get(0),
                    )
                    .unwrap();
                assert_eq!(planned, actual);
                if protected_column < 4 {
                    assert!(!planned);
                }
            }
        }
        assert!(migrations::search_visible(
            Some("[]"),
            [Some("dpapi: ordinary"), Some("linux: ordinary"), None, None]
        ));
        let typed: bool = conn
            .query_row("SELECT dc_search_visible(42,'a','b',NULL,NULL)", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(!typed);
    }
    #[test]
    fn incremental_delete_removes_shadow_secret_and_preserves_shared_terms_with_format_five() {
        let repo = repository();
        seed(&repo, "[]");
        let conn = repo.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO clipboard_history(content_type,content,preview,source_app,timestamp,tags)
             VALUES('text','manualprotected public','summary','fixture',2,'[]')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO clipboard_fts(clipboard_fts) VALUES('optimize')",
            [],
        )
        .unwrap();
        assert!(has_secret(&conn));
        atomic(&conn, |c| {
            let cipher = seal("value").unwrap().into_string();
            c.execute(
                "UPDATE clipboard_history SET content=?1,preview=?1,html_content=?1,ocr_text=?1,tags='[\"sensitive\"]' WHERE id=1",
                [cipher],
            )
            .map_err(|e| e.to_string())?;
            c.execute_batch("SAVEPOINT verify_postings_flush; RELEASE verify_postings_flush;")
                .map_err(|e| e.to_string())?;
            assert!(!has_secret(c), "secure-delete must remove old postings before commit");
            Ok(())
        })
        .unwrap();
        let remaining: i64 = conn
            .query_row(
                "SELECT count(*) FROM clipboard_fts WHERE clipboard_fts MATCH 'manualprotected'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(remaining, 1);
        let private: i64 = conn
            .query_row(
                "SELECT count(*) FROM clipboard_fts WHERE clipboard_fts MATCH 'qvx'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(private, 0);
        conn.execute(
            "INSERT INTO clipboard_fts(clipboard_fts,rank) VALUES('integrity-check',1)",
            [],
        )
        .unwrap();
        let version: i64 = conn
            .query_row(
                "SELECT v FROM clipboard_fts_config WHERE k='version'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(version, 5);
        conn.execute(
            "CREATE VIRTUAL TABLE terms USING fts5vocab(clipboard_fts,'instance')",
            [],
        )
        .unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT count(*) FROM terms WHERE doc=1 OR term='qvx'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        assert!(
            conn.query_row(
                "SELECT count(*) FROM terms WHERE doc=2 AND term='man'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap()
                > 0
        );
    }
    #[test]
    fn configuration_failure_rejects_before_mutation_and_preserves_cached_results() {
        let mut repo = repository();
        seed(&repo, "[]");
        let cached = repo.get_history(10, 0, None).unwrap();
        let (before, index) = {
            let c = repo.conn.lock().unwrap();
            (fields(&c), shadow(&c))
        };
        repo.prepare_search = prepare_then_fail;
        {
            let c = repo.conn.lock().unwrap();
            assert!(repo.encrypt_entry_with_conn(&c, 1).is_err());
            assert_eq!(fields(&c), before);
            assert_eq!(shadow(&c), index);
        }
        assert_eq!(
            repo.get_history(10, 0, None).unwrap()[0].content,
            cached[0].content
        );
    }

    fn storage_snapshot(conn: &Connection) -> Vec<(String, String)> {
        let mut result = Vec::new();
        for (table, expression) in [
            ("clipboard_fts_data", "id||':'||quote(block)"),
            ("clipboard_fts_idx", "segid||':'||quote(term)||':'||pgno"),
            ("clipboard_fts_docsize", "id||':'||quote(sz)"),
            ("clipboard_fts_config", "k||':'||quote(v)"),
        ] {
            let mut statement = conn.prepare(&format!("SELECT {expression} FROM {table} ORDER BY 1")).unwrap();
            result.extend(statement.query_map([], |row| Ok((table.to_owned(), row.get::<_, String>(0)?)))
                .unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap());
        }
        result
    }

    #[test]
    fn flushed_incremental_delete_rolls_back_fields_all_shadow_tables_and_cached_history() {
        let repo = repository();
        seed(&repo, "[]");
        let cached = repo.get_history(10, 0, None).unwrap();
        let conn = repo.conn.lock().unwrap();
        let before = fields(&conn);
        let index = storage_snapshot(&conn);
        assert!(has_secret(&conn));
        let result: Result<(), String> = atomic(&conn, |connection| {
            repo.encrypt_entry_with_conn(connection, 1)?;
            connection.execute_batch("SAVEPOINT flush_then_fail; RELEASE flush_then_fail;")
                .map_err(|error| error.to_string())?;
            assert!(!has_secret(connection));
            assert_ne!(storage_snapshot(connection), index);
            connection.execute("INSERT INTO missing_after_flush VALUES(1)", [])
                .map_err(|error| error.to_string())?;
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(fields(&conn), before);
        assert_eq!(storage_snapshot(&conn), index);
        assert!(has_secret(&conn));
        drop(conn);
        assert_eq!(repo.get_history(10, 0, None).unwrap()[0].content, cached[0].content);
    }
    #[test]
    fn protected_new_and_legacy_protected_repeat_do_not_run_global_maintenance() {
        let mut repo = repository();
        repo.prepare_search = reject_maintenance;
        let id = repo.save(&entry(), None).unwrap();
        assert!(id > 0);
        let mut repeat = entry();
        repeat.id = id;
        assert_eq!(repo.save(&repeat, None).unwrap(), id);
        repo.update_entry_content(id, "changed protected", "updated")
            .unwrap();
        let legacy = repository();
        seed(&legacy, "[\"sensitive\"]");
        let mut repeated = entry();
        repeated.id = 1;
        let mut legacy = legacy;
        legacy.prepare_search = reject_maintenance;
        assert_eq!(legacy.save(&repeated, None).unwrap(), 1);
        let c = legacy.conn.lock().unwrap();
        assert!(encryption::has_ciphertext_envelope(&fields(&c).0));
        c.execute(
            "INSERT INTO clipboard_fts(clipboard_fts,rank) VALUES('integrity-check',1)",
            [],
        )
        .unwrap();
    }
    #[test]
    fn actual_capture_engine_manual_sensitive_duplicate_skips_global_maintenance() {
        use crate::capture::*;
        use std::collections::VecDeque;
        let mut repo = repository();
        repo.prepare_search = reject_maintenance;
        let id = repo.save(&entry(), None).unwrap();
        assert_eq!(id, 1);
        let directory = std::env::temp_dir();
        let store = repo.capture_store(&directory);
        let session = Mutex::new(VecDeque::new());
        let context = CaptureContext {
            source: CaptureSource {
                app_name: "fixture".into(),
                process_path: None,
            },
            timestamp: 2,
            settings: CaptureSettings {
                persistent: true,
                deduplicate: true,
                ..Default::default()
            },
            session: &session,
            paused: false,
            sequential_echo: None,
        };
        let result = CaptureEngine::capture_frame(
            &context,
            CaptureFrame {
                data: CaptureData::Text("manualprotected".into()),
                metadata: Default::default(),
            },
            &store,
            &mut (),
        );
        let CaptureOutcome::Stored(capture) = result else {
            panic!("actual sensitive duplicate must commit without index maintenance");
        };
        assert_eq!(capture.entry.id, 1);
        assert!(capture.entry.tags.iter().any(|t| t == "sensitive"));
    }
    #[test]
    fn save_transition_cleanup_failure_rolls_back_new_tags_fields_and_shadow() {
        let mut repo = repository();
        seed(&repo, "[]");
        repo.prepare_search = reject_maintenance;
        let (before, index) = {
            let c = repo.conn.lock().unwrap();
            (fields(&c), shadow(&c))
        };
        let mut value = entry();
        value.id = 1;
        assert_eq!(
            repo.save(&value, None).unwrap_err(),
            "UnexpectedIndexMaintenance"
        );
        let c = repo.conn.lock().unwrap();
        assert_eq!(fields(&c), before);
        assert_eq!(shadow(&c), index);
    }
    #[test]
    fn readable_envelope_with_ordinary_tags_preserves_protection_without_maintenance() {
        let mut repo = repository();
        seed(&repo, "[]");
        repo.prepare_search = reject_maintenance;
        {
            let c = repo.conn.lock().unwrap();
            c.execute(
                "UPDATE clipboard_history SET content=?1 WHERE id=1",
                [seal("manualprotected").unwrap().into_string()],
            )
            .unwrap();
            require_secure_search(&c).unwrap();
        }
        let mut value = entry();
        value.id = 1;
        assert_eq!(repo.save(&value, None).unwrap(), 1);
        let c = repo.conn.lock().unwrap();
        let stored = fields(&c);
        assert!(encryption::has_ciphertext_envelope(&stored.0));
        assert!(stored
            .4
            .as_deref()
            .is_some_and(encryption::has_ciphertext_envelope));
        assert_eq!(
            c.query_row(
                "SELECT count(*) FROM clipboard_search_content WHERE id=1",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
    }
    #[test]
    fn encrypted_body_with_ordinary_tags_stays_unindexed_and_unavailable_mutation_fails_closed() {
        let mut repo = repository();
        seed(&repo, "[]");
        repo.prepare_search = reject_maintenance;
        {
            let c = repo.conn.lock().unwrap();
            let cipher = format!(
                "linux:{}",
                base64::Engine::encode(&base64::engine::general_purpose::STANDARD, [0u8; 28])
            );
            c.execute(
                "UPDATE clipboard_history SET content=?1 WHERE id=1",
                [cipher],
            )
            .unwrap();
            require_secure_search(&c).unwrap();
        }
        let mut value = entry();
        value.id = 1;
        assert_eq!(
            repo.save(&value, None).unwrap_err(),
            "ProtectedRecordUnavailable"
        );
        let c = repo.conn.lock().unwrap();
        assert!(encryption::has_ciphertext_envelope(&fields(&c).0));
        assert_eq!(
            c.query_row(
                "SELECT count(*) FROM clipboard_search_content WHERE id=1",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
    }
}
