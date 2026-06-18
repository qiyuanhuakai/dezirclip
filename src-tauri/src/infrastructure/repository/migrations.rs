use crate::infrastructure::encryption;
use rusqlite::{params, Connection, Result};

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
                let rows = id_stmt.query_map(params![&encrypted_tag], |row| row.get::<_, i64>(0))?;
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
            let repaired_json = serde_json::to_string(&repaired).unwrap_or_else(|_| "[]".to_string());
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
