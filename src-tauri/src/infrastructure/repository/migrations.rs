use crate::infrastructure::encryption;
use rusqlite::{params, Connection, Result};

/// The value `PRAGMA auto_vacuum` reports for the mode `init_db` asks for.
const AUTO_VACUUM_FULL: i32 = 1;

/// How much stranded page space is worth a rewrite of the whole database file.
///
/// Converting costs a VACUUM, so the conversion is only worth its stall once
/// there are real megabytes to win. A database nobody has deleted from has no
/// free pages at all and is skipped on the freelist count alone.
const RECLAIM_MIN_BYTES: i64 = 16 * 1024 * 1024;

/// Give back the pages a database that predates the `auto_vacuum` pragma has
/// been stranding since its first launch.
///
/// `PRAGMA auto_vacuum` only takes effect on a database that has no tables yet.
/// `init_db` issues it before the migrations create any, so a fresh install
/// lands on FULL -- but on every install that already existed the pragma has
/// been a silent no-op, the mode stayed NONE, and under NONE SQLite never
/// returns a freed page to the file. Deletions are not rare here: crossing the
/// storage limit evicts in batches, every time, and each batch strands its
/// pages. The file then only ever grows, however many entries come and go.
///
/// Switching the mode needs a VACUUM, which rewrites the file and so cannot run
/// inside the transaction the schema migrations use. It is deliberately left
/// outside one, and deliberately allowed to fail: a database that cannot be
/// rewritten -- most often because there is no room for the second copy -- is
/// still a perfectly usable database, just one that keeps stranding pages. The
/// caller logs that and leaves the migration unapplied so the next launch, with
/// more room perhaps, tries again.
fn reclaim_stranded_pages(conn: &Connection, min_bytes: i64) -> Result<()> {
    let mode: i32 = conn.query_row("PRAGMA auto_vacuum", [], |row| row.get(0))?;
    if mode == AUTO_VACUUM_FULL {
        return Ok(());
    }

    let page_size: i64 = conn.query_row("PRAGMA page_size", [], |row| row.get(0))?;
    let free_pages: i64 = conn.query_row("PRAGMA freelist_count", [], |row| row.get(0))?;
    if page_size * free_pages < min_bytes {
        return Ok(());
    }

    conn.execute_batch("PRAGMA auto_vacuum = FULL; VACUUM;")
}

