use super::tag_repo::SqliteTagRepository;
use crate::domain::models::ClipboardEntry;
use crate::infrastructure::encryption;
use rusqlite::{functions::FunctionFlags, params};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct TagItemsPage {
    pub items: Vec<ClipboardEntry>,
    pub total: usize,
    pub has_more: bool,
    pub next_offset: Option<usize>,
}

impl SqliteTagRepository {
    /// Select only a bounded preview page. Full payloads are read by ID on demand.
    pub fn get_page(
        &self,
        tag: &str,
        query: &str,
        sort: &str,
        limit: usize,
        offset: usize,
    ) -> Result<TagItemsPage, String> {
        if tag.trim().is_empty()
            || query.chars().count() > 256
            || limit == 0
            || limit > 100
            || offset > i32::MAX as usize
        {
            return Err("invalid tag page query".into());
        }
        let order = match sort {
            "time" => "ch.timestamp DESC,ch.id DESC",
            "count" => "ch.use_count DESC,ch.timestamp DESC,ch.id DESC",
            _ => return Err("unknown tag sort order".into()),
        };
        let mut conn = self.conn.lock().map_err(|error| error.to_string())?;
        conn.create_scalar_function("dc_tag_matches", 5, FunctionFlags::SQLITE_UTF8, |context| {
            let needle = context.get::<String>(4)?.to_lowercase();
            for index in 0..4 {
                let raw = context.get::<String>(index)?;
                let value = encryption::decrypt_value(&raw).unwrap_or(raw);
                if value.to_lowercase().contains(&needle) {
                    return Ok(true);
                }
            }
            Ok(false)
        })
        .map_err(|error| error.to_string())?;
        let transaction = conn.transaction().map_err(|error| error.to_string())?;
        let filter="FROM entry_tags et JOIN clipboard_history ch ON ch.id=et.entry_id WHERE et.tag=?1 AND (?2='' OR dc_tag_matches(ch.content,ch.preview,ch.source_app,COALESCE(ch.ocr_text,''),?2))";
        let total: usize = transaction
            .query_row(
                &format!("SELECT COUNT(*) {filter}"),
                params![tag, query],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let sql=format!("SELECT ch.id,ch.content_type,
            CASE WHEN ch.content_type='image' AND length(ch.content)<=8192 THEN ch.content ELSE '' END,
            ch.source_app,ch.timestamp,
            CASE WHEN ch.preview LIKE 'dpapi:%' OR ch.preview LIKE 'linux:%' THEN CASE WHEN length(ch.preview)<=131072 THEN ch.preview ELSE '' END ELSE substr(ch.preview,1,800) END,
            ch.is_pinned,ch.tags,ch.use_count,ch.is_external,ch.pinned_order,ch.source_app_path,ch.content_kinds,ch.ocr_status
            {filter} ORDER BY {order} LIMIT ?3 OFFSET ?4");
        let items = {
            let mut statement = transaction
                .prepare(&sql)
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map(params![tag, query, limit as i64, offset as i64], |row| {
                    let raw: String = row.get(2)?;
                    let image = self.maybe_decrypt_text(&raw);
                    let image = if image.len() <= 4096 && !image.starts_with("data:") {
                        image
                    } else {
                        String::new()
                    };
                    let preview = self
                        .maybe_decrypt_text(&row.get::<_, String>(5)?)
                        .chars()
                        .take(800)
                        .collect();
                    Ok(ClipboardEntry {
                        id: row.get(0)?,
                        content_type: row.get(1)?,
                        content: image,
                        html_content: None,
                        source_app: row.get(3)?,
                        timestamp: row.get(4)?,
                        preview,
                        is_pinned: row.get(6)?,
                        tags: self.maybe_decrypt_tags(
                            serde_json::from_str(&row.get::<_, String>(7)?).unwrap_or_default(),
                        ),
                        use_count: row.get(8)?,
                        is_external: row.get(9)?,
                        pinned_order: row.get(10)?,
                        source_app_path: row.get(11)?,
                        file_preview_exists: true,
                        content_kinds: serde_json::from_str(&row.get::<_, String>(12)?)
                            .unwrap_or_default(),
                        ocr_text: None,
                        ocr_status: row.get(13)?,
                    })
                })
                .map_err(|error| error.to_string())?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?
        };
        transaction.commit().map_err(|error| error.to_string())?;
        let next = offset + items.len();
        let has_more = next < total;
        Ok(TagItemsPage {
            items,
            total,
            has_more,
            next_offset: has_more.then_some(next),
        })
    }

    pub fn remove_preserving_entries(&self, name: &str) -> Result<(), String> {
        if crate::database::has_sensitive_tag(&[name.to_string()]) {
            return Err("system sensitive tags cannot be removed globally".into());
        }
        let mut conn = self.conn.lock().map_err(|error| error.to_string())?;
        let transaction = conn.transaction().map_err(|error| error.to_string())?;
        let ids = {
            let mut statement = transaction
                .prepare("SELECT entry_id FROM entry_tags WHERE tag=?1")
                .map_err(|error| error.to_string())?;
            let ids = statement
                .query_map([name], |row| row.get::<_, i64>(0))
                .map_err(|error| error.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?;
            ids
        };
        transaction
            .execute("DELETE FROM saved_tags WHERE name=?1", [name])
            .map_err(|error| error.to_string())?;
        transaction
            .execute("DELETE FROM entry_tags WHERE tag=?1", [name])
            .map_err(|error| error.to_string())?;
        for id in ids {
            Self::refresh_entry_tags_json(&transaction, id)?;
        }
        transaction.commit().map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::repository::{migrations::run_migrations, tag_repo::TagRepository};
    use rusqlite::Connection;
    use std::sync::{Arc, Mutex};
    fn repository() -> (SqliteTagRepository, Arc<Mutex<Connection>>) {
        let mut connection = Connection::open_in_memory().unwrap();
        run_migrations(&mut connection).unwrap();
        for id in 1..=145 {
            connection.execute("INSERT INTO clipboard_history(id,content_type,content,source_app,timestamp,preview,tags,use_count,html_content) VALUES(?1,'text',?2,'fixture',10,?3,'[\"work\",\"keep\"]',?1,'<p>full html</p>')",params![id,"x".repeat(50000),format!("entry {id} 100%_ 中文")]).unwrap();
            connection
                .execute(
                    "INSERT INTO entry_tags(entry_id,tag) VALUES(?1,'work'),(?1,'keep')",
                    [id],
                )
                .unwrap();
        }
        let connection = Arc::new(Mutex::new(connection));
        (SqliteTagRepository::new(connection.clone()), connection)
    }
    #[test]
    fn pages_are_bounded_ordered_and_do_not_transfer_payloads() {
        let (repository, _) = repository();
        let first = repository.get_page("work", "", "time", 60, 0).unwrap();
        let second = repository.get_page("work", "", "time", 60, 60).unwrap();
        assert_eq!(first.total, 145);
        assert_eq!(first.items.len(), 60);
        assert_eq!(first.next_offset, Some(60));
        assert_eq!(first.items[0].id, 145);
        assert_eq!(second.items[0].id, 85);
        assert!(first
            .items
            .iter()
            .all(|item| item.content.is_empty() && item.html_content.is_none()));
        assert!(repository
            .get_page("work", "100%_ 中文", "count", 100, 100)
            .unwrap()
            .items
            .iter()
            .all(|item| item.id <= 45));
        assert_eq!(
            repository
                .get_page("work", "no match", "time", 60, 0)
                .unwrap()
                .total,
            0
        );
        assert!(repository.get_page("work", "", "time", 101, 0).is_err());
        assert_eq!(
            repository
                .get_page("work' OR 1=1 --", "", "time", 60, 0)
                .unwrap()
                .total,
            0
        );
    }
    #[test]
    fn removing_tag_keeps_history_other_tags_and_original_content() {
        let (repository, connection) = repository();
        repository.remove_preserving_entries("work").unwrap();
        let connection = connection.lock().unwrap();
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM clipboard_history", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            145
        );
        assert_eq!(
            connection
                .query_row("SELECT tags FROM clipboard_history WHERE id=1", [], |row| {
                    row.get::<_, String>(0)
                })
                .unwrap(),
            "[\"keep\"]"
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT length(content) FROM clipboard_history WHERE id=1",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            50000
        );
        drop(connection);
        assert!(repository.remove_preserving_entries("sensitive").is_err());
        assert_eq!(repository.get_all_with_counts().unwrap()["keep"], 145);
    }
    #[test]
    fn failed_rename_keeps_both_indexes_and_saved_colors() {
        let (repository, connection) = repository();
        repository.create("work").unwrap();
        repository
            .set_color("work", Some("#123456".into()))
            .unwrap();
        connection.lock().unwrap().execute_batch("CREATE TRIGGER fail_tag_rename BEFORE UPDATE ON clipboard_history WHEN OLD.id=2 BEGIN SELECT RAISE(ABORT,'fixture failure');END;").unwrap();
        assert!(repository.rename("work", "renamed").is_err());
        assert_eq!(
            repository
                .get_page("work", "", "time", 60, 0)
                .unwrap()
                .total,
            145
        );
        assert_eq!(
            repository
                .get_page("renamed", "", "time", 60, 0)
                .unwrap()
                .total,
            0
        );
        assert_eq!(repository.get_colors().unwrap()["work"], "#123456");
        assert!(repository.rename("sensitive", "public").is_err());
    }
    #[test]
    fn global_rename_cannot_skip_sensitive_content_encryption() {
        let (repository, connection) = repository();
        for target in ["sensitive", "SENSITIVE", "密码"] {
            assert!(repository.rename("work", target).is_err());
            assert_eq!(repository.get_all_with_counts().unwrap()["work"], 145);
        }
        let connection = connection.lock().unwrap();
        let tags: String = connection
            .query_row("SELECT tags FROM clipboard_history WHERE id=1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(tags, "[\"work\",\"keep\"]");
    }
    #[test]
    fn failed_removal_rolls_back_the_entire_operation() {
        let (repository, connection) = repository();
        connection.lock().unwrap().execute_batch("CREATE TRIGGER fail_tag_update BEFORE UPDATE ON clipboard_history WHEN OLD.id=2 BEGIN SELECT RAISE(ABORT,'fixture failure');END;").unwrap();
        assert!(repository.remove_preserving_entries("work").is_err());
        assert_eq!(
            repository
                .get_page("work", "", "time", 60, 0)
                .unwrap()
                .total,
            145
        );
    }
}
