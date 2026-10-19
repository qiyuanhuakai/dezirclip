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