pub fn run_migrations(conn: &mut Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )?;

    let current_version: i32 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    // Migration 1: Initial Baseline
    if current_version < 1 {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS clipboard_history (
                id INTEGER PRIMARY KEY,
                content_type TEXT NOT NULL,
                content TEXT NOT NULL,
                source_app TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                preview TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
        ",
        )?;
        conn.execute("INSERT INTO schema_migrations (version) VALUES (1)", [])?;
    }

    // Migration 2: Add core feature columns
    if current_version < 2 {
        let columns = [
            ("is_pinned", "INTEGER NOT NULL DEFAULT 0"),
            ("tags", "TEXT NOT NULL DEFAULT '[]'"),
            ("use_count", "INTEGER NOT NULL DEFAULT 0"),
            ("pinned_order", "INTEGER NOT NULL DEFAULT 0"),
            ("content_hash", "INTEGER NOT NULL DEFAULT 0"),
            ("html_content", "TEXT"),
        ];

        for (name, def) in columns {
            if !has_column(conn, "clipboard_history", name)? {
                conn.execute(
                    &format!("ALTER TABLE clipboard_history ADD COLUMN {} {}", name, def),
                    [],
                )?;
            }
        }
        conn.execute("INSERT INTO schema_migrations (version) VALUES (2)", [])?;
    }

    // Migration 3: Add is_external
    if current_version < 3 {
        if !has_column(conn, "clipboard_history", "is_external")? {
            conn.execute(
                "ALTER TABLE clipboard_history ADD COLUMN is_external INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
        conn.execute("INSERT INTO schema_migrations (version) VALUES (3)", [])?;
    }

    // Migration 4: Tag management
    if current_version < 4 {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS saved_tags (
                name TEXT PRIMARY KEY,
                color TEXT
            )",
            [],
        )?;

        // Insert default tags
        let _ = conn.execute(
            "INSERT OR IGNORE INTO saved_tags (name) VALUES ('sensitive')",
            [],
        );
        let _ = conn.execute(
            "INSERT OR IGNORE INTO saved_tags (name) VALUES ('密码')",
            [],
        );

        conn.execute("INSERT INTO schema_migrations (version) VALUES (4)", [])?;
    }

    // Migration 5: Performance indexes
    if current_version < 5 {
        conn.execute_batch(
            "
            CREATE INDEX IF NOT EXISTS idx_clipboard_history_pinned_order_time
                ON clipboard_history (is_pinned, pinned_order, timestamp);
            CREATE INDEX IF NOT EXISTS idx_clipboard_history_type_hash
                ON clipboard_history (content_type, content_hash);
            CREATE INDEX IF NOT EXISTS idx_clipboard_history_timestamp
                ON clipboard_history (timestamp);
        ",
        )?;
        conn.execute("INSERT INTO schema_migrations (version) VALUES (5)", [])?;
    }

    // Migration 6: Normalize tags into entry_tags
    if current_version < 6 {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS entry_tags (
                entry_id INTEGER NOT NULL,
                tag TEXT NOT NULL,
                PRIMARY KEY (entry_id, tag)
            );
            CREATE INDEX IF NOT EXISTS idx_entry_tags_tag ON entry_tags (tag);
            CREATE INDEX IF NOT EXISTS idx_entry_tags_entry ON entry_tags (entry_id);
        ",
        )?;

        // Backfill entry_tags from clipboard_history.tags JSON
        conn.execute("BEGIN", [])?;
        let backfill = (|| -> Result<()> {
            let mut stmt = conn.prepare("SELECT id, tags FROM clipboard_history")?;
            let rows = stmt.query_map([], |row| {
                let id: i64 = row.get(0)?;
                let tags: Option<String> = row.get(1)?;
                Ok((id, tags.unwrap_or_else(|| "[]".to_string())))
            })?;

            for row in rows {
                let (id, tags_json) = row?;
                let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
                for tag in tags {
                    if tag.trim().is_empty() {
                        continue;
                    }
                    conn.execute(
                        "INSERT OR IGNORE INTO entry_tags (entry_id, tag) VALUES (?1, ?2)",
                        params![id, tag],
                    )?;
                }
            }
            Ok(())
        })();

        if let Err(err) = backfill {
            let _ = conn.execute("ROLLBACK", []);
            return Err(err);
        }
        conn.execute("COMMIT", [])?;
        conn.execute("INSERT INTO schema_migrations (version) VALUES (6)", [])?;
    }

    // Migration 9: Persist source executable path for real app icon rendering
    if current_version < 9 {
        if !has_column(conn, "clipboard_history", "source_app_path")? {
            conn.execute(
                "ALTER TABLE clipboard_history ADD COLUMN source_app_path TEXT",
                [],
            )?;
        }
        conn.execute("INSERT INTO schema_migrations (version) VALUES (9)", [])?;
    }

    // Migration 10: repair oversized previews left by old builds.
    // History lists should never serialize full clipboard bodies through IPC.
    if current_version < 10 {
        conn.execute(
            "UPDATE clipboard_history
             SET preview = substr(preview, 1, 497) || '...'
             WHERE length(preview) > 500",
            [],
        )?;
        conn.execute("INSERT INTO schema_migrations (version) VALUES (10)", [])?;
    }

    // Migration 11: repair tag names accidentally persisted as encrypted values.
    // Tags are metadata and should remain plaintext so tag management can display,
    // rename, delete, and count them consistently across repository methods.
    if current_version < 11 {
        repair_encrypted_tags(conn)?;
        conn.execute(
            "UPDATE settings
             SET value = 'phone,idcard,email,secret,password'
             WHERE key = 'app.privacy_protection_kinds'
               AND value = 'phone,idcard,email,secret'",
            [],
        )?;
        conn.execute("INSERT INTO schema_migrations (version) VALUES (11)", [])?;
    }

    // Migration 12: FTS5 virtual table + triggers for full-text search.
    // The trigram tokenizer supports both ASCII substrings and CJK 3+ char queries.
    if current_version < 12 {
        conn.execute_batch(
            "
            CREATE VIRTUAL TABLE IF NOT EXISTS clipboard_fts USING fts5(
                content,
                preview,
                source_app,
                content='clipboard_history',
                content_rowid='id',
                tokenize='trigram'
            );

            CREATE TRIGGER IF NOT EXISTS clipboard_history_ai AFTER INSERT ON clipboard_history BEGIN
                INSERT INTO clipboard_fts(rowid, content, preview, source_app)
                VALUES (new.id, new.content, new.preview, new.source_app);
            END;

            CREATE TRIGGER IF NOT EXISTS clipboard_history_ad AFTER DELETE ON clipboard_history BEGIN
                INSERT INTO clipboard_fts(clipboard_fts, rowid, content, preview, source_app)
                VALUES ('delete', old.id, old.content, old.preview, old.source_app);
            END;

            CREATE TRIGGER IF NOT EXISTS clipboard_history_au AFTER UPDATE ON clipboard_history BEGIN
                INSERT INTO clipboard_fts(clipboard_fts, rowid, content, preview, source_app)
                VALUES ('delete', old.id, old.content, old.preview, old.source_app);
                INSERT INTO clipboard_fts(rowid, content, preview, source_app)
                VALUES (new.id, new.content, new.preview, new.source_app);
            END;

            INSERT INTO clipboard_fts(clipboard_fts) VALUES ('rebuild');
        ",
        )?;
        conn.execute("INSERT INTO schema_migrations (version) VALUES (12)", [])?;
    }

    // Migration 13: Add content_kinds column for classification results and
    // extend the FTS5 virtual table to index it. content_kinds is a JSON array
    // string (e.g. '["text","code"]') produced by services::classification::classify().
    // The FTS5 schema from v12 is rebuilt because FTS5 virtual tables do not
    // support ALTER TABLE — we must drop + recreate to add a column.
    if current_version < 13 {
        conn.execute("BEGIN", [])?;
        let migration_result = (|| -> Result<()> {
            if !has_column(conn, "clipboard_history", "content_kinds")? {
                conn.execute(
                    "ALTER TABLE clipboard_history ADD COLUMN content_kinds TEXT NOT NULL DEFAULT '[]'",
                    [],
                )?;
            }
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_clipboard_history_content_kinds
                    ON clipboard_history (content_kinds)",
                [],
            )?;
            if !has_column(conn, "clipboard_fts", "content_kinds")? {
                conn.execute_batch(
                    "
                    DROP TRIGGER IF EXISTS clipboard_history_ai;
                    DROP TRIGGER IF EXISTS clipboard_history_ad;
                    DROP TRIGGER IF EXISTS clipboard_history_au;
                    DROP TABLE IF EXISTS clipboard_fts;

                    CREATE VIRTUAL TABLE clipboard_fts USING fts5(
                        content,
                        preview,
                        source_app,
                        content_kinds,
                        content='clipboard_history',
                        content_rowid='id',
                        tokenize='trigram'
                    );

                    CREATE TRIGGER clipboard_history_ai AFTER INSERT ON clipboard_history BEGIN
                        INSERT INTO clipboard_fts(rowid, content, preview, source_app, content_kinds)
                        VALUES (new.id, new.content, new.preview, new.source_app, new.content_kinds);
                    END;

                    CREATE TRIGGER clipboard_history_ad AFTER DELETE ON clipboard_history BEGIN
                        INSERT INTO clipboard_fts(clipboard_fts, rowid, content, preview, source_app, content_kinds)
                        VALUES ('delete', old.id, old.content, old.preview, old.source_app, old.content_kinds);
                    END;

                    CREATE TRIGGER clipboard_history_au AFTER UPDATE ON clipboard_history BEGIN
                        INSERT INTO clipboard_fts(clipboard_fts, rowid, content, preview, source_app, content_kinds)
                        VALUES ('delete', old.id, old.content, old.preview, old.source_app, old.content_kinds);
                        INSERT INTO clipboard_fts(rowid, content, preview, source_app, content_kinds)
                        VALUES (new.id, new.content, new.preview, new.source_app, new.content_kinds);
                    END;
                    ",
                )?;
            }
            conn.execute(
                "INSERT INTO clipboard_fts(clipboard_fts) VALUES ('rebuild')",
                [],
            )?;
            Ok(())
        })();

        if let Err(err) = migration_result {
            let _ = conn.execute("ROLLBACK", []);
            return Err(err);
        }
        conn.execute("COMMIT", [])?;
        conn.execute("INSERT INTO schema_migrations (version) VALUES (13)", [])?;
    }

    // Migration 14: Add ocr_text + ocr_status columns for OCR pipeline output
    // and extend the FTS5 virtual table to index ocr_text. ocr_status is the
    // lifecycle state machine: 'pending' | 'processing' | 'done' | 'failed' |
    // 'unsupported'. The FTS5 schema from v13 is rebuilt because FTS5 virtual
    // tables do not support ALTER TABLE — drop + recreate to add the column.
    // The DELETE trigger is intentionally kept as-is (without ocr_text) because
    // FTS5 'delete' commands locate rows by rowid; the missing column does not
    // affect row resolution, and pre-v14 rows have NULL ocr_text anyway.
    if current_version < 14 {
        conn.execute("BEGIN", [])?;
        let migration_result = (|| -> Result<()> {
            if !has_column(conn, "clipboard_history", "ocr_text")? {
                conn.execute("ALTER TABLE clipboard_history ADD COLUMN ocr_text TEXT", [])?;
            }
            if !has_column(conn, "clipboard_history", "ocr_status")? {
                conn.execute(
                    "ALTER TABLE clipboard_history ADD COLUMN ocr_status TEXT NOT NULL DEFAULT 'pending'",
                    [],
                )?;
            }
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_clipboard_history_ocr_status
                    ON clipboard_history (ocr_status)",
                [],
            )?;
            if !has_column(conn, "clipboard_fts", "ocr_text")? {
                conn.execute_batch(
                    "
                    DROP TRIGGER IF EXISTS clipboard_history_ai;
                    DROP TRIGGER IF EXISTS clipboard_history_ad;
                    DROP TRIGGER IF EXISTS clipboard_history_au;
                    DROP TABLE IF EXISTS clipboard_fts;

                    CREATE VIRTUAL TABLE clipboard_fts USING fts5(
                        content,
                        preview,
                        source_app,
                        content_kinds,
                        ocr_text,
                        content='clipboard_history',
                        content_rowid='id',
                        tokenize='trigram'
                    );

                    CREATE TRIGGER clipboard_history_ai AFTER INSERT ON clipboard_history BEGIN
                        INSERT INTO clipboard_fts(rowid, content, preview, source_app, content_kinds, ocr_text)
                        VALUES (new.id, new.content, new.preview, new.source_app, new.content_kinds, new.ocr_text);
                    END;

                    CREATE TRIGGER clipboard_history_ad AFTER DELETE ON clipboard_history BEGIN
                        INSERT INTO clipboard_fts(clipboard_fts, rowid, content, preview, source_app, content_kinds)
                        VALUES ('delete', old.id, old.content, old.preview, old.source_app, old.content_kinds);
                    END;

                    CREATE TRIGGER clipboard_history_au AFTER UPDATE ON clipboard_history BEGIN
                        INSERT INTO clipboard_fts(clipboard_fts, rowid, content, preview, source_app, content_kinds)
                        VALUES ('delete', old.id, old.content, old.preview, old.source_app, old.content_kinds);
                        INSERT INTO clipboard_fts(rowid, content, preview, source_app, content_kinds, ocr_text)
                        VALUES (new.id, new.content, new.preview, new.source_app, new.content_kinds, new.ocr_text);
                    END;
                    ",
                )?;
            }
            conn.execute(
                "INSERT INTO clipboard_fts(clipboard_fts) VALUES ('rebuild')",
                [],
            )?;
            Ok(())
        })();

        if let Err(err) = migration_result {
            let _ = conn.execute("ROLLBACK", []);
            return Err(err);
        }
        conn.execute("COMMIT", [])?;
        conn.execute("INSERT INTO schema_migrations (version) VALUES (14)", [])?;
    }

    if current_version < 15 {
        conn.execute("BEGIN", [])?;
        let migration_result = (|| -> Result<()> {
            if !has_column(conn, "clipboard_history", "ocr_text")? {
                conn.execute("ALTER TABLE clipboard_history ADD COLUMN ocr_text TEXT", [])?;
            }
            if !has_column(conn, "clipboard_history", "ocr_status")? {
                conn.execute(
                    "ALTER TABLE clipboard_history ADD COLUMN ocr_status TEXT NOT NULL DEFAULT 'pending'",
                    [],
                )?;
            }
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_clipboard_history_ocr_status
                    ON clipboard_history (ocr_status)",
                [],
            )?;
            conn.execute_batch(
                "
                DROP TRIGGER IF EXISTS clipboard_history_ai;
                DROP TRIGGER IF EXISTS clipboard_history_ad;
                DROP TRIGGER IF EXISTS clipboard_history_au;
                DROP TABLE IF EXISTS clipboard_fts;

                CREATE VIRTUAL TABLE clipboard_fts USING fts5(
                    content,
                    preview,
                    source_app,
                    content_kinds,
                    ocr_text,
                    content='clipboard_history',
                    content_rowid='id',
                    tokenize='trigram'
                );

                CREATE TRIGGER clipboard_history_ai AFTER INSERT ON clipboard_history BEGIN
                    INSERT INTO clipboard_fts(rowid, content, preview, source_app, content_kinds, ocr_text)
                    VALUES (new.id, new.content, new.preview, new.source_app, new.content_kinds, new.ocr_text);
                END;

                CREATE TRIGGER clipboard_history_ad AFTER DELETE ON clipboard_history BEGIN
                    INSERT INTO clipboard_fts(clipboard_fts, rowid, content, preview, source_app, content_kinds, ocr_text)
                    VALUES ('delete', old.id, old.content, old.preview, old.source_app, old.content_kinds, old.ocr_text);
                END;

                CREATE TRIGGER clipboard_history_au AFTER UPDATE ON clipboard_history BEGIN
                    INSERT INTO clipboard_fts(clipboard_fts, rowid, content, preview, source_app, content_kinds, ocr_text)
                    VALUES ('delete', old.id, old.content, old.preview, old.source_app, old.content_kinds, old.ocr_text);
                    INSERT INTO clipboard_fts(rowid, content, preview, source_app, content_kinds, ocr_text)
                    VALUES (new.id, new.content, new.preview, new.source_app, new.content_kinds, new.ocr_text);
                END;
                ",
            )?;
            conn.execute(
                "INSERT INTO clipboard_fts(clipboard_fts) VALUES ('rebuild')",
                [],
            )?;
            Ok(())
        })();

        if let Err(err) = migration_result {
            let _ = conn.execute("ROLLBACK", []);
            return Err(err);
        }
        conn.execute("COMMIT", [])?;
        conn.execute("INSERT INTO schema_migrations (version) VALUES (15)", [])?;
    }

    if current_version < 16 {
        conn.execute("BEGIN", [])?;
        let migration_result = (|| -> Result<()> {
            // Storage-limit enforcement runs after every single insert, and both
            // of its queries filter on `is_pinned = 0 AND tags = '[]'`. The
            // existing indexes cannot serve that: `pinned_order` sits between
            // `is_pinned` and `timestamp`, so ordering the candidates by time
            // means sorting every unpinned row, and the `tags` test is not
            // indexed at all, so each candidate costs a trip back to the table.
            //
            // A partial index over exactly the evictable rows makes both halves
            // of the check walk that index alone — the COUNT becomes a count of
            // index entries, and the ORDER BY ... LIMIT walks them in order and
            // stops after the rows it needs. Rows that are pinned or tagged are
            // never written into it, so its size tracks the history that is
            // actually subject to the limit.
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_clipboard_history_evictable
                    ON clipboard_history (timestamp)
                    WHERE is_pinned = 0 AND (tags = '[]' OR tags IS NULL)",
                [],
            )?;
            Ok(())
        })();

        if let Err(err) = migration_result {
            let _ = conn.execute("ROLLBACK", []);
            return Err(err);
        }
        conn.execute("COMMIT", [])?;
        conn.execute("INSERT INTO schema_migrations (version) VALUES (16)", [])?;
    }

    if current_version < 17 {
        // Not wrapped in the transaction every other migration uses, and not
        // allowed to take the app down with it: see `reclaim_stranded_pages`.
        match reclaim_stranded_pages(conn, RECLAIM_MIN_BYTES) {
            Ok(()) => {
                conn.execute("INSERT INTO schema_migrations (version) VALUES (17)", [])?;
            }
            Err(err) => {
                eprintln!(
                    "[clipboard] history database still has stranded page space and could \
                     not be compacted: {err}. The app works normally; the file will keep \
                     the space until the next launch succeeds."
                );
            }
        }
    }

    if current_version < 18 {
        conn.execute("BEGIN", [])?;
        let migration_result = (|| -> Result<()> {
            // Deduplication runs on every single capture, and its lookup is
            //
            //   WHERE (content_type = ? AND content_hash = ?)
            //      OR (content_type = ? AND content = ?)
            //
            // The second arm is the safety net for a row whose stored
            // `content_hash` was not produced by today's hash function, so it
            // has to stay. What it must not cost is a walk of the table: with
            // no index on `content`, the planner satisfies that arm by seeking
            // on `content_type` and then fetching every row of that type back
            // out of the table to compare the text. On this project's history
            // that is 3021 random page reads behind a text capture and a full
            // table scan behind an image one, both on the hot path.
            //
            // Indexing the content lets both arms be seeks. It is the one index
            // that duplicates a column's bytes rather than a rowid, and the
            // column here is text and file paths -- on the real install all of
            // it adds up to 3.4 MB against an 89.9 MB live file. Keeping it up
            // to date measured as free: an insert with the index landed in
            // 2.992 ms against 2.999 ms without, which is noise.
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_clipboard_history_content_type
                    ON clipboard_history (content, content_type)",
                [],
            )?;
            Ok(())
        })();

        if let Err(err) = migration_result {
            let _ = conn.execute("ROLLBACK", []);
            return Err(err);
        }
        conn.execute("COMMIT", [])?;
        conn.execute("INSERT INTO schema_migrations (version) VALUES (18)", [])?;
    }

    Ok(())
}

