//! Snapshot and relocate a live profile while its shared connection holds the write reservation.
use crate::infrastructure::encryption::{rewrite_stored_text, EncryptionError};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

#[derive(Debug)]
pub enum MigrationError {
    Database(String),
    Io(String),
    Validation(String),
    Internal(String),
}

impl std::fmt::Display for MigrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database(s) | Self::Io(s) | Self::Validation(s) | Self::Internal(s) => f.write_str(s),
        }
    }
}

impl std::error::Error for MigrationError {}
impl From<rusqlite::Error> for MigrationError {
    fn from(error: rusqlite::Error) -> Self { Self::Database(error.to_string()) }
}
impl From<std::io::Error> for MigrationError {
    fn from(error: std::io::Error) -> Self { Self::Io(error.to_string()) }
}
type MigrationResult<T> = Result<T, MigrationError>;

impl super::clipboard_repo::SqliteClipboardRepository {
    pub(crate) fn migrate_profile(&self, source: &Path, destination: &Path, redirect: &Path, epoch: Option<u64>) -> MigrationResult<()> {
        let mut conn = self.conn.lock().map_err(|_| MigrationError::Internal("Database lock poisoned".into()))?;
        let database = conn.path().map(Path::new).ok_or_else(|| MigrationError::Validation("Profile has no database file".into()))?;
        if database.file_name() != Some(std::ffi::OsStr::new("clipboard.db")) {
            return Err(MigrationError::Validation("Data migration requires a clipboard.db profile".into()));
        }
        if database.canonicalize()? != source.join("clipboard.db").canonicalize()? {
            return Err(MigrationError::Validation("Profile database does not match the live connection".into()));
        }
        let destination = std::path::absolute(destination)?;
        if !destination.is_dir() { return Err(MigrationError::Validation("Directory does not exist".into())); }
        if source.canonicalize()? == destination.canonicalize()? { return Ok(()); }
        migrate_guarded(&mut conn, source, &destination, redirect, epoch)
    }
}

#[cfg(test)]
fn migrate_with_connection(conn: &mut Connection, source: &Path, destination: &Path, redirect: &Path) -> MigrationResult<()> {
    migrate_guarded(conn, source, destination, redirect, None)
}

fn migrate_guarded(
    conn: &mut Connection,
    old_base: &std::path::Path,
    new_base: &std::path::Path,
    redirect_file: &std::path::Path,
    epoch: Option<u64>,
) -> MigrationResult<()> {
    if conn.pragma_query_value(None, "query_only", |row| row.get::<_, bool>(0)).map_err(MigrationError::from)? {
        return Err(MigrationError::Validation("Data migration is complete; restart the application".into()));
    }
    let transaction = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(MigrationError::from)?;
    if let Some(epoch) = epoch {
        let state = crate::api::privacy_session::snapshot_with_conn(&transaction, chrono::Utc::now().timestamp_millis())
            .map_err(|e| MigrationError::Validation(e.to_string()))?;
        if state.privacy_epoch != epoch { return Err(MigrationError::Validation("PrivacyEpochChanged".into())); }
        if state.session.active { return Err(MigrationError::Validation("PrivacySessionActive".into())); }
    }
    transaction.pragma_update(None, "query_only", true).map_err(MigrationError::from)?;
    let result = migrate_data_directory(old_base, new_base, redirect_file);
    transaction.rollback().map_err(MigrationError::from)?;
    if result.is_err() {
        conn.pragma_update(None, "query_only", false).map_err(MigrationError::from)?;
    }
    result
}

