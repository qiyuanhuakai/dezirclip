use super::sensitive_tags::{AttachmentCleanup, SensitiveTagDeletion};
use super::tag_page::{register_tag_matcher, TAG_MATCH_FILTER};
use super::tag_repo::SqliteTagRepository;
use rusqlite::{params, TransactionBehavior};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::Path;

pub const MAX_TAG_SELECTION_IDS: usize = 10_000;

#[derive(Debug, Deserialize)]
#[serde(tag = "mode", rename_all = "camelCase", deny_unknown_fields)]
pub enum TagEntrySelection {
    Selected {
        ids: Vec<i64>,
    },
    All {
        #[serde(default)]
        query: String,
        #[serde(default, rename = "excludedIds")]
        excluded_ids: Vec<i64>,
    },
}

fn checked_ids(ids: &[i64]) -> Result<HashSet<i64>, String> {
    if ids.len() > MAX_TAG_SELECTION_IDS || ids.iter().any(|id| *id <= 0) {
        return Err("tag selections support at most 10000 positive entry IDs".into());
    }
    Ok(ids.iter().copied().collect())
}

impl SqliteTagRepository {
    /// Delete content within one exact tag, preserving the tag and its color.
    /// Explicit selections are all-or-nothing; all mode resolves current search
    /// matches inside the same write transaction, including unloaded pages.
    pub fn delete_entries_for_tag(
        &self,
        tag: &str,
        selection: TagEntrySelection,
        data_dir: Option<&Path>,
    ) -> Result<SensitiveTagDeletion, String> {
        if tag.is_empty() || tag.chars().count() > 80 || tag.chars().any(char::is_control) {
            return Err("tag names must contain 1..80 printable characters".into());
        }
        let ids = match &selection {
            TagEntrySelection::Selected { ids } => checked_ids(ids)?,
            TagEntrySelection::All {
                query,
                excluded_ids,
            } => {
                if query.chars().count() > 256 {
                    return Err("invalid tag page query".into());
                }
                checked_ids(excluded_ids)?
            }
        };
        if matches!(&selection, TagEntrySelection::Selected { .. }) && ids.is_empty() {
            return Ok(SensitiveTagDeletion::default());
        }
        let mut guard = self.conn.lock().map_err(|error| error.to_string())?;
        register_tag_matcher(&guard)?;
        let transaction = guard
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| error.to_string())?;
        let known: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM saved_tags WHERE name=?1) OR EXISTS(SELECT 1 FROM entry_tags WHERE tag=?1)",
            [tag], |row| row.get(0),
        ).map_err(|error| error.to_string())?;
        if !known {
            return Err("tag no longer exists; refresh the tag library".into());
        }
        let deleted_ids = match selection {
            TagEntrySelection::Selected { .. } => {
                let mut statement = transaction.prepare("SELECT EXISTS(SELECT 1 FROM clipboard_history ch JOIN entry_tags et ON et.entry_id=ch.id WHERE ch.id=?1 AND et.tag=?2)")
                    .map_err(|error| error.to_string())?;
                for id in &ids {
                    let belongs: bool = statement
                        .query_row(params![id, tag], |row| row.get(0))
                        .map_err(|error| error.to_string())?;
                    if !belongs {
                        return Err("selected entries no longer belong to this tag; refresh the tag library".into());
                    }
                }
                ids
            }
            TagEntrySelection::All { query, .. } => {
                let mut statement = transaction
                    .prepare(&format!("SELECT ch.id {TAG_MATCH_FILTER}"))
                    .map_err(|error| error.to_string())?;
                let matches = statement
                    .query_map(params![tag, query], |row| row.get::<_, i64>(0))
                    .map_err(|error| error.to_string())?
                    .collect::<rusqlite::Result<HashSet<_>>>()
                    .map_err(|error| error.to_string())?;
                matches.difference(&ids).copied().collect()
            }
        };
        if deleted_ids.is_empty() {
            transaction.commit().map_err(|error| error.to_string())?;
            return Ok(SensitiveTagDeletion::default());
        }
        transaction
            .execute("INSERT OR IGNORE INTO saved_tags(name) VALUES(?1)", [tag])
            .map_err(|error| error.to_string())?;
        let mut cleanup = AttachmentCleanup::for_entries(&transaction, data_dir, &deleted_ids)?;
        {
            let mut delete_tags = transaction
                .prepare("DELETE FROM entry_tags WHERE entry_id=?1")
                .map_err(|error| error.to_string())?;
            let mut delete_history = transaction
                .prepare("DELETE FROM clipboard_history WHERE id=?1")
                .map_err(|error| error.to_string())?;
            for id in &deleted_ids {
                delete_tags
                    .execute([id])
                    .map_err(|error| error.to_string())?;
                delete_history
                    .execute([id])
                    .map_err(|error| error.to_string())?;
            }
        }
        cleanup.protect_surviving_references(&transaction)?;
        transaction.commit().map_err(|error| error.to_string())?;
        Ok(SensitiveTagDeletion {
            deleted_ids,
            cleanup_errors: cleanup.finish(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::migrations::run_migrations;
    use super::super::tag_repo::TagRepository;
    use super::*;
    use rusqlite::Connection;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    struct Fixture {
        directory: PathBuf,
        conn: Arc<Mutex<Connection>>,
    }
    impl Fixture {
        fn new() -> Self {
            let directory =
                std::env::temp_dir().join(format!("dezirclip-tag-batch-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(directory.join("attachments")).unwrap();
            let mut conn = Connection::open_in_memory().unwrap();
            run_migrations(&mut conn).unwrap();
            crate::database::seed_defaults(&conn).unwrap();
            conn.execute(
                "INSERT INTO saved_tags(name,color) VALUES('work','#123456')",
                [],
            )
            .unwrap();
            Self {
                directory,
                conn: Arc::new(Mutex::new(conn)),
            }
        }
        fn repo(&self) -> SqliteTagRepository {
            SqliteTagRepository::new(self.conn.clone())
        }
        fn file(&self, name: &str) -> PathBuf {
            let path = self.directory.join("attachments").join(name);
            std::fs::write(&path, name).unwrap();
            path
        }
        fn entry(&self, id: i64, tag: &str, content: &str, html: Option<&str>, external: bool) {
            let conn = self.conn.lock().unwrap();
            conn.execute("INSERT INTO clipboard_history(id,content_type,content,html_content,source_app,timestamp,preview,tags,is_external,is_pinned)
                VALUES(?1,'text',?2,?3,'fixture',?1,'preview',?4,?5,1)",
                params![id,content,html,serde_json::to_string(&[tag]).unwrap(),external]).unwrap();
            conn.execute(
                "INSERT INTO entry_tags(entry_id,tag) VALUES(?1,?2)",
                params![id, tag],
            )
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
        fn color(&self) -> String {
            self.conn
                .lock()
                .unwrap()
                .query_row(
                    "SELECT color FROM saved_tags WHERE name='work'",
                    [],
                    |row| row.get(0),
                )
                .unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let directory = self.directory.canonicalize().unwrap();
            assert!(directory.starts_with(std::env::temp_dir().canonicalize().unwrap()));
            assert!(directory
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("dezirclip-tag-batch-"));
            std::fs::remove_dir_all(directory).unwrap();
        }
    }
    fn selected(ids: Vec<i64>) -> TagEntrySelection {
        TagEntrySelection::Selected { ids }
    }
    fn all(query: &str, excluded_ids: Vec<i64>) -> TagEntrySelection {
        TagEntrySelection::All {
            query: query.into(),
            excluded_ids,
        }
    }
    fn file_uri(path: &Path) -> String {
        let path = path.to_str().unwrap().replace('\\', "/");
        let encoded = path
            .split('/')
            .map(urlencoding::encode)
            .collect::<Vec<_>>()
            .join("/");
        format!(
            "{}{}",
            if path.starts_with('/') {
                "file://"
            } else {
                "file:///"
            },
            encoded
        )
    }

    #[test]
    fn explicit_batch_deletes_only_selected_exact_tag_entries_and_preserves_tag_color() {
        let fixture = Fixture::new();
        for (id, tag) in [(1, "work"), (2, "work"), (3, "Work"), (4, "other")] {
            fixture.entry(id, tag, "original", None, false);
        }
        fixture
            .conn
            .lock()
            .unwrap()
            .execute("INSERT INTO entry_tags(entry_id,tag) VALUES(1,'also')", [])
            .unwrap();
        let report = fixture
            .repo()
            .delete_entries_for_tag("work", selected(vec![1, 1]), Some(&fixture.directory))
            .unwrap();
        assert_eq!(report.deleted_ids, HashSet::from([1]));
        assert!(report.cleanup_errors.is_empty());
        assert_eq!(fixture.count("clipboard_history"), 3);
        assert_eq!(fixture.count("entry_tags"), 3);
        assert_eq!(fixture.color(), "#123456");
        assert_eq!(fixture.repo().get_entries_by_tag("work").unwrap()[0].id, 2);
        assert_eq!(fixture.repo().get_entries_by_tag("Work").unwrap()[0].id, 3);
    }

    #[test]
    fn invalid_or_foreign_selections_reject_entire_batch_without_writes() {
        let fixture = Fixture::new();
        fixture.entry(1, "work", "keep", None, false);
        fixture.entry(2, "other", "keep", None, false);
        for ids in [
            vec![1, 2],
            vec![1, 999],
            vec![0],
            vec![-1],
            vec![1; MAX_TAG_SELECTION_IDS + 1],
        ] {
            assert!(fixture
                .repo()
                .delete_entries_for_tag("work", selected(ids), None)
                .is_err());
        }
        for tag in ["unknown", "WORK", "work' OR 1=1 --", "", "work\n"] {
            assert!(fixture
                .repo()
                .delete_entries_for_tag(tag, selected(vec![1]), None)
                .is_err());
        }
        assert!(fixture
            .repo()
            .delete_entries_for_tag("work", all("", vec![1; MAX_TAG_SELECTION_IDS + 1]), None)
            .is_err());
        assert!(fixture
            .repo()
            .delete_entries_for_tag("work", all(&"q".repeat(257), vec![]), None)
            .is_err());
        assert_eq!(fixture.count("clipboard_history"), 2);
        assert_eq!(fixture.count("entry_tags"), 2);
        assert_eq!(fixture.color(), "#123456");
    }

    #[test]
    fn empty_selection_and_no_matching_all_query_delete_nothing() {
        let fixture = Fixture::new();
        fixture.entry(1, "work", "keep", None, false);
        for selection in [selected(vec![]), all("no match", vec![]), all("", vec![1])] {
            let report = fixture
                .repo()
                .delete_entries_for_tag("work", selection, None)
                .unwrap();
            assert!(report.deleted_ids.is_empty());
            assert!(report.cleanup_errors.is_empty());
        }
        assert_eq!(fixture.count("clipboard_history"), 1);
        assert_eq!(fixture.count("entry_tags"), 1);
    }

    #[test]
    fn all_search_deletes_unloaded_matches_preserving_exclusions_and_other_tags() {
        let fixture = Fixture::new();
        for id in 1..=170 {
            fixture.entry(
                id,
                "work",
                if id <= 145 {
                    "needle 中文 100%_"
                } else {
                    "leave"
                },
                None,
                false,
            );
        }
        fixture.entry(171, "other", "needle 中文 100%_", None, false);
        let query = "NEEDLE 中文 100%_";
        let before = fixture
            .repo()
            .get_page("work", query, "time", 60, 0)
            .unwrap();
        assert_eq!(before.total, 145);
        assert_eq!(before.items.len(), 60);
        let report = fixture
            .repo()
            .delete_entries_for_tag("work", all(query, vec![2, 145, 171]), None)
            .unwrap();
        let expected: HashSet<_> = (1..=145).filter(|id| *id != 2 && *id != 145).collect();
        assert_eq!(report.deleted_ids, expected);
        assert_eq!(fixture.count("clipboard_history"), 28);
        assert_eq!(fixture.count("entry_tags"), 28);
        assert_eq!(
            fixture
                .repo()
                .get_page("work", query, "count", 60, 0)
                .unwrap()
                .total,
            2
        );
        assert_eq!(fixture.color(), "#123456");
    }

    #[test]
    fn all_query_reuses_full_content_preview_source_and_ocr_page_matcher() {
        let fixture = Fixture::new();
        fixture.entry(
            1,
            "work",
            &format!("{} final-needle", "x".repeat(50000)),
            None,
            false,
        );
        for id in 2..=5 {
            fixture.entry(id, "work", "keep content", None, false);
        }
        {
            let conn = fixture.conn.lock().unwrap();
            conn.execute(
                "UPDATE clipboard_history SET preview='FINAL-NEEDLE' WHERE id=2",
                [],
            )
            .unwrap();
            conn.execute(
                "UPDATE clipboard_history SET source_app='final-needle source' WHERE id=3",
                [],
            )
            .unwrap();
            conn.execute(
                "UPDATE clipboard_history SET ocr_text='OCR Final-Needle' WHERE id=4",
                [],
            )
            .unwrap();
        }
        let expected: HashSet<_> = fixture
            .repo()
            .get_page("work", "final-needle", "time", 100, 0)
            .unwrap()
            .items
            .iter()
            .map(|entry| entry.id)
            .collect();
        assert_eq!(expected, HashSet::from([1, 2, 3, 4]));
        let report = fixture
            .repo()
            .delete_entries_for_tag("work", all("final-needle", vec![]), None)
            .unwrap();
        assert_eq!(report.deleted_ids, expected);
        assert_eq!(fixture.count("clipboard_history"), 1);
    }

    #[test]
    fn batch_content_delete_allows_privacy_on_but_preserves_sensitive_tag_and_guard() {
        let fixture = Fixture::new();
        fixture.entry(1, "密码", "dpapi:unchanged stored payload", None, false);
        let report = fixture
            .repo()
            .delete_entries_for_tag("密码", all("", vec![]), None)
            .unwrap();
        assert_eq!(report.deleted_ids, HashSet::from([1]));
        let conn = fixture.conn.lock().unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT value FROM settings WHERE key='app.privacy_protection'",
                [],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
            "true"
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
        drop(conn);
        assert!(fixture.repo().delete_globally("密码", None).is_err());
        assert_eq!(fixture.count("clipboard_history"), 0);
    }

    #[test]
    fn batch_delete_failure_rolls_back_history_tags_and_attachment_cleanup() {
        let fixture = Fixture::new();
        let image = fixture.file("rollback.png");
        fixture.entry(1, "work", image.to_str().unwrap(), None, true);
        fixture.entry(2, "work", "keep", None, false);
        fixture.conn.lock().unwrap().execute_batch("CREATE TRIGGER reject_batch BEFORE DELETE ON clipboard_history WHEN OLD.id=2 BEGIN SELECT RAISE(ABORT,'blocked'); END;").unwrap();
        assert!(fixture
            .repo()
            .delete_entries_for_tag("work", all("", vec![]), Some(&fixture.directory))
            .is_err());
        assert_eq!(fixture.count("clipboard_history"), 2);
        assert_eq!(fixture.count("entry_tags"), 2);
        assert_eq!(fixture.color(), "#123456");
        assert!(image.exists());
    }

    #[test]
    fn batch_cleanup_preserves_excluded_html_shares_and_unknown_comment_resources() {
        for markup in [
            "<img src='RESOURCE'>",
            "<!--><img src='RESOURCE'><!-- tail -->",
            "<div style=\"background:url('RESOURCE')\"></div>",
        ] {
            let fixture = Fixture::new();
            let shared = fixture.file("shared.png");
            let exclusive = fixture.file("exclusive.png");
            fixture.entry(1, "work", shared.to_str().unwrap(), None, true);
            fixture.entry(
                2,
                "work",
                "HTML",
                Some(&markup.replace("RESOURCE", &file_uri(&shared))),
                false,
            );
            fixture.entry(3, "work", exclusive.to_str().unwrap(), None, true);
            let report = fixture
                .repo()
                .delete_entries_for_tag("work", all("", vec![2]), Some(&fixture.directory))
                .unwrap();
            assert_eq!(report.deleted_ids, HashSet::from([1, 3]));
            assert!(report.cleanup_errors.is_empty());
            assert!(shared.exists());
            if markup.starts_with("<img") {
                assert!(!exclusive.exists());
            } else {
                assert!(
                    exclusive.exists(),
                    "unknown surviving references retain every candidate"
                );
            }
            assert_eq!(fixture.count("clipboard_history"), 1);
        }
    }

    #[test]
    fn deleting_last_contents_saves_previously_implicit_tag_without_resurrecting_entries() {
        let fixture = Fixture::new();
        fixture.entry(1, "implicit", "last", None, false);
        let report = fixture
            .repo()
            .delete_entries_for_tag("implicit", selected(vec![1]), None)
            .unwrap();
        assert_eq!(report.deleted_ids, HashSet::from([1]));
        assert_eq!(fixture.repo().get_all_with_counts().unwrap()["implicit"], 0);
        assert_eq!(fixture.count("clipboard_history"), 0);
        assert_eq!(fixture.count("entry_tags"), 0);
    }

    #[test]
    fn selection_wire_contract_rejects_ambiguous_or_unknown_fields() {
        let all: TagEntrySelection =
            serde_json::from_str(r#"{"mode":"all","excludedIds":[2]}"#).unwrap();
        assert!(
            matches!(all,TagEntrySelection::All{query,excluded_ids} if query.is_empty() && excluded_ids==[2])
        );
        assert!(
            serde_json::from_str::<TagEntrySelection>(r#"{"mode":"selected","ids":[1,2]}"#).is_ok()
        );
        for invalid in [
            r#"{"mode":"selected"}"#,
            r#"{"mode":"all","ids":[1]}"#,
            r#"{"mode":"selected","ids":[1],"excludedIds":[]}"#,
            r#"{"mode":"selected","ids":[1.5]}"#,
            r#"{"mode":"everything"}"#,
        ] {
            assert!(
                serde_json::from_str::<TagEntrySelection>(invalid).is_err(),
                "{invalid}"
            );
        }
    }
}
