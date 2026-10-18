use super::clipboard_repo::SqliteClipboardRepository;
use super::settings_repo::SqliteSettingsRepository;
use super::tag_repo::SqliteTagRepository;
use crate::database::SENSITIVE_TAGS;
use crate::infrastructure::{attachments, encryption};
use rusqlite::{params, Connection, TransactionBehavior};
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

pub const PRIVACY_SETTING: &str = "app.privacy_protection";

/// Call within the setting transaction so an enabled setting cannot be saved
/// without both system tags. Existing tag colors and history are untouched.
pub fn ensure_system_tags(conn: &Connection) -> rusqlite::Result<()> {
    for name in SENSITIVE_TAGS {
        conn.execute(
            "INSERT OR IGNORE INTO saved_tags (name) VALUES (?1)",
            [name],
        )?;
    }
    Ok(())
}

pub fn ensure_tags_if_protected(conn: &Connection) -> rusqlite::Result<()> {
    let transaction = conn.unchecked_transaction()?;
    if SqliteSettingsRepository::get_raw(&transaction, PRIVACY_SETTING)?.as_deref() != Some("false")
    {
        ensure_system_tags(&transaction)?;
    }
    transaction.commit()
}

#[derive(Debug, Default)]
pub struct SensitiveTagDeletion {
    pub deleted_ids: HashSet<i64>,
    /// The database commit succeeded; callers must refresh their state even if
    /// an attachment could not be removed afterwards.
    pub cleanup_errors: Vec<String>,
}

struct AttachmentOwner {
    configured_root: PathBuf,
    canonical_root: PathBuf,
}

impl AttachmentOwner {
    fn new(data_dir: &Path) -> Result<Option<Self>, String> {
        let configured_root = data_dir.join("attachments");
        let metadata = match std::fs::symlink_metadata(&configured_root) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("inspect attachments directory: {error}")),
        };
        let canonical_data = data_dir.canonicalize().map_err(|error| error.to_string())?;
        let canonical_root = configured_root
            .canonicalize()
            .map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink()
            || !metadata.is_dir()
            || !canonical_root.starts_with(&canonical_data)
            || canonical_root == canonical_data
        {
            return Err(
                "attachments directory must be an owned directory, without a symbolic link".into(),
            );
        }
        Ok(Some(Self {
            configured_root,
            canonical_root,
        }))
    }

    fn owned_file(&self, path: &Path) -> Result<Option<PathBuf>, String> {
        let relative = match path
            .strip_prefix(&self.configured_root)
            .or_else(|_| path.strip_prefix(&self.canonical_root))
        {
            Ok(relative) => relative,
            Err(_) => return Ok(None),
        };
        let mut checked = self.configured_root.clone();
        for component in relative.components() {
            let Component::Normal(component) = component else {
                return Ok(None);
            };
            checked.push(component);
            let metadata = match std::fs::symlink_metadata(&checked) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(format!("inspect attachment: {error}")),
            };
            if metadata.file_type().is_symlink() {
                return Ok(None);
            }
        }
        let canonical = match checked.canonicalize() {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("resolve attachment: {error}")),
        };
        if !canonical.starts_with(&self.canonical_root) || !canonical.is_file() {
            return Ok(None);
        }
        Ok(Some(canonical))
    }

    fn references(
        &self,
        content: &str,
        html: Option<&str>,
        external: bool,
    ) -> Result<HashSet<PathBuf>, String> {
        let mut paths = HashSet::new();
        if external {
            let content = decrypt_reference(content)?;
            let path = attachments::path_from_uri(&content)?;
            if let Some(path) = self.owned_file(Path::new(&path))? {
                paths.insert(path);
            }
        }
        if let Some(html) = html {
            let html = decrypt_reference(html)?;
            if let Some(payload) =
                SqliteClipboardRepository::extract_rich_image_fallback_payload(&html)
            {
                if !payload.starts_with("data:") {
                    let path = attachments::path_from_uri(&payload)?;
                    if let Some(path) = self.owned_file(Path::new(&path))? {
                        paths.insert(path);
                    }
                }
            }
        }
        Ok(paths)
    }
}