fn migrate_data_directory(
    old_base: &std::path::Path,
    new_base: &std::path::Path,
    redirect_file: &std::path::Path,
) -> MigrationResult<()> {
    let old_canonical = old_base.canonicalize().map_err(MigrationError::from)?;
    let new_canonical = new_base.canonicalize().map_err(MigrationError::from)?;
    if new_canonical.starts_with(&old_canonical) || old_canonical.starts_with(&new_canonical) {
        return Err(MigrationError::Validation("Data directories must not contain one another".into()));
    }
    let names = ["clipboard.db", "attachments", "emoji_favorites"];
    for name in names.into_iter().chain(["clipboard.db-wal", "clipboard.db-shm", "clipboard.db-journal"]) {
        match new_base.join(name).symlink_metadata() {
            Ok(_) => return Err(MigrationError::Validation(format!("Destination already contains {name}; choose an empty data directory"))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
    }
    let stage = new_base.join(format!(".dezirclip-migration-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&stage).map_err(MigrationError::from)?;
    let result = (|| {
        // SQLite reads committed WAL pages into a standalone snapshot. Never copy
        // a live DB/WAL/SHM file set with filesystem operations.
        let source = Connection::open_with_flags(old_base.join("clipboard.db"), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(MigrationError::from)?;
        crate::infrastructure::repository::migrations::register_privacy_functions(&source).map_err(MigrationError::from)?;
        let staged_db = stage.join("clipboard.db");
        source.execute("VACUUM main INTO ?1", [staged_db.to_string_lossy().as_ref()]).map_err(MigrationError::from)?;
        drop(source);
        let updates = prepare_attachment_path_rewrites(&staged_db, old_base, new_base)?;
        apply_attachment_path_rewrites(&staged_db, &updates)?;
        rewrite_emoji_favorites_in_db(&staged_db, old_base, new_base)?;
        // Close every staging connection and consolidate its own WAL before
        // publishing the DB. The source remains untouched on every error path.
        let snapshot = Connection::open(&staged_db).map_err(MigrationError::from)?;
        let mode: String = snapshot.query_row("PRAGMA journal_mode=DELETE", [], |row| row.get(0)).map_err(MigrationError::from)?;
        if !mode.eq_ignore_ascii_case("delete") {
            return Err(MigrationError::Internal("Could not finalize migration snapshot".into()));
        }
        drop(snapshot);
        for folder in ["attachments", "emoji_favorites"] {
            let source = old_base.join(folder);
            if source.exists() {
                copy_dir_recursive(&source, &stage.join(folder)).map_err(MigrationError::from)?;
            }
        }
        for name in names {
            let source = stage.join(name);
            if source.is_dir() {
                copy_dir_recursive(&source, &new_base.join(name)).map_err(MigrationError::from)?;
            } else if source.is_file() {
                copy_file_new(&source, &new_base.join(name)).map_err(MigrationError::from)?;
            }
        }
        crate::infrastructure::attachments::write_backup(redirect_file, new_base.to_string_lossy().as_bytes())
            .map_err(MigrationError::Internal)
    })();
    // This is the unique staging folder created above, never a user path.
    let _ = std::fs::remove_dir_all(&stage);
    result
}

fn copy_file_new(source: &std::path::Path, destination: &std::path::Path) -> std::io::Result<()> {
    let mut input = crate::infrastructure::local_file::open_regular_nofollow(source)?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut output = options.open(destination)?;
    std::io::copy(&mut input, &mut output)?;
    output.sync_all()
}

#[derive(Debug)]
struct AttachmentPathRewrite {
    id: i64,
    original_content: String,
    original_html: Option<String>,
    content: String,
    html: Option<String>,
}

fn prepare_attachment_path_rewrites(
    db_path: &std::path::Path,
    old_base: &std::path::Path,
    new_base: &std::path::Path,
) -> MigrationResult<Vec<AttachmentPathRewrite>> {
    let paths = DirectoryRewrite::new(&old_base.join("attachments"), &new_base.join("attachments"))?;
    let conn = Connection::open(db_path).map_err(MigrationError::from)?;

    let mut stmt = conn
        .prepare("SELECT id, content, html_content FROM clipboard_history WHERE is_external = 1 OR html_content IS NOT NULL")
        .map_err(MigrationError::from)?;

    let rows = stmt
        .query_map([], |row| {
            let id: i64 = row.get(0)?;
            let content: String = row.get(1)?;
            let html_content: Option<String> = row.get(2)?;
            Ok((id, content, html_content))
        })
        .map_err(MigrationError::from)?;

    let mut updates = Vec::new();
    for row in rows {
        let (id, content_raw, html_raw) = row.map_err(MigrationError::from)?;
        let mut content_new: Option<String> = None;
        let mut html_new: Option<String> = None;

        if let Some(updated) = rewrite_stored_text(&content_raw, |plain| paths.path(plain))
        .map_err(|error| MigrationError::Internal(error.to_string()))?
        {
            content_new = Some(updated);
        }

        if let Some(html) = html_raw.as_ref() {
            if let Some(updated) = rewrite_stored_text(html, |plain| paths.html(plain))
            .map_err(|error| MigrationError::Internal(error.to_string()))?
            {
                html_new = Some(updated);
            }
        }

        {
            let content_final = content_new.as_ref().unwrap_or(&content_raw);
            let html_final = match html_new.as_ref() {
                Some(v) => Some(v.as_str()),
                None => html_raw.as_deref(),
            };
            let content = content_final.to_owned();
            let html = html_final.map(str::to_owned);
            updates.push(AttachmentPathRewrite {
                id,
                original_content: content_raw,
                original_html: html_raw,
                content,
                html,
            });
        }
    }

    Ok(updates)
}

fn apply_attachment_path_rewrites(
    db_path: &std::path::Path,
    updates: &[AttachmentPathRewrite],
) -> MigrationResult<()> {
    let mut conn = Connection::open(db_path).map_err(MigrationError::from)?;
    crate::infrastructure::repository::migrations::configure_privacy_writer(&conn).map_err(MigrationError::from)?;
    let transaction = conn.transaction().map_err(MigrationError::from)?;
    let candidates: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM clipboard_history WHERE is_external = 1 OR html_content IS NOT NULL", [],
        |row| row.get(0),
    ).map_err(MigrationError::from)?;
    if candidates
        != i64::try_from(updates.len()).map_err(|_| {
            MigrationError::Internal(EncryptionError::SensitivePersistenceRejected.to_string())
        })?
    {
        return Err(MigrationError::Internal(
            EncryptionError::SensitivePersistenceRejected.to_string(),
        ));
    }
    for update in updates {
        let changed = transaction.execute(
            "UPDATE clipboard_history SET content = ?1, html_content = ?2 WHERE id = ?3 AND content IS ?4 AND html_content IS ?5",
            params![update.content, update.html, update.id, update.original_content, update.original_html],
        ).map_err(MigrationError::from)?;
        if changed != 1 {
            return Err(MigrationError::Internal(
                EncryptionError::SensitivePersistenceRejected.to_string(),
            ));
        }
    }
    transaction.commit().map_err(MigrationError::from)
}

fn rewrite_emoji_favorites_in_db(
    db_path: &std::path::Path,
    old_base: &std::path::Path,
    new_base: &std::path::Path,
) -> MigrationResult<()> {
    let paths = DirectoryRewrite::new(&old_base.join("emoji_favorites"), &new_base.join("emoji_favorites"))?;
    let conn = Connection::open(db_path).map_err(MigrationError::from)?;
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'app.emoji_favorites'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(MigrationError::from)?;

    let Some(raw) = value else {
        return Ok(());
    };
    let parsed: Vec<String> = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };

    let mut changed = false;
    let mut updated: Vec<String> = Vec::with_capacity(parsed.len());
    for path in parsed {
        if let Some(next) = paths.path(&path) {
            changed = true;
            updated.push(next);
        } else {
            updated.push(path);
        }
    }

    if changed {
        let serialized = serde_json::to_string(&updated).unwrap_or(raw);
        conn.execute(
            "UPDATE settings SET value = ?1 WHERE key = 'app.emoji_favorites'",
            params![serialized],
        )
        .map_err(MigrationError::from)?;
    }

    Ok(())
}

struct DirectoryRewrite {
    pattern: regex::Regex,
    native: String,
    destination: String,
    destination_slash: String,
}

impl DirectoryRewrite {
    fn new(source: &Path, destination: &Path) -> MigrationResult<Self> {
        let native = source.to_string_lossy().into_owned();
        let destination = destination.to_string_lossy().into_owned();
        let mut prefix = regex::escape(&native);
        if cfg!(windows) {
            prefix = format!("(?:{prefix}|{})[/\\\\]", regex::escape(&native.replace('\\', "/")));
        } else {
            prefix.push('/');
        }
        let pattern = regex::Regex::new(&prefix).map_err(|e| MigrationError::Internal(e.to_string()))?;
        let destination_slash = destination.replace('\\', "/");
        Ok(Self { pattern, native, destination, destination_slash })
    }

    fn replacement(&self, matched: &str) -> String {
        let target = if matched.starts_with(&self.native) { &self.destination } else { &self.destination_slash };
        format!("{target}{}", &matched[matched.len()-1..])
    }

    fn path(&self, text: &str) -> Option<String> {
        let matched = self.pattern.find(text)?;
        if matched.start() != 0 { return None; }
        Some(self.replacement(matched.as_str()) + &text[matched.end()..])
    }

    fn html(&self, text: &str) -> Option<String> {
        match self.pattern.replace_all(text, |parts: &regex::Captures<'_>| self.replacement(&parts[0])) {
            std::borrow::Cow::Borrowed(_) => None,
            std::borrow::Cow::Owned(updated) => Some(updated),
        }
    }
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    if src.symlink_metadata()?.is_symlink() {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Data migration does not follow symbolic links"));
    }
    std::fs::create_dir(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if ty.is_symlink() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Data migration does not follow symbolic links"));
        }
        if ty.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if ty.is_file() {
            copy_file_new(&src_path, &dst_path)?;
        } else {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "Data migration only copies regular files"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod path_rewrite_tests {
    use super::*;
    use base64::Engine;

    fn database() -> (std::path::PathBuf, std::path::PathBuf) {
        let directory =
            std::env::temp_dir().join(format!("dezirclip-path-rewrite-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("clipboard.db");
        let mut conn = Connection::open(&path).unwrap();
        crate::infrastructure::repository::migrations::run_migrations(&mut conn).unwrap();
        (directory, path)
    }

    fn fixture_connection(path: &std::path::Path) -> Connection {
        let connection = Connection::open(path).unwrap();
        crate::infrastructure::repository::migrations::configure_privacy_writer(&connection).unwrap();
        connection
    }

    fn migration_fixture() -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf, Connection) {
        let (root, _) = database();
        let old = root.join("old");
        let new = root.join("new");
        std::fs::create_dir(&old).unwrap();
        std::fs::create_dir(&new).unwrap();
        std::fs::rename(root.join("clipboard.db"), old.join("clipboard.db")).unwrap();
        let conn = fixture_connection(&old.join("clipboard.db"));
        conn.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
        std::fs::create_dir(old.join("attachments")).unwrap();
        std::fs::write(old.join("attachments/image.png"), b"fixture image").unwrap();
        let image = old.join("attachments/image.png").to_string_lossy().to_string();
        conn.execute("INSERT INTO clipboard_history(content_type,source_app,timestamp,preview,id,content,html_content,is_external) VALUES('text','fixture',1,'preview',1,?1,NULL,1)", [&image]).unwrap();
        (root, old, new, conn)
    }

    #[test]
    fn shared_profile_entry_point_checks_the_live_database_before_cutover() {
        let (root, old, new, conn) = migration_fixture();
        drop(conn);
        let history = crate::api::HistoryService::open(&old.join("clipboard.db")).unwrap();
        let redirect = root.join("datapath.txt");
        drop(Connection::open(root.join("clipboard.db")).unwrap());
        let result = history.migrate_profile(&root, &new, &redirect);
        assert!(matches!(result, Err(MigrationError::Validation(_))));
        assert!(!redirect.exists() && !new.join("clipboard.db").exists());
        history.migrate_profile(&old, &old, &redirect).unwrap();
        assert!(!redirect.exists());
        history.migrate_profile(&old, &new, &redirect).unwrap();
        assert_eq!(std::fs::read_to_string(&redirect).unwrap(), new.to_string_lossy());
        assert_eq!(std::fs::read(new.join("attachments/image.png")).unwrap(), b"fixture image");
        drop(history);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn directory_migration_preserves_lookalike_external_paths_and_rejects_an_existing_journal() {
        let (root, old, new, mut conn) = migration_fixture();
        let image = old.join("attachments-other").join("external.png");
        let favorite = old.join("emoji_favorites-other").join("external.png");
        let favorites = serde_json::to_string(&[favorite]).unwrap();
        let html = format!("<img src='{}'>", image.display());
        conn.execute("UPDATE clipboard_history SET content=?1, html_content=?2 WHERE id=1", params![image.to_string_lossy(), html]).unwrap();
        conn.execute("INSERT OR REPLACE INTO settings(key,value) VALUES('app.emoji_favorites',?1)", [&favorites]).unwrap();
        let journal = new.join("clipboard.db-journal");
        std::fs::write(&journal, b"existing journal").unwrap();
        let redirect = root.join("datapath.txt");
        assert!(migrate_with_connection(&mut conn, &old, &new, &redirect).is_err());
        assert!(!redirect.exists() && !new.join("clipboard.db").exists());
        assert_eq!(std::fs::read(&journal).unwrap(), b"existing journal");
        std::fs::remove_file(journal).unwrap();
        migrate_with_connection(&mut conn, &old, &new, &redirect).unwrap();
        let dest = fixture_connection(&new.join("clipboard.db"));
        let stored: (String,String) = dest.query_row("SELECT content,html_content FROM clipboard_history WHERE id=1", [], |r| Ok((r.get(0)?,r.get(1)?))).unwrap();
        assert_eq!(stored, (image.to_string_lossy().into_owned(), html));
        assert_eq!(dest.query_row("SELECT value FROM settings WHERE key='app.emoji_favorites'", [], |r| r.get::<_,String>(0)).unwrap(), favorites);
        drop(dest); drop(conn);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn directory_migration_preserves_schema20_filtered_fts_and_rewrites_html() {
        let (root, old, new, mut conn) = migration_fixture();
        conn.execute("DELETE FROM clipboard_history", []).unwrap();
        let original = old.join("attachments/image.png").to_string_lossy().to_string();
        let html = format!("<p>ordinaryprobe</p><img src='{original}'>");
        conn.execute("INSERT INTO clipboard_history(id,content_type,content,html_content,source_app,timestamp,preview,tags) VALUES(1,'rich_text','ordinaryprobe',?1,'test',1,'ordinaryprobe','[]')", [&html]).unwrap();
        migrate_with_connection(&mut conn, &old, &new, &root.join("datapath.txt")).unwrap();
        let migrated = crate::database::init_db(new.join("clipboard.db").to_str().unwrap()).unwrap();
        let html: String = migrated.query_row("SELECT html_content FROM clipboard_history WHERE id=1", [], |row| row.get(0)).unwrap();
        assert!(html.contains(new.join("attachments/image.png").to_string_lossy().as_ref()));
        assert_eq!(migrated.query_row("SELECT count(*) FROM clipboard_fts WHERE clipboard_fts MATCH 'ordinaryprobe'", [], |row| row.get::<_, i64>(0)).unwrap(), 1);
        migrated.execute("INSERT INTO clipboard_fts(clipboard_fts,rank) VALUES('integrity-check',1)", []).unwrap();
        assert!(old.join("attachments/image.png").is_file());
        drop(migrated); drop(conn);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn directory_migration_sql_rejection_keeps_live_source_files_and_redirect_usable() {
        let (root, old, new, mut conn) = migration_fixture();
        conn.execute_batch("CREATE TRIGGER reject_rewrite BEFORE UPDATE ON clipboard_history BEGIN SELECT RAISE(ABORT,'fixture rejection'); END;").unwrap();
        let redirect = root.join("datapath.txt");
        std::fs::write(&redirect, old.to_string_lossy().as_bytes()).unwrap();
        let error = migrate_with_connection(&mut conn, &old, &new, &redirect).unwrap_err();
        assert!(error.to_string().contains("fixture rejection"));
        assert_eq!(std::fs::read_to_string(&redirect).unwrap(), old.to_string_lossy());
        assert_eq!(std::fs::read(old.join("attachments/image.png")).unwrap(), b"fixture image");
        assert!(old.join("clipboard.db").exists());
        assert!(!new.join("clipboard.db").exists());
        conn.execute("INSERT INTO clipboard_history(content_type,source_app,timestamp,preview,id,content,html_content,is_external) VALUES('text','fixture',1,'preview',2,'still writable',NULL,0)", []).unwrap();
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM clipboard_history", [], |row| row.get::<_, i64>(0)).unwrap(), 2);
        drop(conn);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn directory_migration_snapshots_committed_wal_and_preserves_source_until_restart() {
        let (root, old, new, mut conn) = migration_fixture();
        let second = fixture_connection(&old.join("clipboard.db"));
        second.execute("INSERT INTO clipboard_history(content_type,source_app,timestamp,preview,id,content,html_content,is_external) VALUES('text','fixture',1,'preview',2,'concurrent committed row',NULL,0)", []).unwrap();
        let redirect = root.join("datapath.txt");
        migrate_with_connection(&mut conn, &old, &new, &redirect).unwrap();
        assert_eq!(std::fs::read_to_string(&redirect).unwrap(), new.to_string_lossy());
        let migrated = fixture_connection(&new.join("clipboard.db"));
        let expected_new = new.join("attachments/image.png").to_string_lossy().to_string();
        let expected_old = old.join("attachments/image.png").to_string_lossy().to_string();
        assert_eq!(migrated.query_row("SELECT content FROM clipboard_history WHERE id=1", [], |row| row.get::<_, String>(0)).unwrap(), expected_new);
        assert_eq!(conn.query_row("SELECT content FROM clipboard_history WHERE id=1", [], |row| row.get::<_, String>(0)).unwrap(), expected_old);
        assert_eq!(migrated.query_row("SELECT content FROM clipboard_history WHERE id=2", [], |row| row.get::<_, String>(0)).unwrap(), "concurrent committed row");
        assert_eq!(std::fs::read(old.join("attachments/image.png")).unwrap(), std::fs::read(new.join("attachments/image.png")).unwrap());
        assert!(conn.execute("INSERT INTO clipboard_history(content_type,source_app,timestamp,preview,id,content,html_content,is_external) VALUES('text','fixture',1,'preview',3,'blocked until relaunch',NULL,0)", []).is_err());
        assert!(migrate_with_connection(&mut conn, &old, &new, &redirect).is_err());
        assert!(conn.pragma_query_value(None, "query_only", |row| row.get::<_, bool>(0)).unwrap());
        drop(migrated);
        drop(second);
        drop(conn);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn directory_migration_redirect_failure_retains_source_and_restores_writes() {
        let (root, old, new, mut conn) = migration_fixture();
        let redirect = root.join("datapath.txt");
        std::fs::create_dir(&redirect).unwrap();
        std::fs::write(redirect.join("keep"), b"existing configuration obstacle").unwrap();
        let error = migrate_with_connection(&mut conn, &old, &new, &redirect).unwrap_err();
        assert!(matches!(error, MigrationError::Internal(_)));
        let published = fixture_connection(&new.join("clipboard.db"));
        let rewritten: String = published.query_row("SELECT content FROM clipboard_history WHERE id=1", [], |row| row.get(0)).unwrap();
        assert_eq!(rewritten, new.join("attachments/image.png").to_string_lossy());
        assert!(new.join("attachments/image.png").is_file());
        drop(published);
        assert_eq!(std::fs::read(redirect.join("keep")).unwrap(), b"existing configuration obstacle");
        assert!(old.join("clipboard.db").exists());
        assert!(old.join("attachments/image.png").exists());
        conn.execute("INSERT INTO clipboard_history(content_type,source_app,timestamp,preview,id,content,html_content,is_external) VALUES('text','fixture',1,'preview',2,'still writable',NULL,0)", []).unwrap();
        drop(conn);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn directory_migration_never_overwrites_existing_destination_data() {
        let (root, old, new, mut conn) = migration_fixture();
        std::fs::write(new.join("clipboard.db"), b"existing target history").unwrap();
        let redirect = root.join("datapath.txt");
        std::fs::write(&redirect, old.to_string_lossy().as_bytes()).unwrap();
        assert!(migrate_with_connection(&mut conn, &old, &new, &redirect).is_err());
        assert_eq!(std::fs::read(new.join("clipboard.db")).unwrap(), b"existing target history");
        assert_eq!(std::fs::read_to_string(&redirect).unwrap(), old.to_string_lossy());
        assert!(old.join("clipboard.db").exists());
        drop(conn);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn directory_migration_source_reservation_blocks_other_writers_through_snapshot() {
        let (root, old, new, mut conn) = migration_fixture();
        let writer = fixture_connection(&old.join("clipboard.db"));
        writer.busy_timeout(std::time::Duration::ZERO).unwrap();
        let transaction = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).unwrap();
        let error = writer.execute("INSERT INTO clipboard_history(content_type,source_app,timestamp,preview,id,content,html_content,is_external) VALUES('text','fixture',1,'preview',2,'blocked',NULL,0)", []).unwrap_err();
        assert!(matches!(error, rusqlite::Error::SqliteFailure(code, _) if code.code == rusqlite::ErrorCode::DatabaseBusy));
        migrate_data_directory(&old, &new, &root.join("datapath.txt")).unwrap();
        let error = writer.execute("INSERT INTO clipboard_history(content_type,source_app,timestamp,preview,id,content,html_content,is_external) VALUES('text','fixture',1,'preview',2,'blocked',NULL,0)", []).unwrap_err();
        assert!(matches!(error, rusqlite::Error::SqliteFailure(code, _) if code.code == rusqlite::ErrorCode::DatabaseBusy));
        transaction.rollback().unwrap();
        writer.execute("INSERT INTO clipboard_history(content_type,source_app,timestamp,preview,id,content,html_content,is_external) VALUES('text','fixture',1,'preview',2,'unblocked after rollback',NULL,0)", []).unwrap();
        drop(writer);
        drop(conn);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn directory_migration_fifo_rejection_returns_promptly_and_releases_source_write_lock() {
        let (root, old, new, mut conn) = migration_fixture();
        assert!(std::process::Command::new("mkfifo").arg(old.join("attachments/blocked.png")).status().unwrap().success());
        let redirect = root.join("datapath.txt");
        std::fs::write(&redirect, old.to_string_lossy().as_bytes()).unwrap();
        let writer = fixture_connection(&old.join("clipboard.db"));
        writer.busy_timeout(std::time::Duration::ZERO).unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        let old_thread = old.clone();
        let redirect_thread = redirect.clone();
        let worker = std::thread::spawn(move || {
            assert!(migrate_with_connection(&mut conn, &old_thread, &new, &redirect_thread).is_err());
            assert!(conn.is_autocommit());
            assert!(!conn.pragma_query_value(None, "query_only", |row| row.get::<_, bool>(0)).unwrap());
            sender.send(()).unwrap();
        });
        receiver.recv_timeout(std::time::Duration::from_secs(5)).expect("FIFO input must be rejected without waiting for a writer");
        worker.join().unwrap();
        writer.execute("INSERT INTO clipboard_history(content_type,source_app,timestamp,preview,id,content,html_content,is_external) VALUES('text','fixture',1,'preview',2,'source remains writable',NULL,0)", []).unwrap();
        assert_eq!(std::fs::read_to_string(&redirect).unwrap(), old.to_string_lossy());
        assert_eq!(std::fs::read(old.join("attachments/image.png")).unwrap(), b"fixture image");
        drop(writer);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn path_rewrite_preflight_rejects_unavailable_ciphertext_without_changing_source() {
        let (directory, path) = database();
        let cipher = format!(
            "linux:{}",
            base64::engine::general_purpose::STANDARD.encode([0u8; 28])
        );
        let conn = fixture_connection(&path);
        conn.execute(
            "INSERT INTO clipboard_history(content_type,source_app,timestamp,preview,id,content,html_content,is_external) VALUES('text','fixture',1,'preview',1,?1,NULL,1)",
            [&cipher],
        )
        .unwrap();
        let before = std::fs::read(&path).unwrap();
        let error = prepare_attachment_path_rewrites(&path, &directory, &directory.join("new"))
            .unwrap_err();
        assert!(error.to_string().contains("ProtectedRecordUnavailable"));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        drop(conn);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn path_rewrite_sql_failure_rolls_back_every_updated_row() {
        let (directory, path) = database();
        let conn = fixture_connection(&path);
        conn.execute_batch("INSERT INTO clipboard_history(content_type,source_app,timestamp,preview,id,content,html_content,is_external) VALUES('text','fixture',1,'preview',1,'original one',NULL,1),('text','fixture',1,'preview',2,'original two',NULL,1);
            CREATE TRIGGER reject_second BEFORE UPDATE ON clipboard_history WHEN new.id=2 BEGIN SELECT RAISE(ABORT,'rejected'); END;").unwrap();
        let updates: Vec<_> = [
            (1, "original one", "new one"),
            (2, "original two", "new two"),
        ]
        .into_iter()
        .map(|(id, original, updated)| AttachmentPathRewrite {
            id,
            original_content: original.to_string(),
            original_html: None,
            content: updated.to_string(),
            html: None,
        })
        .collect();
        let error = apply_attachment_path_rewrites(&path, &updates).unwrap_err();
        assert!(error.to_string().contains("rejected"));
        let values: Vec<String> = conn
            .prepare("SELECT content FROM clipboard_history ORDER BY id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(values, vec!["original one", "original two"]);
        drop(conn);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn path_rewrite_concurrent_edit_preserves_new_values_and_rolls_back_earlier_rows() {
        let (directory, path) = database();
        let first = directory
            .join("attachments")
            .join("one.png")
            .to_string_lossy()
            .to_string();
        let second = directory
            .join("attachments")
            .join("two.png")
            .to_string_lossy()
            .to_string();
        let reader = fixture_connection(&path);
        reader
            .execute(
                "INSERT INTO clipboard_history(content_type,source_app,timestamp,preview,id,content,html_content,is_external) VALUES('text','fixture',1,'preview',1,?1,NULL,1),('text','fixture',1,'preview',2,?2,NULL,1)",
                params![first, second],
            )
            .unwrap();
        let updates =
            prepare_attachment_path_rewrites(&path, &directory, &directory.join("new")).unwrap();
        let writer = fixture_connection(&path);
        writer.execute("UPDATE clipboard_history SET content='concurrent edit', html_content='<p>new HTML</p>' WHERE id=2", []).unwrap();
        let error = apply_attachment_path_rewrites(&path, &updates).unwrap_err();
        assert!(error.to_string().contains("SensitivePersistenceRejected"));
        let unchanged: String = reader
            .query_row(
                "SELECT content FROM clipboard_history WHERE id=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let edited: (String, String) = reader
            .query_row(
                "SELECT content,html_content FROM clipboard_history WHERE id=2",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(unchanged, first);
        assert_eq!(
            edited,
            ("concurrent edit".to_string(), "<p>new HTML</p>".to_string())
        );
        drop(writer);
        drop(reader);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn path_rewrite_concurrent_new_attachment_row_rejects_the_prepared_plan() {
        let (directory, path) = database();
        let original = directory
            .join("attachments")
            .join("one.png")
            .to_string_lossy()
            .to_string();
        let reader = fixture_connection(&path);
        reader
            .execute(
                "INSERT INTO clipboard_history(content_type,source_app,timestamp,preview,id,content,html_content,is_external) VALUES('text','fixture',1,'preview',1,?1,NULL,1)",
                [&original],
            )
            .unwrap();
        let updates =
            prepare_attachment_path_rewrites(&path, &directory, &directory.join("new")).unwrap();
        let writer = fixture_connection(&path);
        writer
            .execute(
                "INSERT INTO clipboard_history(content_type,source_app,timestamp,preview,id,content,html_content,is_external) VALUES('text','fixture',1,'preview',2,'concurrent attachment',NULL,1)",
                [],
            )
            .unwrap();
        let error = apply_attachment_path_rewrites(&path, &updates).unwrap_err();
        assert!(error.to_string().contains("SensitivePersistenceRejected"));
        let unchanged: String = reader
            .query_row(
                "SELECT content FROM clipboard_history WHERE id=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(unchanged, original);
        let inserted: String = reader
            .query_row(
                "SELECT content FROM clipboard_history WHERE id=2",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(inserted, "concurrent attachment");
        drop(writer);
        drop(reader);
        std::fs::remove_dir_all(directory).unwrap();
    }

}
