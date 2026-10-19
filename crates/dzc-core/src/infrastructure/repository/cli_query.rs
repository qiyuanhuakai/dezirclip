//! CLI queries keep pagination after filtering and avoid loading payloads for metadata.
use super::clipboard_repo::SqliteClipboardRepository;
use crate::domain::models::ClipboardEntry;
use rusqlite::functions::FunctionFlags;

#[derive(Default)]
pub struct CliQuery<'a> {
    pub limit: i32,
    pub offset: i32,
    pub kind: Option<&'a str>,
    pub tag: Option<&'a str>,
    pub pinned: bool,
    pub metadata: bool,
    pub exclude_sensitive: bool,
    pub search: Option<(&'a str, &'a str)>,
    pub id: Option<i64>,
    pub latest: bool,
}

impl SqliteClipboardRepository {
    pub fn ensure_export_destination(&self, target: &std::path::Path) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|error| error.to_string())?;
        let Some(database) = conn.path().filter(|path| !path.is_empty()) else {
            return Ok(());
        };
        let target = crate::infrastructure::attachments::resolved_path(target)?;
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let database = crate::infrastructure::attachments::resolved_path(
                std::path::Path::new(&format!("{database}{suffix}")),
            )?;
            if target == database
                || (cfg!(target_os = "windows")
                    && target
                        .to_string_lossy()
                        .eq_ignore_ascii_case(&database.to_string_lossy()))
            {
                return Err(
                    "backup destination must not replace the active database or its journal files"
                        .to_string(),
                );
            }
        }
        Ok(())
    }

    /// Import atomically with original IDs, including rows absent from the target.
    pub fn import_cli_entries(
        &self,
        entries: &[ClipboardEntry],
        replace: bool,
    ) -> Result<(), String> {
        let mut seen = std::collections::HashSet::new();
        if entries
            .iter()
            .any(|entry| entry.id <= 0 || !seen.insert(entry.id))
        {
            return Err("import: entry IDs must be unique positive integers".to_string());
        }
        let mut conn = self.conn.lock().map_err(|e| e.to_string())?;
        let transaction = conn.transaction().map_err(|e| e.to_string())?;
        if replace {
            transaction
                .execute("DELETE FROM entry_tags", [])
                .map_err(|e| e.to_string())?;
            transaction
                .execute("DELETE FROM clipboard_history", [])
                .map_err(|e| e.to_string())?;
        }
        for entry in entries {
            // Materialize the requested primary key before using the shared update path.
            transaction.execute("INSERT INTO clipboard_history(id,content_type,content,source_app,timestamp,preview,is_pinned,tags)
                VALUES(?1,'text','','',0,'',0,'[]') ON CONFLICT(id) DO NOTHING",[entry.id]).map_err(|e|e.to_string())?;
            self.save_with_conn(&transaction, entry, None)?;
            transaction
                .execute(
                    "UPDATE clipboard_history SET is_pinned=?1,pinned_order=?2,use_count=?3,
                content_kinds=?4 WHERE id=?5",
                    rusqlite::params![
                        entry.is_pinned,
                        entry.pinned_order,
                        entry.use_count,
                        serde_json::to_string(&entry.content_kinds).map_err(|e| e.to_string())?,
                        entry.id
                    ],
                )
                .map_err(|e| e.to_string())?;
        }
        transaction.commit().map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn query_cli(&self, query: &CliQuery<'_>) -> Result<Vec<ClipboardEntry>, String> {
        self.query_cli_with_decoding(query, false)
    }

    pub fn query_cli_for_export(
        &self,
        query: &CliQuery<'_>,
    ) -> Result<Vec<ClipboardEntry>, String> {
        if query.metadata {
            return Err("export requires complete payloads".to_string());
        }
        self.query_cli_with_decoding(query, true)
    }

    fn query_cli_with_decoding(
        &self,
        query: &CliQuery<'_>,
        strict_decryption: bool,
    ) -> Result<Vec<ClipboardEntry>, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let search = query.search.map(|(q, m)| (q.to_owned(), m.to_owned()));
        let regex = match &search {
            Some((q, m)) if m == "regex" => {
                Some(regex::Regex::new(q).map_err(|e| format!("invalid regex: {e}"))?)
            }
            _ => None,
        };
        conn.create_scalar_function("dzc_matches", 4, FunctionFlags::SQLITE_UTF8, move |ctx| {
            let Some((needle, mode)) = &search else {
                return Ok(true);
            };
            let raw: String = ctx.get(0)?;
            let content = crate::infrastructure::encryption::decrypt_value(&raw).unwrap_or(raw);
            if let Some(re) = &regex {
                return Ok(re.is_match(&content));
            }
            if mode == "fuzzy" {
                return Ok(fuzzy_match(needle, &content));
            }
            let needle = needle.to_lowercase();
            for value in [
                content,
                ctx.get::<String>(1)?,
                ctx.get::<String>(2)?,
                { let raw = ctx.get::<String>(3)?; crate::infrastructure::encryption::decrypt_value(&raw).unwrap_or(raw) },
            ] {
                if value.to_lowercase().contains(&needle) {
                    return Ok(true);
                }
            }
            Ok(false)
        })
        .map_err(|e| e.to_string())?;
        let fields = if query.metadata {
            "id, content_type, '', CASE WHEN html_content IS NOT NULL THEN '' END, source_app, timestamp, CASE WHEN preview LIKE 'dpapi:%' OR preview LIKE 'linux:%' THEN preview ELSE substr(preview,1,500) END, is_pinned, tags, use_count, is_external, pinned_order, source_app_path, NULL, ocr_status, content_kinds"
        } else {
            "id, content_type, content, html_content, source_app, timestamp, preview, is_pinned, tags, use_count, is_external, pinned_order, source_app_path, ocr_text, ocr_status, content_kinds"
        };
        let order = if query.latest {
            "timestamp DESC, id DESC"
        } else {
            "is_pinned DESC, pinned_order DESC, timestamp DESC, id DESC"
        };
        let fts = query
            .search
            .filter(|(_, m)| *m == "fts5")
            .map(|(q, _)| crate::services::search::escape_fts5_term(q));
        let matching = if fts.is_some() {
            "id IN (SELECT rowid FROM clipboard_fts WHERE clipboard_fts MATCH ?9)"
        } else if query.search.is_some() {
            "dzc_matches(content,source_app,tags,COALESCE(ocr_text,''))"
        } else {
            "1"
        };
        let sql = format!("SELECT {fields} FROM clipboard_history WHERE
            (?1 IS NULL OR content_type=?1) AND (?2 IS NULL OR EXISTS(SELECT 1 FROM json_each(tags) WHERE value=?2))
            AND (?3=0 OR is_pinned=1) AND (?4 IS NULL OR id=?4)
            AND (?5=0 OR (NOT EXISTS(SELECT 1 FROM json_each(tags) WHERE lower(value) IN ('sensitive','密码'))
                AND content NOT LIKE 'dpapi:%' AND content NOT LIKE 'linux:%'
                AND preview NOT LIKE 'dpapi:%' AND preview NOT LIKE 'linux:%'
                AND COALESCE(html_content,'') NOT LIKE 'dpapi:%' AND COALESCE(html_content,'') NOT LIKE 'linux:%'
                AND COALESCE(ocr_text,'') NOT LIKE 'dpapi:%' AND COALESCE(ocr_text,'') NOT LIKE 'linux:%'))
            AND {matching} ORDER BY {order} LIMIT ?6 OFFSET ?7");
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        // Numbered bindings leave room for the optional FTS expression.
        stmt.raw_bind_parameter(1, query.kind)
            .map_err(|e| e.to_string())?;
        stmt.raw_bind_parameter(2, query.tag)
            .map_err(|e| e.to_string())?;
        stmt.raw_bind_parameter(3, query.pinned)
            .map_err(|e| e.to_string())?;
        stmt.raw_bind_parameter(4, query.id)
            .map_err(|e| e.to_string())?;
        stmt.raw_bind_parameter(5, query.exclude_sensitive)
            .map_err(|e| e.to_string())?;
        stmt.raw_bind_parameter(6, query.limit)
            .map_err(|e| e.to_string())?;
        stmt.raw_bind_parameter(7, query.offset)
            .map_err(|e| e.to_string())?;
        if let Some(fts) = fts {
            stmt.raw_bind_parameter(9, fts).map_err(|e| e.to_string())?;
        }
        let mut rows = stmt.raw_query();
        let mut result = Vec::new();
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let id: i64 = row.get(0).map_err(|e| e.to_string())?;
            let decode_value = |value: String| -> Result<String, String> {
                if !strict_decryption {
                    return Ok(self.maybe_decrypt_text(&value));
                }
                match crate::infrastructure::encryption::decode_stored_text(&value) {
                    crate::infrastructure::encryption::StoredText::Plain(value) => Ok(value.to_owned()),
                    crate::infrastructure::encryption::StoredText::Decrypted(value) => Ok(value),
                    crate::infrastructure::encryption::StoredText::Failed => Err(format!("export: entry id={id} could not be decrypted; restore access to its encryption key before backing up")),
                }
            };
            let decode = |index| -> Result<String, String> {
                let value: String = row.get(index).map_err(|e| e.to_string())?;
                decode_value(value)
            };
            result.push(ClipboardEntry {
                id,
                content_type: row.get(1).map_err(|e| e.to_string())?,
                content: decode(2)?,
                html_content: row
                    .get::<_, Option<String>>(3)
                    .map_err(|e| e.to_string())?
                    .map(decode_value)
                    .transpose()?,
                source_app: row.get(4).map_err(|e| e.to_string())?,
                timestamp: row.get(5).map_err(|e| e.to_string())?,
                preview: decode(6)?
                    .chars()
                    .take(if query.metadata { 500 } else { usize::MAX })
                    .collect(),
                is_pinned: row.get(7).map_err(|e| e.to_string())?,
                tags: serde_json::from_str(&row.get::<_, String>(8).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?,
                use_count: row.get(9).unwrap_or(0),
                is_external: row.get(10).unwrap_or(false),
                pinned_order: row.get(11).unwrap_or(0),
                source_app_path: row.get(12).unwrap_or(None),
                file_preview_exists: true,
                content_kinds: serde_json::from_str(
                    &row.get::<_, String>(15)
                        .unwrap_or_else(|_| "[]".to_string()),
                )
                .map_err(|e| e.to_string())?,
                ocr_text: row.get::<_, Option<String>>(13).map_err(|error| error.to_string())?.map(decode_value).transpose()?,
                ocr_status: row.get(14).unwrap_or(None),
            });
        }
        Ok(result)
    }
}

fn fuzzy_match(pattern: &str, content: &str) -> bool {
    if pattern.is_empty() {
        return false;
    }
    let pattern = pattern.to_lowercase();
    let mut chars = pattern.chars().peekable();
    for ch in content.to_lowercase().chars() {
        if chars.peek() == Some(&ch) {
            chars.next();
        }
    }
    chars.peek().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use std::sync::{Arc, Mutex};

    #[test]
    fn active_database_and_journal_paths_cannot_be_backup_destinations() {
        let directory =
            std::env::temp_dir().join(format!("dezirclip-export-target-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("history.db");
        let connection = crate::database::init_db(path.to_str().unwrap()).unwrap();
        let repo = SqliteClipboardRepository::new(Arc::new(Mutex::new(connection)));
        let before = std::fs::read(&path).unwrap();
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let target = directory.join(format!("history.db{suffix}"));
            assert!(repo
                .ensure_export_destination(&target)
                .unwrap_err()
                .contains("active database"));
        }
        assert!(repo
            .ensure_export_destination(&directory.join("not-created").join("..").join("history.db"))
            .is_err());
        assert!(repo
            .ensure_export_destination(&directory.join("backups").join("history.json"))
            .is_ok());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        drop(repo);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn filters_precede_pagination_and_metadata_omits_payload() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        super::super::migrations::run_migrations(&mut conn).unwrap();
        for (id, content, tags, pinned) in [
            (1, "alpha beta", "[\"work\"]", 0),
            (2, "gamma", "[]", 1),
            (3, "secret", "[\"sensitive\"]", 0),
        ] {
            conn.execute("INSERT INTO clipboard_history(id,content_type,content,source_app,timestamp,preview,is_pinned,tags) VALUES(?1,'text',?2,'test',?1,?2,?3,?4)",params![id,content,pinned,tags]).unwrap();
        }
        let repo = SqliteClipboardRepository::new(Arc::new(Mutex::new(conn)));
        let rows = repo
            .query_cli(&CliQuery {
                limit: 1,
                tag: Some("work"),
                metadata: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(rows[0].id, 1);
        assert!(rows[0].content.is_empty());
        let rows = repo
            .query_cli(&CliQuery {
                limit: 20,
                exclude_sensitive: true,
                search: Some(("a.*b", "regex")),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, 1);
        let latest = repo
            .query_cli(&CliQuery {
                limit: 1,
                latest: true,
                exclude_sensitive: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(latest[0].id, 2);
        for mode in ["contains", "fuzzy", "regex", "fts5"] {
            for offset in [0, 1] {
                assert!(
                    repo.query_cli(&CliQuery {
                        limit: 1,
                        offset,
                        exclude_sensitive: true,
                        search: Some(("secret", mode)),
                        ..Default::default()
                    })
                    .unwrap()
                    .is_empty(),
                    "{mode}/{offset}"
                );
            }
        }
    }

    #[test]
    fn search_kind_precedes_limit_and_offset_for_every_mode() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        super::super::migrations::run_migrations(&mut conn).unwrap();
        for (id, kind) in [
            (1, "code"),
            (2, "text"),
            (3, "code"),
            (4, "url"),
            (5, "text"),
        ] {
            conn.execute(
                "INSERT INTO clipboard_history(id,content_type,content,source_app,timestamp,preview,is_pinned,tags)
                 VALUES(?1,?2,?3,'test',?1,?3,0,'[]')",
                params![id, kind, format!("needle record {id}")],
            )
            .unwrap();
        }
        let repo = SqliteClipboardRepository::new(Arc::new(Mutex::new(conn)));
        for (mode, needle) in [
            ("contains", "needle"),
            ("fuzzy", "ndl"),
            ("regex", "n.*dle"),
            ("fts5", "needle"),
        ] {
            for (offset, expected) in [(0, vec![3]), (1, vec![1]), (2, vec![])] {
                let rows = repo
                    .query_cli(&CliQuery {
                        kind: Some("code"),
                        limit: 1,
                        offset,
                        metadata: true,
                        search: Some((needle, mode)),
                        ..Default::default()
                    })
                    .unwrap();
                assert_eq!(
                    rows.iter().map(|row| row.id).collect::<Vec<_>>(),
                    expected,
                    "mode={mode}, offset={offset}"
                );
            }
        }
    }
}