fn decrypt_reference(value: &str) -> Result<String, String> {
    match encryption::decode_stored_text(value) {
        encryption::StoredText::Plain(value) => Ok(value.to_owned()),
        encryption::StoredText::Decrypted(value) => Ok(value),
        encryption::StoredText::Failed => Err(
            "cannot safely resolve an encrypted attachment reference; history was retained".into(),
        ),
    }
}

impl SqliteTagRepository {
    pub fn delete_sensitive_with_contents(
        &self,
        name: &str,
        data_dir: Option<&Path>,
    ) -> Result<SensitiveTagDeletion, String> {
        if !SENSITIVE_TAGS
            .iter()
            .any(|system| system.eq_ignore_ascii_case(name))
        {
            return Err(
                "only the system tags sensitive and 密码 can delete their associated history"
                    .into(),
            );
        }
        let mut guard = self.conn.lock().map_err(|error| error.to_string())?;
        // IMMEDIATE also serializes a concurrent CLI/settings connection. The
        // permission check and every destructive database write share this lock.
        let transaction = guard
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| error.to_string())?;
        if SqliteSettingsRepository::get_raw(&transaction, PRIVACY_SETTING)
            .map_err(|error| error.to_string())?
            .as_deref()
            != Some("false")
        {
            return Err(
                "disable privacy protection before deleting a system sensitive tag and its history"
                    .into(),
            );
        }
        let mut report = SensitiveTagDeletion::default();
        let mut stmt = transaction
            .prepare("SELECT DISTINCT entry_id FROM entry_tags WHERE tag = ?1")
            .map_err(|error| error.to_string())?;
        report.deleted_ids = stmt
            .query_map([name], |row| row.get(0))
            .map_err(|error| error.to_string())?
            .collect::<rusqlite::Result<_>>()
            .map_err(|error| error.to_string())?;
        drop(stmt);

