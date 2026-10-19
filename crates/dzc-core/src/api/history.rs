//! History operations own their repository handles; clients never receive a connection or cache.
use super::{CoreError, CoreResult};
use crate::domain::models::ClipboardEntry;
use crate::infrastructure::repository::{
    cli_query::CliQuery,
    clipboard_repo::{ClipboardRepository, SqliteClipboardRepository},
    tag_repo::{SqliteTagRepository, TagRepository},
};
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistorySearchMode {
    Contains,
    Fuzzy,
    Regex,
    Fts5,
}
impl HistorySearchMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Contains => "contains",
            Self::Fuzzy => "fuzzy",
            Self::Regex => "regex",
            Self::Fts5 => "fts5",
        }
    }
}
impl TryFrom<&str> for HistorySearchMode {
    type Error = CoreError;
    fn try_from(value: &str) -> CoreResult<Self> {
        match value {
            "contains" => Ok(Self::Contains),
            "fuzzy" => Ok(Self::Fuzzy),
            "regex" => Ok(Self::Regex),
            "fts5" => Ok(Self::Fts5),
            _ => Err(CoreError::validation("unknown history search mode")),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct HistoryQuery<'a> {
    pub limit: i32,
    pub offset: i32,
    pub kind: Option<&'a str>,
    pub tag: Option<&'a str>,
    pub pinned: bool,
    pub metadata: bool,
    pub exclude_sensitive: bool,
    pub search: Option<(&'a str, HistorySearchMode)>,
    pub id: Option<i64>,
    pub latest: bool,
}
impl<'a> HistoryQuery<'a> {
    fn repository_query(&self) -> CliQuery<'a> {
        CliQuery {
            limit: self.limit,
            offset: self.offset,
            kind: self.kind,
            tag: self.tag,
            pinned: self.pinned,
            metadata: self.metadata,
            exclude_sensitive: self.exclude_sensitive,
            search: self.search.map(|(query, mode)| (query, mode.as_str())),
            id: self.id,
            latest: self.latest,
        }
    }
}

#[derive(Clone)]
pub struct HistoryService {
    pub(super) repository: Arc<SqliteClipboardRepository>,
    tags: Arc<SqliteTagRepository>,
}

impl HistoryService {
    pub fn open(path: &Path) -> CoreResult<Self> {
        if path.as_os_str().is_empty() {
            return Err(CoreError::validation("database path is empty"));
        }
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)
                .map_err(|error| CoreError::from(format!("mkdir {parent:?}: {error}")))?;
        }
        let mut conn = rusqlite::Connection::open(path)
            .map_err(|error| CoreError::from(format!("open {path:?}: {error}")))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|error| CoreError::from(error.to_string()))?;
        crate::infrastructure::repository::migrations::run_migrations(&mut conn)
            .map_err(|error| CoreError::from(format!("migrations: {error}")))?;
        let shared = Arc::new(Mutex::new(conn));
        Ok(Self {
            repository: Arc::new(SqliteClipboardRepository::new(shared.clone())),
            tags: Arc::new(SqliteTagRepository::new(shared)),
        })
    }

    pub fn query(&self, query: &HistoryQuery<'_>) -> CoreResult<Vec<ClipboardEntry>> {
        self.repository
            .query_cli(&query.repository_query())
            .map_err(Into::into)
    }
    pub fn query_for_export(&self, query: &HistoryQuery<'_>) -> CoreResult<Vec<ClipboardEntry>> {
        self.repository
            .query_cli_for_export(&query.repository_query())
            .map_err(Into::into)
    }
    pub fn automation(&self) -> AutomationHistory {
        AutomationHistory(self.clone())
    }
    pub fn get_history(
        &self,
        limit: i32,
        offset: i32,
        kind: Option<&str>,
    ) -> CoreResult<Vec<ClipboardEntry>> {
        self.repository
            .get_history(limit, offset, kind)
            .map_err(Into::into)
    }
    pub fn get_entry_by_id(&self, id: i64) -> CoreResult<Option<ClipboardEntry>> {
        self.repository.get_entry_by_id(id).map_err(Into::into)
    }
    pub fn search(&self, query: &str, limit: i32) -> CoreResult<Vec<ClipboardEntry>> {
        self.repository.search(query, limit).map_err(Into::into)
    }
    pub fn search_fts(&self, query: &str, limit: u32) -> CoreResult<Vec<ClipboardEntry>> {
        self.repository.search_fts(query, limit).map_err(Into::into)
    }
    pub fn search_fuzzy(
        &self,
        query: &str,
        threshold: u8,
        limit: u32,
    ) -> CoreResult<Vec<ClipboardEntry>> {
        self.repository
            .search_desktop(
                crate::services::search::SearchMode::Fuzzy {
                    pattern: query.to_owned(),
                    threshold,
                },
                limit,
            )
            .map_err(Into::into)
    }
    pub fn search_regex(&self, pattern: &str, limit: u32) -> CoreResult<Vec<ClipboardEntry>> {
        if regex::Regex::new(pattern).is_err() {
            return Err(CoreError::validation(format!("Invalid regex: {pattern}")));
        }
        self.repository
            .search_desktop(
                crate::services::search::SearchMode::Regex {
                    pattern: pattern.to_owned(),
                },
                limit,
            )
            .map_err(Into::into)
    }
    pub fn get_entries_by_tag(&self, tag: &str) -> CoreResult<Vec<ClipboardEntry>> {
        self.tags.get_entries_by_tag(tag).map_err(Into::into)
    }
    pub fn tag_counts(&self) -> CoreResult<std::collections::HashMap<String, i32>> {
        self.tags.get_all_with_counts().map_err(Into::into)
    }
    pub fn tag_page(
        &self,
        tag: &str,
        query: &str,
        sort: &str,
        limit: usize,
        offset: usize,
    ) -> CoreResult<super::TagItemsPage> {
        self.tags
            .get_page(tag, query, sort, limit, offset)
            .map_err(Into::into)
    }
    pub fn create_tag(&self, name: &str) -> CoreResult<()> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
            return Err(CoreError::validation(
                "tag names must contain 1..80 printable characters",
            ));
        }
        self.tags.create(name).map_err(Into::into)
    }
    pub fn rename_tag(&self, old: &str, new: &str) -> CoreResult<()> {
        self.tags.rename(old, new).map_err(CoreError::from)?;
        self.repository.invalidate_caches();
        Ok(())
    }
    pub fn remove_tag_preserving_entries(&self, name: &str) -> CoreResult<()> {
        self.tags
            .remove_preserving_entries(name)
            .map_err(CoreError::from)?;
        self.repository.invalidate_caches();
        Ok(())
    }
    pub fn delete_tag_with_entries(
        &self,
        name: &str,
        data_dir: Option<&Path>,
    ) -> CoreResult<super::TagDeletionReport> {
        let report = self
            .tags
            .delete_globally(name, data_dir)
            .map_err(CoreError::from)?;
        self.repository.invalidate_caches();
        Ok(report)
    }
    pub fn delete_tag_entries(
        &self,
        name: &str,
        selection: super::TagEntrySelection,
        data_dir: Option<&Path>,
    ) -> CoreResult<super::TagDeletionReport> {
        let report = self
            .tags
            .delete_entries_for_tag(name, selection, data_dir)
            .map_err(CoreError::from)?;
        self.repository.invalidate_caches();
        Ok(report)
    }
    pub fn get_entry_content(&self, id: i64) -> CoreResult<Option<String>> {
        self.repository.get_entry_content(id).map_err(Into::into)
    }
    pub fn update_pinned_order(&self, orders: Vec<(i64, i64)>) -> CoreResult<()> {
        self.repository
            .update_pinned_order(orders)
            .map_err(Into::into)
    }
    pub fn get_count(&self) -> CoreResult<i64> {
        self.repository.get_count().map_err(Into::into)
    }
    pub fn save(&self, entry: &ClipboardEntry, data_dir: Option<&Path>) -> CoreResult<i64> {
        self.repository.save(entry, data_dir).map_err(Into::into)
    }
    pub fn update_entry_content(&self, id: i64, content: &str, preview: &str) -> CoreResult<()> {
        self.repository
            .update_entry_content(id, content, preview)
            .map_err(Into::into)
    }
    pub fn update_entry_tags(&self, id: i64, tags: Vec<String>) -> CoreResult<()> {
        self.tags
            .update_entry_tags(id, tags)
            .map_err(CoreError::from)?;
        self.repository.invalidate_caches();
        Ok(())
    }
    pub fn toggle_pin(&self, id: i64, pinned: bool) -> CoreResult<()> {
        self.repository.toggle_pin(id, pinned).map_err(Into::into)
    }
    pub fn delete(&self, id: i64, data_dir: Option<&Path>) -> CoreResult<()> {
        self.repository.delete(id, data_dir).map_err(Into::into)
    }
    pub fn clear(&self, data_dir: Option<&Path>) -> CoreResult<()> {
        self.repository.clear(data_dir).map_err(Into::into)
    }
    pub fn increment_use_count(&self, id: i64) -> CoreResult<()> {
        self.repository.increment_use_count(id).map_err(Into::into)
    }
    pub fn ensure_export_destination(&self, target: &Path) -> CoreResult<()> {
        self.repository
            .ensure_export_destination(target)
            .map_err(Into::into)
    }

    pub fn restore(&self, entries: &[ClipboardEntry], mode: super::ImportMode) -> CoreResult<()> {
        self.repository
            .import_cli_entries(entries, mode == super::ImportMode::Replace)
            .map_err(Into::into)
    }

    #[cfg(feature = "legacy-bridge")]
    #[doc(hidden)]
    pub fn from_legacy_connection(shared: Arc<Mutex<rusqlite::Connection>>) -> Self {
        Self {
            repository: Arc::new(SqliteClipboardRepository::new(shared.clone())),
            tags: Arc::new(SqliteTagRepository::new(shared)),
        }
    }

    #[cfg(feature = "legacy-bridge")]
    #[doc(hidden)]
    pub fn from_legacy_parts(
        repository: Arc<SqliteClipboardRepository>,
        tags: Arc<SqliteTagRepository>,
    ) -> Self {
        Self { repository, tags }
    }
}

