use crate::domain::models::ClipboardEntry;
use crate::infrastructure::encryption;
use rusqlite::{params, Connection};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

pub trait TagRepository {
    fn set_color(&self, name: &str, color: Option<String>) -> Result<(), String>;
    fn get_colors(&self) -> Result<HashMap<String, String>, String>;
    fn get_all_with_counts(&self) -> Result<HashMap<String, i32>, String>;
    fn create(&self, name: &str) -> Result<(), String>;
    fn rename(&self, old_name: &str, new_name: &str) -> Result<(), String>;
    fn delete_globally(
        &self,
        name: &str,
        data_dir: Option<&std::path::Path>,
    ) -> Result<super::sensitive_tags::SensitiveTagDeletion, String>;
    fn get_entries_by_tag(&self, tag: &str) -> Result<Vec<ClipboardEntry>, String>;
    fn update_entry_tags(&self, id: i64, tags: Vec<String>) -> Result<(), String>;
}

#[derive(Clone)]
pub struct SqliteTagRepository {
    pub(super) conn: Arc<Mutex<Connection>>,
    encrypt_text: fn(&str) -> Result<encryption::EncryptedPayload, encryption::EncryptionError>,
}

impl SqliteTagRepository {
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn, encrypt_text: encryption::encrypt_sensitive }
    }

    pub(super) fn maybe_decrypt_text(&self, value: &str) -> String {
        if encryption::is_encrypted_value(value) {
            encryption::decrypt_value(value).unwrap_or_else(|| value.to_string())
        } else {
            value.to_string()
        }
    }

    pub(super) fn maybe_decrypt_tags(&self, tags: Vec<String>) -> Vec<String> {
        let mut seen: HashSet<String> = HashSet::new();
        let mut decrypted = Vec::new();

        for tag in tags {
            let tag = self.maybe_decrypt_text(&tag);
            let tag = tag.trim();
            if tag.is_empty() {
                continue;
            }

            let tag = tag.to_string();
            if seen.insert(tag.clone()) {
                decrypted.push(tag);
            }
        }

        decrypted
    }

    pub(super) fn refresh_entry_tags_json(conn: &Connection, entry_id: i64) -> Result<(), String> {
        let mut stmt = conn
            .prepare("SELECT tag FROM entry_tags WHERE entry_id = ? ORDER BY tag")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![entry_id], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?;

        let mut tags: Vec<String> = Vec::new();
        for row in rows {
            if let Ok(tag) = row {
                if !tag.trim().is_empty() {
                    tags.push(tag);
                }
            }
        }

        let tags_json = serde_json::to_string(&tags).unwrap_or_else(|_| "[]".to_string());
        conn.execute(
            "UPDATE clipboard_history SET tags = ? WHERE id = ?",
            params![tags_json, entry_id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }
}

impl TagRepository for SqliteTagRepository {
    fn set_color(&self, name: &str, color: Option<String>) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        if let Some(c) = color {
            conn.execute(
                "INSERT INTO saved_tags (name, color) VALUES (?1, ?2) 
                 ON CONFLICT(name) DO UPDATE SET color = ?2",
                params![name, c],
            )
            .map_err(|e| e.to_string())?;
        } else {
            conn.execute(
                "UPDATE saved_tags SET color = NULL WHERE name = ?1",
                params![name],
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn get_colors(&self) -> Result<HashMap<String, String>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT name, color FROM saved_tags WHERE color IS NOT NULL AND color != ''")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?;

        let mut map = HashMap::new();
        for row in rows {
            if let Ok((name, color)) = row {
                let name = self.maybe_decrypt_text(&name);
                map.insert(name, color);
            }
        }
        Ok(map)
    }

    fn get_all_with_counts(&self) -> Result<HashMap<String, i32>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare("SELECT tag, COUNT(*) FROM entry_tags GROUP BY tag")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i32>(1)?))
            })
            .map_err(|e| e.to_string())?;

        let mut tag_counts: HashMap<String, i32> = HashMap::new();
        for row in rows {
            if let Ok((tag, count)) = row {
                let tag = self.maybe_decrypt_text(&tag);
                *tag_counts.entry(tag).or_insert(0) += count;
            }
        }

        // Also include saved tags with 0 count if not present
        let mut stmt_saved = conn
            .prepare("SELECT name FROM saved_tags")
            .map_err(|e| e.to_string())?;
        let saved_rows = stmt_saved
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?;

        for row in saved_rows {
            if let Ok(name) = row {
                let name = self.maybe_decrypt_text(&name);
                tag_counts.entry(name).or_insert(0);
            }
        }

        Ok(tag_counts)
    }

    fn create(&self, name: &str) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT OR IGNORE INTO saved_tags (name) VALUES (?)",
            params![name],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn rename(&self, old_name: &str, new_name: &str) -> Result<(), String> {
        let new_name = new_name.trim();
        if new_name.is_empty()
            || new_name.chars().count() > 80
            || new_name.chars().any(char::is_control)
        {
            return Err("tag names must contain 1..80 printable characters".into());
        }
        if old_name == new_name {
            return Ok(());
        }
        if crate::database::has_sensitive_tag(&[old_name.to_string(), new_name.to_string()]) {
            return Err(
                "system sensitive tags require per-entry updates and cannot be renamed globally"
                    .into(),
            );
        }
        let mut guard = self.conn.lock().map_err(|e| e.to_string())?;
        let conn = guard.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(|error| error.to_string())?;

        // Update saved_tags table: merge color info if exists
        let old_color: Option<String> = conn
            .query_row(
                "SELECT color FROM saved_tags WHERE name = ?",
                params![old_name],
                |row| row.get(0),
            )
            .ok();

        conn.execute(
            "INSERT OR IGNORE INTO saved_tags (name, color) VALUES (?1, ?2)",
            params![new_name, old_color],
        )
        .map_err(|e| e.to_string())?;

        conn.execute("DELETE FROM saved_tags WHERE name = ?", params![old_name])
            .map_err(|error| error.to_string())?;

        // Update entry_tags and refresh JSON cache
        let mut stmt = conn
            .prepare("SELECT entry_id FROM entry_tags WHERE tag = ?")
            .map_err(|e| e.to_string())?;
        let ids: Vec<i64> = stmt
            .query_map(params![old_name], |row| row.get(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<_, _>>()
            .map_err(|error| error.to_string())?;
        drop(stmt);

        for id in ids {
            conn.execute(
                "INSERT OR IGNORE INTO entry_tags (entry_id, tag) VALUES (?1, ?2)",
                params![id, new_name],
            )
            .map_err(|e| e.to_string())?;
            conn.execute(
                "DELETE FROM entry_tags WHERE entry_id = ? AND tag = ?",
                params![id, old_name],
            )
            .map_err(|e| e.to_string())?;
            Self::refresh_entry_tags_json(&conn, id)?;
        }
        conn.commit().map_err(|error| error.to_string())
    }

    fn delete_globally(
        &self,
        name: &str,
        data_dir: Option<&std::path::Path>,
    ) -> Result<super::sensitive_tags::SensitiveTagDeletion, String> {
        self.delete_sensitive_with_contents(name, data_dir)
    }

    fn get_entries_by_tag(&self, tag: &str) -> Result<Vec<ClipboardEntry>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn.prepare(
            "SELECT DISTINCT ch.id, ch.content_type, ch.content, ch.html_content, ch.source_app, ch.timestamp, ch.preview, ch.is_pinned, ch.tags, ch.use_count, ch.is_external, ch.pinned_order, ch.source_app_path, ch.content_kinds, ch.ocr_text, ch.ocr_status
             FROM clipboard_history ch
             INNER JOIN entry_tags et ON ch.id = et.entry_id
             WHERE et.tag = ?
             ORDER BY ch.is_pinned DESC, ch.pinned_order DESC, ch.timestamp DESC",
        ).map_err(|e| e.to_string())?;

        let rows = stmt
            .query_map([tag], |row| {
                let tags_str: String = row.get(8).unwrap_or_else(|_| "[]".to_string());
                let tags =
                    self.maybe_decrypt_tags(serde_json::from_str(&tags_str).unwrap_or_default());
                let content_raw: String = row.get(2)?;
                let html_raw: Option<String> = row.get(3).ok();
                let preview_raw: String = row.get(6)?;
                let content = self.maybe_decrypt_text(&content_raw);
                let preview = self.maybe_decrypt_text(&preview_raw);
                let html_content = html_raw.map(|v| self.maybe_decrypt_text(&v));

                Ok(ClipboardEntry {
                    id: row.get(0)?,
                    content_type: row.get(1)?,
                    content,
                    html_content,
                    source_app: row.get(4)?,
                    timestamp: row.get(5)?,
                    preview,
                    is_pinned: row.get::<_, i32>(7)? == 1,
                    tags,
                    use_count: row.get(9).unwrap_or(0),
                    is_external: row.get::<_, i32>(10)? == 1,
                    pinned_order: row.get(11).unwrap_or(0),
                    source_app_path: row.get(12).unwrap_or(None),
                    file_preview_exists: true, // simplified
                    content_kinds: serde_json::from_str(
                        &row.get::<_, String>(13)
                            .unwrap_or_else(|_| "[]".to_string()),
                    )
                    .unwrap_or_default(),
                    ocr_text: row.get::<_, Option<String>>(14)?.map(|value| self.maybe_decrypt_text(&value)),
                    ocr_status: row.get(15).ok(),
                })
            })
            .map_err(|e| e.to_string())?;

        let mut history = Vec::new();
        for row in rows {
            if let Ok(entry) = row {
                if entry.tags.iter().any(|entry_tag| entry_tag == tag) {
                    history.push(entry);
                }
            }
        }
        Ok(history)
    }

    fn update_entry_tags(&self, id: i64, tags: Vec<String>) -> Result<(), String> {
        let mut guard = self.conn.lock().map_err(|e| e.to_string())?;
        let conn = guard.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(|error| error.to_string())?;
        let mut seen: HashSet<String> = HashSet::new();
        let mut cleaned: Vec<String> = Vec::new();
        for tag in tags {
            let t = tag.trim();
            if t.is_empty() {
                continue;
            }
            let t_owned = t.to_string();
            if seen.insert(t_owned.clone()) {
                cleaned.push(t_owned);
            }
        }

        let old_tags: String = conn.query_row("SELECT tags FROM clipboard_history WHERE id=?1", [id], |row| row.get(0))
            .map_err(|error| error.to_string())?;
        let old_tags: Vec<String> = serde_json::from_str(&old_tags).map_err(|_| encryption::EncryptionError::SensitivePersistenceRejected.to_string())?;
        let directory=conn.path().filter(|path|!path.is_empty()).and_then(|path|std::path::Path::new(path).parent()).map(std::path::Path::to_path_buf);
        let mut cleanup=if crate::database::has_sensitive_tag(&cleaned) {
            Some(super::sensitive_tags::AttachmentCleanup::for_entries(&conn,directory.as_deref(),&HashSet::from([id]))?)
        } else {None};
        let mut protector = super::clipboard_repo::SqliteClipboardRepository::new(self.conn.clone());
        protector.encrypt_text = self.encrypt_text;
        if crate::database::has_sensitive_tag(&cleaned) {
            protector.encrypt_entry_with_conn(&conn, id)?;
        } else if crate::database::has_sensitive_tag(&old_tags) {
            protector.decrypt_entry_with_conn(&conn, id)?;
        }
        conn.execute("DELETE FROM entry_tags WHERE entry_id = ?", params![id])
            .map_err(|e| e.to_string())?;
        for tag in &cleaned {
            conn.execute(
                "INSERT OR IGNORE INTO entry_tags (entry_id, tag) VALUES (?1, ?2)",
                params![id, tag],
            )
            .map_err(|e| e.to_string())?;
        }

        let tags_json = serde_json::to_string(&cleaned).unwrap_or_else(|_| "[]".to_string());
        conn.execute(
            "UPDATE clipboard_history SET tags = ? WHERE id = ?",
            params![tags_json, id],
        )
        .map_err(|e| e.to_string())?;
        if let Some(cleanup)=&mut cleanup {cleanup.protect_surviving_references(&conn)?;}
        conn.commit().map_err(|error| error.to_string())?;
        if cleanup.is_some_and(|cleanup|!cleanup.finish().is_empty()){return Err("ProtectedAttachmentCleanupIncomplete".into());}
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{SqliteTagRepository, TagRepository};
    use crate::infrastructure::repository::migrations::run_migrations;
    use rusqlite::{params, Connection};
    use std::sync::{Arc, Mutex};

    #[test]
    fn get_entries_by_tag_preserves_ocr_fields() {
        let mut conn = Connection::open_in_memory().expect("open memory db");
        run_migrations(&mut conn).expect("run migrations");
        conn.execute(
            "INSERT INTO clipboard_history (id, content_type, content, source_app, timestamp, preview, tags, content_kinds, ocr_text, ocr_status)
             VALUES (?1, 'image', 'data:image/png;base64,AA==', 'test', 1, 'image', '[\"ocr\"]', '[\"image\"]', 'receipt total', 'done')",
            params![7_i64],
        )
        .expect("insert history");
        conn.execute(
            "INSERT INTO entry_tags (entry_id, tag) VALUES (?1, ?2)",
            params![7_i64, "ocr"],
        )
        .expect("insert tag");

        let repo = SqliteTagRepository::new(Arc::new(Mutex::new(conn)));
        let entries = repo.get_entries_by_tag("ocr").expect("entries by tag");

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].content_kinds, vec!["image".to_string()]);
        assert_eq!(entries[0].ocr_text.as_deref(), Some("receipt total"));
        assert_eq!(entries[0].ocr_status.as_deref(), Some("done"));
    }
}