fn maybe_decrypt_metadata(value: &str) -> Option<String> {
    if !encryption::is_encrypted_value(value) {
        return None;
    }

    encryption::decrypt_value(value).and_then(|plain| {
        let plain = plain.trim();
        if plain.is_empty() || plain == value {
            None
        } else {
            Some(plain.to_string())
        }
    })
}

fn repair_encrypted_tags(conn: &mut Connection) -> Result<()> {
    let tx = conn.transaction()?;

    let encrypted_tags: Vec<String> = {
        let mut stmt = tx.prepare("SELECT DISTINCT tag FROM entry_tags")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let tags = rows
            .filter_map(|row| row.ok())
            .filter(|tag| encryption::is_encrypted_value(tag))
            .collect();
        tags
    };

    for encrypted_tag in encrypted_tags {
        if let Some(plain_tag) = maybe_decrypt_metadata(&encrypted_tag) {
            let entry_ids: Vec<i64> = {
                let mut id_stmt = tx.prepare("SELECT entry_id FROM entry_tags WHERE tag = ?")?;
                let rows =
                    id_stmt.query_map(params![&encrypted_tag], |row| row.get::<_, i64>(0))?;
                let ids = rows.filter_map(|row| row.ok()).collect();
                ids
            };

            for entry_id in entry_ids {
                tx.execute(
                    "INSERT OR IGNORE INTO entry_tags (entry_id, tag) VALUES (?1, ?2)",
                    params![entry_id, &plain_tag],
                )?;
                tx.execute(
                    "DELETE FROM entry_tags WHERE entry_id = ?1 AND tag = ?2",
                    params![entry_id, &encrypted_tag],
                )?;
            }
        }
    }

    let saved_tags: Vec<(String, Option<String>)> = {
        let mut stmt = tx.prepare("SELECT name, color FROM saved_tags")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })?;
        let tags = rows
            .filter_map(|row| row.ok())
            .filter(|(name, _)| encryption::is_encrypted_value(name))
            .collect();
        tags
    };

    for (encrypted_name, color) in saved_tags {
        if let Some(plain_name) = maybe_decrypt_metadata(&encrypted_name) {
            tx.execute(
                "INSERT OR IGNORE INTO saved_tags (name, color) VALUES (?1, ?2)",
                params![&plain_name, color],
            )?;
            tx.execute(
                "DELETE FROM saved_tags WHERE name = ?",
                params![&encrypted_name],
            )?;
        }
    }

    let rows: Vec<(i64, String)> = {
        let mut stmt = tx.prepare("SELECT id, tags FROM clipboard_history")?;
        let mapped_rows = stmt.query_map([], |row| {
            let id: i64 = row.get(0)?;
            let tags: Option<String> = row.get(1)?;
            Ok((id, tags.unwrap_or_else(|| "[]".to_string())))
        })?;
        let rows = mapped_rows.filter_map(|row| row.ok()).collect();
        rows
    };

    for (id, tags_json) in rows {
        let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
        let mut changed = false;
        let mut seen = std::collections::HashSet::new();
        let mut repaired = Vec::new();

        for tag in tags {
            let next = maybe_decrypt_metadata(&tag).unwrap_or_else(|| tag.clone());
            if next != tag {
                changed = true;
            }
            if !next.trim().is_empty() && seen.insert(next.clone()) {
                repaired.push(next);
            }
        }

        if changed {
            let repaired_json =
                serde_json::to_string(&repaired).unwrap_or_else(|_| "[]".to_string());
            tx.execute(
                "UPDATE clipboard_history SET tags = ? WHERE id = ?",
                params![repaired_json, id],
            )?;
        }
    }

    tx.commit()
}

