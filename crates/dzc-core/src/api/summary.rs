//! Owned, restricted values for future clients. This is an embedded Core API,
//! not an IPC endpoint, authentication grant or mutable privacy-state snapshot.
//! Only the restricted query methods enforce this fixed visibility scope. DTO
//! construction does not authorize callers, and the existing complete
//! HistoryService remains a trusted embedded interface, unsuitable for direct
//! exposure to a future Surface without Host authentication and authorization.
use super::{CoreError, CoreResult, HistoryQuery, HistorySearchMode, HistoryService};
use crate::domain::models::ClipboardEntry;
use dzc_protocol::history as wire;
use serde::{Deserialize, Serialize};

const MAX_LIMIT: u16 = 1000;
const MAX_QUERY_BYTES: usize = 16 * 1024;
const PREVIEW_CHARACTERS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SummarySearchMode { Contains, Fuzzy, Regex, Fts5 }

impl SummarySearchMode {
    fn history_mode(self) -> HistorySearchMode {
        match self {
            Self::Contains => HistorySearchMode::Contains,
            Self::Fuzzy => HistorySearchMode::Fuzzy,
            Self::Regex => HistorySearchMode::Regex,
            Self::Fts5 => HistorySearchMode::Fts5,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SummaryQuery {
    pub limit: u16,
    pub offset: u32,
    pub kind: Option<String>,
    pub tag: Option<String>,
    pub pinned: bool,
}

impl Default for SummaryQuery {
    fn default() -> Self {
        Self { limit: 50, offset: 0, kind: None, tag: None, pinned: false }
    }
}

impl SummaryQuery {
    pub(super) fn validate(&self) -> CoreResult<()> {
        if !(1..=MAX_LIMIT).contains(&self.limit) || self.offset > i32::MAX as u32 {
            return Err(CoreError::validation("summary query requires a bounded page"));
        }
        if self.kind.iter().chain(self.tag.iter()).any(|value| value.len() > MAX_QUERY_BYTES) {
            return Err(CoreError::validation("summary filter exceeds its byte budget"));
        }
        Ok(())
    }

    fn history_query(&self) -> HistoryQuery<'_> {
        HistoryQuery {
            limit: i32::from(self.limit),
            offset: self.offset as i32,
            kind: self.kind.as_deref(),
            tag: self.tag.as_deref(),
            pinned: self.pinned,
            metadata: true,
            exclude_sensitive: true,
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    pub mode: SummarySearchMode,
    pub page: SummaryQuery,
}

impl SearchRequest {
    fn validate(&self) -> CoreResult<()> {
        self.page.validate()?;
        if self.query.len() > MAX_QUERY_BYTES {
            return Err(CoreError::validation("summary search query exceeds its byte budget"));
        }
        if self.mode == SummarySearchMode::Regex && regex::Regex::new(&self.query).is_err() {
            return Err(CoreError::validation("invalid summary regular expression"));
        }
        Ok(())
    }
}

/// Returned by restricted Core queries after the automation visibility filter.
/// The value itself grants no access. Resource-bearing entries have no preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntrySummary {
    pub id: i64,
    pub kind: String,
    pub preview: Option<String>,
    pub tags: Vec<String>,
    pub pinned: bool,
    pub timestamp: i64,
    pub source_app: String,
}

pub(super) fn summarize(entry: ClipboardEntry) -> EntrySummary {
    let preview_prefix = entry.preview.trim_start().to_ascii_lowercase();
    let preview = if matches!(entry.content_type.as_str(), "text" | "code")
        && !entry.is_external && entry.html_content.is_none()
        && !preview_prefix.starts_with("data:") && !preview_prefix.starts_with("file:")
        && !preview_prefix.starts_with("asset:")
        && !(entry.preview.contains('<') && entry.preview.contains('>'))
    {
        Some(entry.preview.chars().take(PREVIEW_CHARACTERS).collect())
    } else {
        None
    };
    EntrySummary {
        id: entry.id,
        kind: entry.content_type,
        preview,
        tags: entry.tags,
        pinned: entry.is_pinned,
        timestamp: entry.timestamp,
        source_app: if entry.source_app.len() > 256
            || entry.source_app.contains('/') || entry.source_app.contains('\\')
            || entry.source_app.trim_start().to_ascii_lowercase().starts_with("data:")
        {
            String::new()
        } else {
            entry.source_app
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResult {
    pub entries: Vec<EntrySummary>,
}

impl HistoryService {
    /// This restricted view cannot opt into sensitive records or complete bodies.
    /// Future IPC hosts must additionally authenticate and authorize each caller.
    pub fn summaries(&self, query: &SummaryQuery) -> CoreResult<SearchResult> {
        query.validate()?;
        self.automation().query(&query.history_query()).map(|entries| SearchResult {
            entries: entries.into_iter().map(summarize).collect(),
        })
    }

    pub fn search_summaries(&self, request: &SearchRequest) -> CoreResult<SearchResult> {
        request.validate()?;
        let mut query = request.page.history_query();
        query.search = Some((&request.query, request.mode.history_mode()));
        self.automation().query(&query).map(|entries| SearchResult {
            entries: entries.into_iter().map(summarize).collect(),
        })
    }
}

impl From<SummaryQuery> for wire::SummaryQuery {
    fn from(value: SummaryQuery) -> Self {
        Self { protocol: dzc_protocol::PROTOCOL_VERSION, limit: value.limit,
            offset: value.offset, kind: value.kind, tag: value.tag, pinned: value.pinned }
    }
}

impl TryFrom<wire::SummaryQuery> for SummaryQuery {
    type Error = wire::DzcError;
    fn try_from(value: wire::SummaryQuery) -> Result<Self, Self::Error> {
        if !value.protocol.is_compatible_with(dzc_protocol::PROTOCOL_VERSION) {
            return Err(wire::DzcError { code: wire::DzcErrorCode::IncompatibleVersion });
        }
        let result = Self { limit: value.limit, offset: value.offset,
            kind: value.kind, tag: value.tag, pinned: value.pinned };
        result.validate().map_err(|error| error.to_protocol())?;
        Ok(result)
    }
}

impl From<SearchRequest> for wire::SearchRequest {
    fn from(value: SearchRequest) -> Self {
        Self { query: value.query, page: value.page.into(), mode: match value.mode {
            SummarySearchMode::Contains => wire::SearchMode::Contains,
            SummarySearchMode::Fuzzy => wire::SearchMode::Fuzzy,
            SummarySearchMode::Regex => wire::SearchMode::Regex,
            SummarySearchMode::Fts5 => wire::SearchMode::Fts5,
        } }
    }
}

impl TryFrom<wire::SearchRequest> for SearchRequest {
    type Error = wire::DzcError;
    fn try_from(value: wire::SearchRequest) -> Result<Self, Self::Error> {
        let result = Self { query: value.query, page: value.page.try_into()?,
            mode: match value.mode {
                wire::SearchMode::Contains => SummarySearchMode::Contains,
                wire::SearchMode::Fuzzy => SummarySearchMode::Fuzzy,
                wire::SearchMode::Regex => SummarySearchMode::Regex,
                wire::SearchMode::Fts5 => SummarySearchMode::Fts5,
            } };
        result.validate().map_err(|error| error.to_protocol())?;
        Ok(result)
    }
}

impl From<EntrySummary> for wire::EntrySummary {
    fn from(value: EntrySummary) -> Self {
        Self { id: value.id, kind: value.kind, preview: value.preview,
            tags: value.tags, pinned: value.pinned, timestamp: value.timestamp,
            source_app: value.source_app, privacy_visibility: wire::SummaryVisibility::Ordinary }
    }
}

impl From<SearchResult> for wire::SearchResult {
    fn from(value: SearchResult) -> Self {
        Self { protocol: dzc_protocol::PROTOCOL_VERSION,
            entries: value.entries.into_iter().map(Into::into).collect() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use rusqlite::params;
    use std::path::Path;

    fn entry(content_type: &str, preview: &str) -> ClipboardEntry {
        serde_json::from_value(serde_json::json!({"id":0,"content_type":content_type,
            "content":"body never belongs to the summary","preview":preview,
            "source_app":"Synthetic app","source_app_path":"/private/application.exe",
            "timestamp":1,"is_pinned":false})).unwrap()
    }

    #[test]
    fn summaries_are_bounded_owned_values_without_resource_bodies_or_native_paths() {
        let history = HistoryService::open(Path::new(":memory:")).unwrap();
        let ordinary = history.save(&entry("text", &"界".repeat(1024 * 1024)), None).unwrap();
        for kind in ["image", "file", "video"] {
            history.save(&entry(kind, "file:///private/attachment.png data:image/png;base64,SECRET"), None).unwrap();
        }
        let mut html = entry("text", "<img src='file:///private/image.png'>HTML_PRIVATE");
        html.html_content = Some("<b>HTML_PRIVATE</b>".to_owned());
        history.save(&html, None).unwrap();
        let summaries = history.summaries(&SummaryQuery::default()).unwrap();
        assert_eq!(summaries.entries.len(), 5);
        for value in &summaries.entries {
            if value.id == ordinary { assert_eq!(value.preview.as_ref().unwrap().chars().count(), PREVIEW_CHARACTERS); }
            else { assert_eq!(value.preview, None); }
        }
        let encoded = serde_json::to_value(wire::SearchResult::from(summaries)).unwrap();
        let text = encoded.to_string();
        for forbidden in ["body never", "HTML_PRIVATE", "attachment.png", "data:image", "/private", "html_content", "ocr_text", "source_app_path", "history_revision", "privacy_epoch"] {
            assert!(!text.contains(forbidden), "unexpected {forbidden}");
        }
        assert_eq!(encoded["protocol"], serde_json::json!({"major":1,"minor":0}));
    }

    #[test]
    fn real_storage_filters_protected_or_structurally_damaged_rows_before_summary_pagination() {
        let directory = std::env::temp_dir().join(format!("dzc-summary-{}", uuid::Uuid::new_v4()));
        let path = directory.join("history.db");
        let history = HistoryService::open(&path).unwrap();
        for i in 0..8 {
            let mut e = entry("text", &format!("ordinary-{i}"));
            e.content = format!("ordinary-{i}");
            history.save(&e, None).unwrap();
        }
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            crate::infrastructure::repository::migrations::register_privacy_functions(&conn).unwrap();
            let protected = format!("linux:{}", base64::engine::general_purpose::STANDARD.encode([0u8; 28]));
            conn.execute("UPDATE clipboard_history SET tags='[\"  SeNsItIvE  \"]' WHERE id=2", []).unwrap();
            conn.execute("UPDATE clipboard_history SET content=?1 WHERE id=3", params![protected]).unwrap();
            conn.execute("UPDATE clipboard_history SET preview=?1 WHERE id=4", params![protected]).unwrap();
            conn.execute("UPDATE clipboard_history SET html_content=?1 WHERE id=5", params![protected]).unwrap();
            conn.execute("UPDATE clipboard_history SET ocr_text=?1 WHERE id=6", params![protected]).unwrap();
            conn.execute("UPDATE clipboard_history SET tags='{' WHERE id=8", []).unwrap();
        }
        let page = history.summaries(&SummaryQuery { limit: 1, ..Default::default() }).unwrap();
        assert_eq!(page.entries.len(), 1);
        assert_eq!(page.entries[0].id, 7);
        let next = history.summaries(&SummaryQuery { offset: 1, ..Default::default() }).unwrap();
        assert_eq!(next.entries.iter().map(|e| e.id).collect::<Vec<_>>(), vec![1]);
        for mode in [SummarySearchMode::Contains, SummarySearchMode::Fuzzy, SummarySearchMode::Regex, SummarySearchMode::Fts5] {
            let result = history.search_summaries(&SearchRequest { query: "ordinary".to_owned(), mode, page: SummaryQuery::default() }).unwrap();
            assert_eq!(result.entries.iter().map(|e| e.id).collect::<Vec<_>>(), vec![7, 1]);
        }
        let query: SummaryQuery = serde_json::from_value(serde_json::json!({"limit":1000,"offset":0,"kind":null,"tag":null,"pinned":false,"exclude_sensitive":false,"include_body":true})).unwrap();
        assert_eq!(history.summaries(&query).unwrap().entries.len(), 2);
        drop(history);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn conversions_validate_versions_pages_search_budget_and_preserve_all_modes() {
        for mode in [SummarySearchMode::Contains, SummarySearchMode::Fuzzy, SummarySearchMode::Regex, SummarySearchMode::Fts5] {
            let request = SearchRequest { query: "owned query".to_owned(), mode, page: SummaryQuery::default() };
            let wire = wire::SearchRequest::from(request.clone());
            let decoded = serde_json::from_slice::<wire::SearchRequest>(&serde_json::to_vec(&wire).unwrap()).unwrap();
            assert_eq!(SearchRequest::try_from(decoded).unwrap(), request);
        }
        let mut wrong = wire::SummaryQuery::default(); wrong.protocol.major += 1;
        assert_eq!(SummaryQuery::try_from(wrong).unwrap_err().code, wire::DzcErrorCode::IncompatibleVersion);
        for (limit, offset) in [(0, 0), (1001, 0), (1, u32::MAX)] {
            let query = wire::SummaryQuery { limit, offset, ..Default::default() };
            assert_eq!(SummaryQuery::try_from(query).unwrap_err().code, wire::DzcErrorCode::InvalidRequest);
        }
        let large = wire::SearchRequest { query: "x".repeat(MAX_QUERY_BYTES + 1), mode: wire::SearchMode::Contains, page: wire::SummaryQuery::default() };
        assert_eq!(SearchRequest::try_from(large).unwrap_err().code, wire::DzcErrorCode::InvalidRequest);
        for tag_filter in [false, true] {
            let mut query = wire::SummaryQuery::default();
            if tag_filter { query.tag = Some("x".repeat(MAX_QUERY_BYTES + 1)); }
            else { query.kind = Some("x".repeat(MAX_QUERY_BYTES + 1)); }
            assert_eq!(SummaryQuery::try_from(query).unwrap_err().code, wire::DzcErrorCode::InvalidRequest);
        }
        let internal = CoreError::from("open C:/private/history.db: SQL_PRIVATE".to_owned());
        assert_eq!(serde_json::to_value(internal.to_protocol()).unwrap(), serde_json::json!({"code":"storage_failed"}));
        assert_eq!(serde_json::to_value(CoreError::validation("USER_QUERY_PRIVATE").to_protocol()).unwrap(), serde_json::json!({"code":"invalid_request"}));
    }

    #[test]
    fn resource_like_previews_and_native_source_paths_are_not_summary_values() {
        for preview in [" data:image/png;base64,PRIVATE", "file:///private/image.png", "asset://localhost/private", "text <img src='private'>"] {
            assert_eq!(summarize(entry("text", preview)).preview, None);
        }
        let mut external = entry("text", "/private/image.png");
        external.is_external = true;
        assert_eq!(summarize(external).preview, None);
        for source in ["C:\\private\\application.exe", "/private/application", "data:image/png;base64,PRIVATE"] {
            let mut value = entry("text", "ordinary");
            value.source_app = source.to_owned();
            assert_eq!(summarize(value).source_app, "");
        }
        let request = SearchRequest { query: "[".to_owned(), mode: SummarySearchMode::Regex, page: SummaryQuery::default() };
        assert_eq!(request.validate().unwrap_err().to_protocol().code, wire::DzcErrorCode::InvalidRequest);
    }
}