/// Automation has only bounded read operations and always applies privacy filtering before decoding.
#[derive(Clone)]
pub struct AutomationHistory(HistoryService);
impl AutomationHistory {
    pub fn query(&self, query: &HistoryQuery<'_>) -> CoreResult<Vec<ClipboardEntry>> {
        if !(1..=1000).contains(&query.limit) || query.offset < 0 {
            return Err(CoreError::validation(
                "automation history query must use a bounded page",
            ));
        }
        let mut query = query.clone();
        query.exclude_sensitive = true;
        self.0.query(&query)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_history_and_automation_share_storage_without_sharing_a_connection_api() {
        let directory =
            std::env::temp_dir().join(format!("dzc-history-service-{}", uuid::Uuid::new_v4()));
        let history = HistoryService::open(&directory.join("history.db")).unwrap();
        let mut entry: ClipboardEntry = serde_json::from_value(serde_json::json!({"id":0,"content_type":"text","content":"original body","preview":"original","source_app":"test","timestamp":1,"is_pinned":false})).unwrap();
        let id = history.save(&entry, None).unwrap();
        let other = HistoryService::open(&directory.join("history.db")).unwrap();
        assert_eq!(
            other.get_entry_by_id(id).unwrap().unwrap().content,
            "original body"
        );
        history
            .update_entry_content(id, "updated body", "updated")
            .unwrap();
        assert_eq!(
            other
                .query(&HistoryQuery {
                    limit: 10,
                    search: Some(("updated", HistorySearchMode::Contains)),
                    ..Default::default()
                })
                .unwrap()
                .len(),
            1
        );
        assert_eq!(history.search_fuzzy("updated", 1, 10).unwrap().len(), 1);
        assert_eq!(history.search_regex("updated.*body", 10).unwrap().len(), 1);
        assert!(history.search_regex("[", 10).is_err());
        assert_eq!(
            history.create_tag("  ").unwrap_err().code,
            super::super::CoreErrorCode::Validation
        );
        history.create_tag(" work ").unwrap();
        history
            .update_entry_tags(id, vec!["work".to_owned()])
            .unwrap();
        assert_eq!(
            other
                .tag_page("work", "updated", "time", 10, 0)
                .unwrap()
                .total,
            1
        );
        let (backup, count) = super::super::BackupService::new(history.clone())
            .export_bytes(None)
            .unwrap();
        assert_eq!(count, 1);
        let restored = HistoryService::open(Path::new(":memory:")).unwrap();
        super::super::BackupService::new(restored.clone())
            .import_bytes(&backup, None, super::super::ImportMode::Merge)
            .unwrap();
        assert_eq!(
            restored.get_entry_by_id(id).unwrap().unwrap().content,
            "updated body"
        );
        entry.id = id;
        entry.tags = vec!["sensitive".to_string()];
        // The public facade preserves the repository's fail-closed result rather than downgrading it.
        if let Err(error) = history.save(&entry, None) {
            assert!(matches!(
                error.code,
                super::super::CoreErrorCode::KeyUnavailable
                    | super::super::CoreErrorCode::SensitivePersistenceRejected
            ));
        } else {
            assert!(history
                .automation()
                .query(&HistoryQuery {
                    limit: 10,
                    ..Default::default()
                })
                .unwrap()
                .is_empty());
        }
        drop(other);
        drop(history);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