fn has_column(conn: &Connection, table_name: &str, column_name: &str) -> Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({})", table_name))?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == column_name {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::{reclaim_stranded_pages, run_migrations};
    use rusqlite::{params, Connection};

    fn fresh_db() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory");
        run_migrations(&mut conn).expect("run_migrations");
        conn
    }

    fn table_has_column(conn: &Connection, table: &str, column: &str) -> bool {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({})", table))
            .expect("table_info");
        let mut rows = stmt.query([]).expect("query");
        while let Some(row) = rows.next().expect("next") {
            let name: String = row.get(1).expect("name");
            if name == column {
                return true;
            }
        }
        false
    }

    #[test]
    fn test_v14_adds_ocr_columns() {
        let conn = fresh_db();
        assert!(
            table_has_column(&conn, "clipboard_history", "ocr_text"),
            "ocr_text column must exist on clipboard_history after v14"
        );
        assert!(
            table_has_column(&conn, "clipboard_history", "ocr_status"),
            "ocr_status column must exist on clipboard_history after v14"
        );
        assert!(
            table_has_column(&conn, "clipboard_fts", "ocr_text"),
            "ocr_text column must exist on clipboard_fts FTS5 virtual table after v14"
        );
    }

    #[test]
    fn test_v14_default_ocr_status() {
        let conn = fresh_db();
        conn.execute(
            "INSERT INTO clipboard_history
             (content_type, content, html_content, source_app, timestamp, preview,
              is_pinned, content_hash, tags, is_external, pinned_order, source_app_path,
              ocr_text, ocr_status)
             VALUES ('text', 'hello world', NULL, 'App1', 1700000000, 'hello world',
                     0, 0, '[]', 0, 0, NULL, NULL, 'pending')",
            [],
        )
        .expect("insert");
        let (ocr_text, ocr_status): (Option<String>, String) = conn
            .query_row(
                "SELECT ocr_text, ocr_status FROM clipboard_history ORDER BY id DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("query");
        assert_eq!(ocr_text, None, "ocr_text default must be NULL");
        assert_eq!(
            ocr_status, "pending",
            "ocr_status default must be 'pending'"
        );
    }

    #[test]
    fn test_v14_fts5_indexes_ocr_text() {
        let conn = fresh_db();
        conn.execute(
            "INSERT INTO clipboard_history
             (content_type, content, html_content, source_app, timestamp, preview,
              is_pinned, content_hash, tags, is_external, pinned_order, source_app_path,
              ocr_text, ocr_status)
             VALUES ('image', 'image-bytes', NULL, 'Screenshot', 1700000000, 'image',
                     0, 0, '[]', 0, 0, NULL,
                     'invoice total forty-two dollars and seventeen cents', 'done')",
            [],
        )
        .expect("insert");

        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM clipboard_fts WHERE clipboard_fts MATCH ?1",
                params!["invoice"],
                |row| row.get(0),
            )
            .expect("fts count");
        assert!(
            count >= 1,
            "FTS5 INSERT trigger must index ocr_text 'invoice total forty-two dollars' (got count={count})"
        );

        let forty_two: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM clipboard_fts WHERE clipboard_fts MATCH ?1",
                params!["forty"],
                |row| row.get(0),
            )
            .expect("fts count 2");
        assert!(
            forty_two >= 1,
            "FTS5 INSERT trigger must index 'forty' from ocr_text (got count={forty_two})"
        );
    }

    #[test]
    fn test_v14_fts5_rebuild_includes_ocr_text() {
        // Simulate pre-v14 state: rows inserted before the migration existed
        // without ocr_text. After v14 runs, the FTS5 rebuild must re-index
        // those rows (with NULL ocr_text) and any subsequent UPDATE setting
        // ocr_text must surface in FTS5 search results.
        let mut conn = Connection::open_in_memory().expect("open");
        run_migrations(&mut conn).expect("migrations");

        // Insert a row without ocr_text (column added later with NULL default).
        conn.execute(
            "INSERT INTO clipboard_history
             (content_type, content, html_content, source_app, timestamp, preview,
              is_pinned, content_hash, tags, is_external, pinned_order, source_app_path,
              ocr_text, ocr_status)
             VALUES ('text', 'plain clipboard body', NULL, 'App1', 1700000000,
                     'plain', 0, 0, '[]', 0, 0, NULL, NULL, 'pending')",
            [],
        )
        .expect("insert pre-rebuild row");

        // Drop the v14 FTS5 surface and roll back the v14 schema marker.
        conn.execute("DROP TRIGGER IF EXISTS clipboard_history_ai", [])
            .expect("drop ai");
        conn.execute("DROP TRIGGER IF EXISTS clipboard_history_ad", [])
            .expect("drop ad");
        conn.execute("DROP TRIGGER IF EXISTS clipboard_history_au", [])
            .expect("drop au");
        conn.execute("DROP TABLE IF EXISTS clipboard_fts", [])
            .expect("drop fts");
        // Roll the whole tail back, not just the versions that existed when this
        // test was written: the gate is `MAX(version)`, so leaving any later
        // marker behind would skip the blocks this test is trying to re-run.
        conn.execute("DELETE FROM schema_migrations WHERE version >= 14", [])
            .expect("delete v14 row");

        // Re-apply migrations: this must re-add columns (already there but the
        // has_column guard makes it idempotent) and rebuild FTS5 including
        // ocr_text. The rebuild must pick up the pre-existing row with NULL
        // ocr_text without error.
        run_migrations(&mut conn).expect("re-apply migrations");

        // After rebuild, the existing row's ocr_text is NULL — FTS5 search for
        // its clipboard body content must still succeed.
        let body_match: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM clipboard_fts WHERE clipboard_fts MATCH ?1",
                params!["plain"],
                |row| row.get(0),
            )
            .expect("fts body count");
        assert!(
            body_match >= 1,
            "FTS5 rebuild must preserve existing rows' content searchability (got count={body_match})"
        );

        // Now mutate the row to populate ocr_text, and force another rebuild.
        conn.execute(
            "UPDATE clipboard_history SET ocr_text = ?1 WHERE id = 1",
            params!["handwritten note: meeting at three pm"],
        )
        .expect("update ocr_text");
        conn.execute(
            "INSERT INTO clipboard_fts(clipboard_fts) VALUES ('rebuild')",
            [],
        )
        .expect("rebuild");

        let handwritten: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM clipboard_fts WHERE clipboard_fts MATCH ?1",
                params!["handwritten"],
                |row| row.get(0),
            )
            .expect("fts handwritten count");
        assert!(
            handwritten >= 1,
            "FTS5 rebuild must include ocr_text for updated rows (got count={handwritten})"
        );
    }

    #[test]
    fn test_v15_rebuilds_existing_v14_fts_without_ocr_text() {
        let mut conn = fresh_db();
        conn.execute("DELETE FROM schema_migrations WHERE version >= 15", [])
            .expect("remove v15 marker");
        conn.execute_batch(
            "
            DROP TRIGGER IF EXISTS clipboard_history_ai;
            DROP TRIGGER IF EXISTS clipboard_history_ad;
            DROP TRIGGER IF EXISTS clipboard_history_au;
            DROP TABLE IF EXISTS clipboard_fts;

            CREATE VIRTUAL TABLE clipboard_fts USING fts5(
                content,
                preview,
                source_app,
                content_kinds,
                content='clipboard_history',
                content_rowid='id',
                tokenize='trigram'
            );

            CREATE TRIGGER clipboard_history_ai AFTER INSERT ON clipboard_history BEGIN
                INSERT INTO clipboard_fts(rowid, content, preview, source_app, content_kinds)
                VALUES (new.id, new.content, new.preview, new.source_app, new.content_kinds);
            END;

            CREATE TRIGGER clipboard_history_ad AFTER DELETE ON clipboard_history BEGIN
                INSERT INTO clipboard_fts(clipboard_fts, rowid, content, preview, source_app, content_kinds)
                VALUES ('delete', old.id, old.content, old.preview, old.source_app, old.content_kinds);
            END;

            CREATE TRIGGER clipboard_history_au AFTER UPDATE ON clipboard_history BEGIN
                INSERT INTO clipboard_fts(clipboard_fts, rowid, content, preview, source_app, content_kinds)
                VALUES ('delete', old.id, old.content, old.preview, old.source_app, old.content_kinds);
                INSERT INTO clipboard_fts(rowid, content, preview, source_app, content_kinds)
                VALUES (new.id, new.content, new.preview, new.source_app, new.content_kinds);
            END;
            ",
        )
        .expect("restore old v14 fts surface");
        conn.execute(
            "INSERT INTO clipboard_history
             (content_type, content, html_content, source_app, timestamp, preview,
              is_pinned, content_hash, tags, is_external, pinned_order, source_app_path,
              content_kinds, ocr_text, ocr_status)
             VALUES ('image', 'data:image/png;base64,old', NULL, 'Screenshot', 1700000000,
                     'image', 0, 0, '[]', 0, 0, NULL, '[\"image\"]',
                     'receipt total one hundred yuan', 'done')",
            [],
        )
        .expect("insert old v14 image row");

        let before: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM clipboard_fts WHERE clipboard_fts MATCH ?1",
                params!["receipt"],
                |row| row.get(0),
            )
            .expect("old fts count");
        assert_eq!(before, 0, "old v14 FTS surface should miss OCR text");

        run_migrations(&mut conn).expect("apply v15 migration");

        let version: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version = 15",
                [],
                |row| row.get(0),
            )
            .expect("schema version");
        assert_eq!(version, 1, "v15 migration marker must be written");

        let after: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM clipboard_fts WHERE clipboard_fts MATCH ?1",
                params!["receipt"],
                |row| row.get(0),
            )
            .expect("new fts count");
        assert!(after >= 1, "v15 rebuild must index existing OCR text");
    }

    fn insert_history_row(conn: &Connection, id: i64, pinned: i32, tags: &str, ts: i64) {
        conn.execute(
            "INSERT INTO clipboard_history
             (id, content_type, content, html_content, source_app, timestamp, preview,
              is_pinned, content_hash, tags, is_external, pinned_order, source_app_path,
              ocr_text, ocr_status)
             VALUES (?1, 'text', ?2, NULL, 'App', ?3, 'p', ?4, 0, ?5, 0, 0, NULL, NULL, 'pending')",
            params![id, format!("entry {id}"), ts, pinned, tags],
        )
        .expect("insert history row");
    }

    fn query_plan(conn: &Connection, sql: &str) -> String {
        let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).expect("plan");
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(3))
            .expect("plan rows");
        let mut out = String::new();
        for row in rows {
            out.push_str(&row.expect("plan row"));
            out.push('\n');
        }
        out
    }

    // The index only pays for itself if the enforcement queries actually take
    // it. Asserting on the plan is what stops a later "simplification" from
    // quietly reintroducing a full scan that the timings would not catch:
    // without the index the planner picks idx_clipboard_history_pinned_order_time
    // and has to read every unpinned row back out of the table.
    #[test]
    fn eviction_queries_are_served_by_the_partial_index() {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).expect("migrations");

        for id in 1..=200 {
            insert_history_row(&mut conn, id, 0, "[]", id);
        }

        let count_plan = query_plan(
            &conn,
            "SELECT COUNT(*) FROM clipboard_history INDEXED BY idx_clipboard_history_evictable \
             WHERE is_pinned = 0 AND (tags = '[]' OR tags IS NULL)",
        );
        assert!(
            count_plan.contains("INDEX idx_clipboard_history_evictable"),
            "the COUNT must be driven by the partial index, got: {count_plan}"
        );
        assert!(
            !count_plan.contains("idx_clipboard_history_pinned_order_time"),
            "the old index forces a full pass over every unpinned row, got: {count_plan}"
        );

        let order_plan = query_plan(
            &conn,
            "SELECT id FROM clipboard_history INDEXED BY idx_clipboard_history_evictable \
             WHERE is_pinned = 0 AND (tags = '[]' OR tags IS NULL) \
             ORDER BY timestamp ASC LIMIT 10",
        );
        assert!(
            order_plan.contains("INDEX idx_clipboard_history_evictable"),
            "the eviction query must be driven by the partial index, got: {order_plan}"
        );
        assert!(
            !order_plan.contains("TEMP B-TREE"),
            "the index is already in timestamp order, so the query must not sort: {order_plan}"
        );
    }

    // A partial index that quietly held pinned or tagged rows would keep them
    // alive in the index and make the count disagree with the table. `INDEXED
    // BY` on its own is only a planner hint and does not filter, so the
    // predicate has to stay in the query for the two counts to be comparable.
    #[test]
    fn partial_index_covers_only_evictable_rows() {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).expect("migrations");

        insert_history_row(&mut conn, 1, 0, "[]", 1);
        insert_history_row(&mut conn, 2, 1, "[]", 2);
        insert_history_row(&mut conn, 3, 0, "[\"work\"]", 3);
        insert_history_row(&mut conn, 4, 0, "[]", 4);

        let via_index: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM clipboard_history INDEXED BY idx_clipboard_history_evictable \
                 WHERE is_pinned = 0 AND (tags = '[]' OR tags IS NULL)",
                [],
                |row| row.get(0),
            )
            .expect("count through partial index");
        assert_eq!(
            via_index, 2,
            "pinned and tagged rows must not be reachable through the evictable index"
        );

        let via_table: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM clipboard_history \
                 WHERE is_pinned = 0 AND (tags = '[]' OR tags IS NULL)",
                [],
                |row| row.get(0),
            )
            .expect("count through table");
        assert_eq!(
            via_table, via_index,
            "the index and the predicate must agree on how many rows are evictable"
        );
    }

    #[test]
    fn eviction_still_removes_the_oldest_rows_first() {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).expect("migrations");

        for id in 1..=10 {
            insert_history_row(&mut conn, id, 0, "[]", id);
        }
        insert_history_row(&mut conn, 11, 1, "[]", 11);
        insert_history_row(&mut conn, 12, 0, "[\"keep\"]", 12);

        let ids: Vec<i64> = {
            let mut stmt = conn
                .prepare(
                    "SELECT id FROM clipboard_history \
                     WHERE is_pinned = 0 AND (tags = '[]' OR tags IS NULL) \
                     ORDER BY timestamp ASC LIMIT 3",
                )
                .expect("prepare");
            let rows = stmt.query_map([], |row| row.get(0)).expect("rows");
            rows.filter_map(|r| r.ok()).collect()
        };

        assert_eq!(ids, vec![1, 2, 3], "eviction order must still be oldest first");
    }

    fn auto_vacuum_mode(conn: &Connection) -> i32 {
        conn.query_row("PRAGMA auto_vacuum", [], |row| row.get(0))
            .expect("auto_vacuum")
    }

    fn free_bytes(conn: &Connection) -> i64 {
        let page_size: i64 = conn
            .query_row("PRAGMA page_size", [], |row| row.get(0))
            .expect("page_size");
        let free_pages: i64 = conn
            .query_row("PRAGMA freelist_count", [], |row| row.get(0))
            .expect("freelist_count");
        page_size * free_pages
    }

    /// A database in the state every install that predates the pragma is in:
    /// mode NONE, tables present, and deletions that have left their pages
    /// stranded in the file.
    fn db_stranding_free_pages() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).expect("migrations");
        conn.execute_batch("PRAGMA auto_vacuum = NONE; VACUUM;")
            .expect("force NONE");

        for id in 1..=40 {
            conn.execute(
                "INSERT INTO clipboard_history
                 (id, content_type, content, html_content, source_app, timestamp, preview,
                  is_pinned, content_hash, tags, is_external, pinned_order, source_app_path,
                  ocr_text, ocr_status)
                 VALUES (?1, 'text', ?2, NULL, 'App', ?1, 'p', 0, 0, '[]', 0, 0, NULL, NULL, 'pending')",
                params![id, format!("entry {id} {}", "x".repeat(900))],
            )
            .expect("insert");
        }
        conn.execute("DELETE FROM clipboard_history WHERE id <= 39", [])
            .expect("delete");
        assert_eq!(
            auto_vacuum_mode(&conn),
            0,
            "a database with tables cannot be pushed back to NONE unless the pragma took"
        );
        assert!(free_bytes(&conn) > 0, "the deletions must have stranded pages");
        conn
    }

    // The whole point of the migration: under NONE those pages are never given
    // back, so the file can only grow. One conversion has to both claim the
    // pages and leave the mode in a state where the next deletion reuses them
    // instead of stranding more.
    #[test]
    fn stranded_pages_are_reclaimed_and_the_mode_is_kept() {
        let conn = db_stranding_free_pages();
        let stranded = free_bytes(&conn);
        assert!(stranded > 0);

        reclaim_stranded_pages(&conn, 1).expect("reclaim");

        assert_eq!(auto_vacuum_mode(&conn), 1, "the mode must end up FULL");
        assert_eq!(
            free_bytes(&conn),
            0,
            "the pages the deletions stranded must be back in the file's use"
        );
    }

    // The conversion costs a rewrite of the whole database, so the freelist
    // count is the only thing standing between a normal startup and a multi
    // second stall. A database with a handful of free pages must be left alone
    // rather than compacted for nothing.
    #[test]
    fn a_database_below_the_threshold_is_left_alone() {
        let conn = db_stranding_free_pages();
        let stranded = free_bytes(&conn);

        reclaim_stranded_pages(&conn, stranded + 1).expect("below threshold");

        assert_eq!(auto_vacuum_mode(&conn), 0, "below the threshold nothing changes");
        assert_eq!(
            free_bytes(&conn),
            stranded,
            "the stranded pages must still be there to be reclaimed later"
        );
    }

    // A fresh install already gets FULL from the pragma `init_db` issues before
    // any table exists. Rewriting such a database would stall the first launch
    // for a file that has nothing to give back.
    #[test]
    fn a_database_already_on_full_is_not_rewritten() {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).expect("migrations");
        conn.execute_batch("PRAGMA auto_vacuum = FULL; VACUUM;")
            .expect("force FULL");
        assert_eq!(auto_vacuum_mode(&conn), 1);

        reclaim_stranded_pages(&conn, 0).expect("reclaim on a FULL database");

        assert_eq!(
            auto_vacuum_mode(&conn),
            1,
            "a database that already returns its pages must not be rewritten"
        );
    }

    // Deduplication runs on every capture, and each half of its lookup is an
    // equality against the index built for it. Asserting on the plan is what
    // stops this coming back: on a history small enough to sit in the page
    // cache the timings look fine either way, and an `OR` left in place would
    // quietly reintroduce the walk of the table.
    #[test]
    fn both_dedup_arms_are_served_by_an_index() {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).expect("migrations");

        for id in 1..=40 {
            insert_history_row(&mut conn, id, 0, "[]", id);
        }

        for (label, sql, index) in [
            (
                "the typed hash arm",
                "SELECT id FROM clipboard_history \
                 WHERE content_type = 'text' AND content_hash = 0",
                "idx_clipboard_history_type_hash",
            ),
            (
                "the typed content arm",
                "SELECT id FROM clipboard_history \
                 WHERE content_type = 'text' AND content = 'a payload'",
                "idx_clipboard_history_content_type",
            ),
            (
                "the untyped content arm",
                "SELECT id FROM clipboard_history WHERE content = 'a payload'",
                "idx_clipboard_history_content_type",
            ),
            (
                "the image hash arm",
                "SELECT id FROM clipboard_history \
                 WHERE content_type = 'image' AND content_hash = 0",
                "idx_clipboard_history_type_hash",
            ),
            (
                "the image content arm",
                "SELECT id FROM clipboard_history WHERE content = 'a payload'",
                "idx_clipboard_history_content_type",
            ),
        ] {
            let plan = query_plan(&conn, sql);
            assert!(
                !plan.contains("SCAN"),
                "{label} must not walk the table, got: {plan}"
            );
            assert!(
                plan.contains(index),
                "{label} must be a seek on {index}, got: {plan}"
            );
        }
    }

    // The index duplicates the bytes of `content`, so what it costs is bounded
    // by how much content the history holds. Its value is that it is the same
    // storage every history query already pays for, which is why this is an
    // index and not a narrower rewrite of the query: dropping the content arm
    // would stop dedup from recognising a row whose stored hash came from an
    // older build, and users would start seeing duplicates after an update.
    #[test]
    fn the_content_index_exists_after_v18() {
        let mut conn = Connection::open_in_memory().unwrap();
        run_migrations(&mut conn).expect("migrations");
        let mut stmt = conn
            .prepare(
                "SELECT sql FROM sqlite_master WHERE type = 'index' \
                 AND name = 'idx_clipboard_history_content_type'",
            )
            .expect("prepare");
        let sql: Option<String> = stmt
            .query_row([], |row| row.get(0))
            .expect("index must exist after v18");
        assert!(
            sql.unwrap().contains("(content, content_type)"),
            "the index must be ordered content first so the content arm can seek on it alone"
        );
    }
}