#[cfg(test)]
mod protection_transaction_tests {
    use super::*;
    use super::super::clipboard_repo::SqliteClipboardRepository;
    #[cfg(all(target_os = "windows", not(feature = "portable")))]
    use super::super::clipboard_repo::ClipboardRepository;
    #[cfg(all(target_os = "windows", not(feature = "portable")))]
    use super::super::cli_query::CliQuery;
    use base64::Engine;

    fn repository() -> (SqliteTagRepository, SqliteClipboardRepository, Arc<Mutex<Connection>>) {
        let mut connection = Connection::open_in_memory().unwrap();
        super::super::migrations::run_migrations(&mut connection).unwrap();
        connection.execute_batch(r#"INSERT INTO clipboard_history(id,content_type,content,preview,html_content,source_app,timestamp,tags,ocr_text,ocr_status)
            VALUES(1,'rich_text','privateprobe','summary','<p>private text</p>','test',1,'["work"]','privateocr','done');
            INSERT INTO entry_tags VALUES(1,'work');"#).unwrap();
        let connection = Arc::new(Mutex::new(connection));
        (SqliteTagRepository::new(connection.clone()), SqliteClipboardRepository::new(connection.clone()), connection)
    }

    fn snapshot(conn: &Connection) -> (String, String, Option<String>, String, Option<String>) {
        conn.query_row("SELECT content,preview,html_content,tags,ocr_text FROM clipboard_history WHERE id=1", [],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?))).unwrap()
    }

    fn fail_key(_: &str) -> Result<encryption::EncryptedPayload, encryption::EncryptionError> {
        Err(encryption::EncryptionError::KeyUnavailable)
    }

    fn seal(value: &str) -> Result<encryption::EncryptedPayload, encryption::EncryptionError> {
        Ok(encryption::EncryptedPayload::for_test(format!("sealed:{value}")))
    }

    #[test]
    fn sensitive_tag_encryption_failure_preserves_both_tag_indexes_and_fields() {
        let (mut tags, _, shared) = repository();
        tags.encrypt_text = fail_key;
        let before = snapshot(&shared.lock().unwrap());
        assert_eq!(tags.update_entry_tags(1, vec!["sensitive".to_string()]).unwrap_err(), "KeyUnavailable");
        let conn = shared.lock().unwrap();
        assert_eq!(snapshot(&conn), before);
        let tag: String = conn.query_row("SELECT tag FROM entry_tags WHERE entry_id=1", [], |row| row.get(0)).unwrap();
        assert_eq!(tag, "work");
    }

    #[test]
    fn final_tag_sql_failure_rolls_back_field_protection_and_fts_changes() {
        let (mut tags, _, shared) = repository();
        tags.encrypt_text = seal;
        shared.lock().unwrap().execute_batch("CREATE TRIGGER reject_tag BEFORE UPDATE OF tags ON clipboard_history WHEN instr(new.tags,'sensitive')>0 BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
        let before = snapshot(&shared.lock().unwrap());
        assert!(tags.update_entry_tags(1, vec!["sensitive".to_string()]).is_err());
        let conn = shared.lock().unwrap();
        assert_eq!(snapshot(&conn), before);
        let matches: i64 = conn.query_row("SELECT COUNT(*) FROM clipboard_fts WHERE clipboard_fts MATCH 'privateprobe'", [], |row| row.get(0)).unwrap();
        assert_eq!(matches, 1);
        let tag: String = conn.query_row("SELECT tag FROM entry_tags WHERE entry_id=1", [], |row| row.get(0)).unwrap();
        assert_eq!(tag, "work");
    }

    #[test]
    fn deferred_tag_commit_failure_restores_fields_tags_index_and_cached_history() {
        use super::super::clipboard_repo::ClipboardRepository;
        let (mut tags, repo, shared) = repository();
        tags.encrypt_text = |value| {
            encryption::encrypt_with_key(
                value,
                || Ok([37; 32]),
                |nonce| {
                    nonce.fill(17);
                    Ok(())
                },
            )
            .map(encryption::EncryptedPayload::for_test)
        };
        let cached = repo.get_history(10, 0, None).unwrap();
        let before = snapshot(&shared.lock().unwrap());
        let shadow = |conn: &Connection| {
            conn.prepare("SELECT id,block FROM clipboard_fts_data ORDER BY id")
                .unwrap()
                .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap()
        };
        let index_before = shadow(&shared.lock().unwrap());
        shared.lock().unwrap().execute_batch(
            "PRAGMA foreign_keys=ON;
             CREATE TABLE parent(id INTEGER PRIMARY KEY);
             CREATE TABLE delayed_failure(
                 parent INTEGER REFERENCES parent(id) DEFERRABLE INITIALLY DEFERRED
             );
             CREATE TRIGGER fail_commit AFTER UPDATE OF tags ON clipboard_history
             WHEN instr(new.tags,'sensitive')>0
             BEGIN INSERT INTO delayed_failure VALUES(42); END;",
        ).unwrap();
        assert!(tags.update_entry_tags(1, vec!["sensitive".into()]).is_err());
        {
            let conn = shared.lock().unwrap();
            assert_eq!(snapshot(&conn), before);
            assert_eq!(shadow(&conn), index_before);
            assert_eq!(
                conn.query_row("SELECT tag FROM entry_tags WHERE entry_id=1", [], |r| {
                    r.get::<_, String>(0)
                })
                .unwrap(),
                "work"
            );
            conn.execute(
                "INSERT INTO clipboard_fts(clipboard_fts,rank) VALUES('integrity-check',1)",
                [],
            )
            .unwrap();
            assert_eq!(
                conn.query_row(
                    "SELECT count(*) FROM clipboard_fts WHERE clipboard_fts MATCH 'privateocr'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                1
            );
        }
        assert_eq!(
            repo.get_history(10, 0, None).unwrap()[0].content,
            cached[0].content
        );
    }

    #[test]
    fn removing_sensitive_tag_cannot_overwrite_unavailable_protected_ocr() {
        let (tags, _, shared) = repository();
        let cipher = format!("linux:{}", base64::engine::general_purpose::STANDARD.encode([0u8;28]));
        { let conn=shared.lock().unwrap(); conn.execute(r#"UPDATE clipboard_history SET tags='["sensitive"]',ocr_text=?1 WHERE id=1"#, [&cipher]).unwrap(); }
        let before = snapshot(&shared.lock().unwrap());
        assert_eq!(tags.update_entry_tags(1, vec!["work".to_string()]).unwrap_err(), "ProtectedRecordUnavailable");
        assert_eq!(snapshot(&shared.lock().unwrap()), before);
    }

    #[test]
    fn sensitive_attachment_conversion_preserves_the_original_source() {
        let (mut tags, _, shared) = repository();
        tags.encrypt_text = seal;
        let source = std::env::temp_dir().join(format!("dezirclip-protected-source-{}", uuid::Uuid::new_v4()));
        std::fs::write(&source, b"original attachment").unwrap();
        shared.lock().unwrap().execute("UPDATE clipboard_history SET content_type='image',content=?1,is_external=1 WHERE id=1", [source.to_string_lossy().as_ref()]).unwrap();
        let before = snapshot(&shared.lock().unwrap());
        assert_eq!(tags.update_entry_tags(1, vec!["sensitive".to_string()]).unwrap_err(), "SensitivePersistenceRejected");
        assert_eq!(snapshot(&shared.lock().unwrap()), before);
        assert_eq!(std::fs::read(&source).unwrap(), b"original attachment");
        std::fs::remove_file(source).unwrap();
    }

    #[cfg(all(windows, not(feature = "portable")))]
    #[test]
    fn sensitive_transition_protects_ocr_and_invalidates_cached_history_without_indexing_payload() {
        let (tags, repo, shared) = repository();
        let cached = repo.get_history(10,0,None).unwrap();
        assert_eq!(cached[0].tags, vec!["work"]);
        tags.update_entry_tags(1, vec!["sensitive".to_string()]).unwrap();
        let cached = repo.get_history(10,0,None).unwrap();
        assert_eq!(cached[0].tags, vec!["sensitive"]);
        { let conn=shared.lock().unwrap();
          let stored = snapshot(&conn);
          assert!([Some(stored.0),Some(stored.1),stored.2,stored.4].into_iter().flatten().all(|value| encryption::is_encrypted_value(&value)));
          let matches: i64 = conn.query_row("SELECT COUNT(*) FROM clipboard_fts WHERE clipboard_fts MATCH 'privateprobe'", [], |row| row.get(0)).unwrap();
          assert_eq!(matches,0);
          conn.execute("INSERT INTO clipboard_fts(clipboard_fts,rank) VALUES('integrity-check',1)", []).unwrap();
          let secure: i64=conn.query_row("PRAGMA secure_delete",[],|row|row.get(0)).unwrap(); assert_eq!(secure,1);
        }
        let query=CliQuery {limit:20,..Default::default()};
        assert_eq!(repo.query_cli_for_export(&query).unwrap()[0].ocr_text.as_deref(),Some("privateocr"));
        assert_eq!(tags.get_entries_by_tag("sensitive").unwrap()[0].ocr_text.as_deref(),Some("privateocr"));
        assert_eq!(repo.search("privateocr",10).unwrap().len(),1);
        assert!(repo.query_cli(&CliQuery {limit:20,exclude_sensitive:true,..Default::default()}).unwrap().is_empty());
        tags.update_entry_tags(1, vec!["work".to_string()]).unwrap();
        let conn=shared.lock().unwrap();
        let restored=snapshot(&conn); assert_eq!(restored.0,"privateprobe"); assert_eq!(restored.4.as_deref(),Some("privateocr"));
        let matches:i64=conn.query_row("SELECT COUNT(*) FROM clipboard_fts WHERE clipboard_fts MATCH 'privateprobe'",[],|row|row.get(0)).unwrap(); assert_eq!(matches,1);
    }
}