        let mut candidates = HashSet::new();
        let owner = data_dir.map(AttachmentOwner::new).transpose()?.flatten();
        if let Some(owner) = &owner {
            let mut stmt = transaction
                .prepare(
                    "SELECT ch.content, ch.html_content, ch.is_external FROM clipboard_history ch
                 INNER JOIN entry_tags et ON ch.id = et.entry_id WHERE et.tag = ?1",
                )
                .map_err(|error| error.to_string())?;
            let rows = stmt
                .query_map([name], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, bool>(2)?,
                    ))
                })
                .map_err(|error| error.to_string())?;
            for row in rows {
                let (content, html, external) = row.map_err(|error| error.to_string())?;
                candidates.extend(owner.references(&content, html.as_deref(), external)?);
            }
        }

        transaction.execute("DELETE FROM entry_tags WHERE entry_id IN (SELECT entry_id FROM entry_tags WHERE tag = ?1)", [name])
            .map_err(|error| error.to_string())?;
        // IDs were collected before removing associations; include every
        // association on a removed row, without relying on absent FK cascades.
        for id in &report.deleted_ids {
            transaction
                .execute("DELETE FROM clipboard_history WHERE id = ?1", params![id])
                .map_err(|error| error.to_string())?;
        }
        transaction
            .execute("DELETE FROM saved_tags WHERE name = ?1", [name])
            .map_err(|error| error.to_string())?;

        if let Some(owner) = &owner {
            if !candidates.is_empty() {
                let mut stmt = transaction.prepare("SELECT content, html_content, is_external FROM clipboard_history WHERE is_external = 1 OR html_content IS NOT NULL")
                    .map_err(|error| error.to_string())?;
                let rows = stmt
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, Option<String>>(1)?,
                            row.get::<_, bool>(2)?,
                        ))
                    })
                    .map_err(|error| error.to_string())?;
                for row in rows {
                    let (content, html, external) = row.map_err(|error| error.to_string())?;
                    for shared in owner.references(&content, html.as_deref(), external)? {
                        candidates.remove(&shared);
                    }
                }
            }
        }
        transaction.commit().map_err(|error| error.to_string())?;
        // Keep the connection locked through cleanup so in-process captures
        // cannot add a reference between the shared-file check and removal.
        if let Some(owner) = &owner {
            for path in candidates {
                match owner.owned_file(&path) {
                    Ok(Some(verified)) if verified == path => {
                        if let Err(error) = std::fs::remove_file(&verified) {
                            if error.kind() != std::io::ErrorKind::NotFound {
                                report
                                    .cleanup_errors
                                    .push(format!("remove attachment: {error}"));
                            }
                        }
                    }
                    Ok(None) if !path.exists() => {}
                    Ok(_) => report
                        .cleanup_errors
                        .push("attachment path changed during cleanup; file retained".into()),
                    Err(error) => report.cleanup_errors.push(error),
                }
            }
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::super::migrations::run_migrations;
    use super::super::settings_repo::SettingsRepository;
    use super::super::tag_repo::TagRepository;
    use super::*;
    use std::sync::{Arc, Mutex};

    struct Fixture {
        directory: PathBuf,
        conn: Arc<Mutex<Connection>>,
    }

    impl Fixture {
        fn new() -> Self {
            let directory = std::env::temp_dir()
                .join(format!("dezirclip-sensitive-tags-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(directory.join("attachments")).unwrap();
            let mut conn = Connection::open_in_memory().unwrap();
            run_migrations(&mut conn).unwrap();
            crate::database::seed_defaults(&conn).unwrap();
            Self {
                directory,
                conn: Arc::new(Mutex::new(conn)),
            }
        }

        fn repository(&self) -> SqliteTagRepository {
            SqliteTagRepository::new(self.conn.clone())
        }
        fn settings(&self) -> SqliteSettingsRepository {
            SqliteSettingsRepository::new(self.conn.clone())
        }
        fn file(&self, name: &str) -> PathBuf {
            let path = self.directory.join("attachments").join(name);
            std::fs::write(&path, name).unwrap();
            path
        }

        fn entry(&self, id: i64, tag: &str, content: &str, external: bool, html: Option<&str>) {
            let conn = self.conn.lock().unwrap();
            conn.execute("INSERT INTO clipboard_history (id, content_type, content, html_content, source_app, timestamp, preview, tags, is_external, is_pinned)
                 VALUES (?1, 'text', ?2, ?3, 'test', ?1, 'preview', ?4, ?5, 1)",
                params![id, content, html, serde_json::to_string(&vec![tag]).unwrap(), external]).unwrap();
            conn.execute(
                "INSERT INTO entry_tags (entry_id, tag) VALUES (?1, ?2)",
                params![id, tag],
            )
            .unwrap();
            conn.execute("INSERT OR IGNORE INTO saved_tags(name) VALUES (?1)", [tag])
                .unwrap();
        }

        fn count(&self, table: &str) -> i64 {
            self.conn
                .lock()
                .unwrap()
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let parent = std::env::temp_dir().canonicalize().unwrap();
            let resolved = self.directory.canonicalize().unwrap();
            assert!(resolved.starts_with(parent));
            assert!(resolved
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("dezirclip-sensitive-tags-"));
            std::fs::remove_dir_all(resolved).unwrap();
        }
    }

    #[test]
    fn sensitive_delete_rejects_protected_missing_or_invalid_setting_without_changes() {
        for setting in [
            Some("true"),
            None,
            Some("TRUE"),
            Some("plain:false"),
            Some(""),
        ] {
            let fixture = Fixture::new();
            let file = fixture.file("protected.txt");
            fixture.entry(1, "sensitive", file.to_str().unwrap(), true, None);
            {
                let conn = fixture.conn.lock().unwrap();
                conn.execute("DELETE FROM settings WHERE key = ?1", [PRIVACY_SETTING])
                    .unwrap();
                if let Some(value) = setting {
                    conn.execute(
                        "INSERT INTO settings(key,value) VALUES (?1,?2)",
                        params![PRIVACY_SETTING, value],
                    )
                    .unwrap();
                }
            }
            let before = fixture.count("saved_tags");
            assert!(fixture
                .repository()
                .delete_globally("sensitive", Some(&fixture.directory))
                .is_err());
            assert_eq!(fixture.count("clipboard_history"), 1);
            assert_eq!(fixture.count("entry_tags"), 1);
            assert_eq!(fixture.count("saved_tags"), before);
            assert!(file.exists());
        }
    }

    #[test]
    fn sensitive_delete_requires_system_name_and_uses_exact_associations() {
        let fixture = Fixture::new();
        fixture.settings().set(PRIVACY_SETTING, "false").unwrap();
        fixture.entry(1, "sensitive", "lowercase", false, None);
        fixture.entry(2, "Sensitive", "uppercase", false, None);
        fixture.entry(3, "personal", "ordinary", false, None);
        for name in ["personal", "password", "敏感", " sensitive", "密码 "] {
            assert!(fixture.repository().delete_globally(name, None).is_err());
        }
        let report = fixture
            .repository()
            .delete_globally("Sensitive", None)
            .unwrap();
        assert_eq!(report.deleted_ids, HashSet::from([2]));
        assert_eq!(fixture.count("clipboard_history"), 2);
        assert!(fixture
            .repository()
            .get_entries_by_tag("sensitive")
            .unwrap()
            .iter()
            .any(|entry| entry.id == 1));
        fixture.settings().set(PRIVACY_SETTING, "true").unwrap();
        assert!(fixture
            .repository()
            .delete_globally("SENSITIVE", None)
            .is_err());
    }

    #[test]
    fn sensitive_delete_cleans_only_unshared_owned_attachments_and_all_associations() {
        let fixture = Fixture::new();
        fixture.settings().set(PRIVACY_SETTING, "false").unwrap();
        let removed = fixture.file("removed.txt");
        let shared = fixture.file("shared.txt");
        let rich = fixture.file("rich-image.png");
        let original = fixture.directory.join("user-original.txt");
        std::fs::write(&original, "user file").unwrap();
        fixture.entry(1, "密码", removed.to_str().unwrap(), true, None);
        fixture.entry(2, "密码", shared.to_str().unwrap(), true, None);
        fixture.entry(3, "keep", shared.to_str().unwrap(), true, None);
        fixture.entry(
            4,
            "密码",
            "rich image",
            false,
            Some(&format!(
                "<p>image</p><!--TIEZ_RICH_IMAGE:{}-->",
                rich.display()
            )),
        );
        fixture.entry(5, "密码", original.to_str().unwrap(), true, None);
        fixture
            .conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO entry_tags(entry_id,tag) VALUES (1,'other')",
                [],
            )
            .unwrap();
        let report = fixture
            .repository()
            .delete_globally("密码", Some(&fixture.directory))
            .unwrap();
        assert_eq!(report.deleted_ids, HashSet::from([1, 2, 4, 5]));
        assert!(
            report.cleanup_errors.is_empty(),
            "{:?}",
            report.cleanup_errors
        );
        assert_eq!(fixture.count("clipboard_history"), 1);
        assert_eq!(fixture.count("entry_tags"), 1);
        assert!(!removed.exists());
        assert!(!rich.exists());
        assert!(shared.exists());
        assert!(original.exists());
        assert_eq!(
            fixture
                .conn
                .lock()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM saved_tags WHERE name='密码'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn sensitive_delete_sql_failure_retains_tag_history_and_attachment() {
        let fixture = Fixture::new();
        fixture.settings().set(PRIVACY_SETTING, "false").unwrap();
        let file = fixture.file("rollback.txt");
        fixture.entry(1, "sensitive", file.to_str().unwrap(), true, None);
        fixture.conn.lock().unwrap().execute_batch("CREATE TRIGGER reject_tag_delete BEFORE DELETE ON saved_tags BEGIN SELECT RAISE(ABORT, 'blocked'); END;").unwrap();
        assert!(fixture
            .repository()
            .delete_globally("sensitive", Some(&fixture.directory))
            .is_err());
        assert_eq!(fixture.count("clipboard_history"), 1);
        assert_eq!(fixture.count("entry_tags"), 1);
        assert_eq!(fixture.count("saved_tags"), 2);
        assert!(file.exists());
    }

    #[test]
    fn sensitive_delete_accepts_plaintext_with_encryption_like_prefixes() {
        for value in ["linux: kernel notes", "dpapi:<p>HTML</p>"] {
            assert_eq!(decrypt_reference(value).unwrap(), value);
            let fixture = Fixture::new();
            fixture.settings().set(PRIVACY_SETTING, "false").unwrap();
            let file = fixture.file("prefix.txt");
            fixture.entry(1, "sensitive", file.to_str().unwrap(), true, Some(value));
            let report = fixture
                .repository()
                .delete_globally("sensitive", Some(&fixture.directory))
                .unwrap();
            assert_eq!(report.deleted_ids, HashSet::from([1]));
            assert!(report.cleanup_errors.is_empty());
            assert_eq!(fixture.count("clipboard_history"), 0);
            assert!(!file.exists());
        }
    }

    #[cfg(windows)]
    #[test]
    fn sensitive_delete_rejects_failed_ciphertext_envelope_and_retains_history() {
        use base64::Engine;
        // A foreign-platform envelope fails without accessing the user's keyring.
        let cipher = format!(
            "linux:{}",
            base64::engine::general_purpose::STANDARD.encode([42; 28])
        );
        let fixture = Fixture::new();
        fixture.settings().set(PRIVACY_SETTING, "false").unwrap();
        let file = fixture.file("failed-cipher.txt");
        fixture.entry(1, "sensitive", file.to_str().unwrap(), true, Some(&cipher));
        assert!(fixture
            .repository()
            .delete_globally("sensitive", Some(&fixture.directory))
            .is_err());
        assert_eq!(fixture.count("clipboard_history"), 1);
        assert_eq!(fixture.count("entry_tags"), 1);
        assert_eq!(fixture.count("saved_tags"), 2);
        assert!(file.exists());
    }

    #[cfg(windows)]
    #[test]
    fn sensitive_delete_reports_locked_attachment_after_successful_database_commit() {
        use std::os::windows::fs::OpenOptionsExt;
        let fixture = Fixture::new();
        fixture.settings().set(PRIVACY_SETTING, "false").unwrap();
        let file = fixture.file("locked.txt");
        fixture.entry(1, "sensitive", file.to_str().unwrap(), true, None);
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(3)
            .open(&file)
            .unwrap();
        let report = fixture
            .repository()
            .delete_globally("sensitive", Some(&fixture.directory))
            .unwrap();
        assert_eq!(report.deleted_ids, HashSet::from([1]));
        assert_eq!(fixture.count("clipboard_history"), 0);
        assert_eq!(fixture.count("entry_tags"), 0);
        assert!(!report.cleanup_errors.is_empty());
        assert!(file.exists());
        drop(lock);
    }

    #[test]
    fn privacy_enable_recreates_both_tags_atomically_without_touching_ciphertext() {
        let fixture = Fixture::new();
        let settings = fixture.settings();
        settings.set(PRIVACY_SETTING, "false").unwrap();
        fixture.entry(1, "keep", "linux:unavailable-key-ciphertext", false, None);
        {
            let conn = fixture.conn.lock().unwrap();
            conn.execute(
                "DELETE FROM saved_tags WHERE name IN ('sensitive','密码')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO saved_tags(name,color) VALUES ('sensitive','#123456')",
                [],
            )
            .unwrap();
            conn.execute_batch("CREATE TRIGGER reject_system_create BEFORE INSERT ON saved_tags WHEN NEW.name='密码' BEGIN SELECT RAISE(ABORT, 'blocked'); END;").unwrap();
        }
        assert!(settings.set(PRIVACY_SETTING, "true").is_err());
        assert_eq!(
            settings.get(PRIVACY_SETTING).unwrap().as_deref(),
            Some("false")
        );
        assert_eq!(fixture.count("saved_tags"), 2);
        fixture
            .conn
            .lock()
            .unwrap()
            .execute_batch("DROP TRIGGER reject_system_create;")
            .unwrap();
        settings.set(PRIVACY_SETTING, "true").unwrap();
        let conn = fixture.conn.lock().unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT color FROM saved_tags WHERE name='sensitive'",
                [],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
            "#123456"
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM saved_tags WHERE name IN ('sensitive','密码')",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            2
        );
        assert_eq!(
            conn.query_row(
                "SELECT content FROM clipboard_history WHERE id=1",
                [],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
            "linux:unavailable-key-ciphertext"
        );
    }

    #[test]
    fn startup_only_recreates_system_tags_when_protected_and_never_restores_history() {
        let fixture = Fixture::new();
        let settings = fixture.settings();
        settings.set(PRIVACY_SETTING, "false").unwrap();
        fixture.entry(1, "sensitive", "gone", false, None);
        fixture
            .repository()
            .delete_globally("sensitive", Some(&fixture.directory))
            .unwrap();
        fixture
            .repository()
            .delete_globally("密码", Some(&fixture.directory))
            .unwrap();
        {
            let conn = fixture.conn.lock().unwrap();
            crate::database::seed_defaults(&conn).unwrap();
        }
        assert_eq!(fixture.count("saved_tags"), 0);
        assert_eq!(fixture.count("clipboard_history"), 0);
        settings.set(PRIVACY_SETTING, "true").unwrap();
        fixture
            .conn
            .lock()
            .unwrap()
            .execute("DELETE FROM saved_tags", [])
            .unwrap();
        {
            let conn = fixture.conn.lock().unwrap();
            crate::database::seed_defaults(&conn).unwrap();
        }
        assert_eq!(fixture.count("saved_tags"), 2);
        assert_eq!(fixture.count("clipboard_history"), 0);
    }

    #[test]
    fn ordinary_remove_keeps_history_and_system_tags_cannot_use_that_path() {
        let fixture = Fixture::new();
        fixture.settings().set(PRIVACY_SETTING, "false").unwrap();
        fixture.entry(1, "ordinary", "keep me", false, None);
        fixture
            .repository()
            .remove_preserving_entries("ordinary")
            .unwrap();
        assert_eq!(fixture.count("clipboard_history"), 1);
        assert_eq!(fixture.count("entry_tags"), 0);
        for name in ["sensitive", "密码", "SENSITIVE"] {
            assert!(fixture
                .repository()
                .remove_preserving_entries(name)
                .is_err());
        }
        assert!(fixture.settings().set(PRIVACY_SETTING, "unknown").is_err());
        assert_eq!(
            fixture.settings().get(PRIVACY_SETTING).unwrap().as_deref(),
            Some("false")
        );
    }

    #[test]
    fn sensitive_delete_retains_paths_that_escape_the_owned_attachment_directory() {
        let fixture = Fixture::new();
        fixture.settings().set(PRIVACY_SETTING, "false").unwrap();
        let original = fixture.directory.join("original.txt");
        std::fs::write(&original, "keep").unwrap();
        let escaped = fixture
            .directory
            .join("attachments")
            .join("..")
            .join("original.txt");
        fixture.entry(1, "sensitive", escaped.to_str().unwrap(), true, None);
        fixture
            .repository()
            .delete_globally("sensitive", Some(&fixture.directory))
            .unwrap();
        assert!(original.exists());
        assert_eq!(fixture.count("clipboard_history"), 0);
    }

    #[cfg(unix)]
    #[test]
    fn sensitive_delete_retains_symbolic_link_targets_outside_attachments() {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::new();
        fixture.settings().set(PRIVACY_SETTING, "false").unwrap();
        let original = fixture.directory.join("original.txt");
        std::fs::write(&original, "keep").unwrap();
        let linked = fixture.directory.join("attachments/linked.txt");
        symlink(&original, &linked).unwrap();
        fixture.entry(1, "sensitive", linked.to_str().unwrap(), true, None);
        fixture
            .repository()
            .delete_globally("sensitive", Some(&fixture.directory))
            .unwrap();
        assert!(original.exists());
        assert!(linked.exists());
    }
}
