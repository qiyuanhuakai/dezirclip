use tauri::State;

use crate::app_state::SearchHistory;
use dzc_core::api::HistoryService;
use crate::domain::models::ClipboardEntry;

fn normalize_limit(limit: u32) -> u32 {
    if limit == 0 {
        1
    } else if limit > 500 {
        500
    } else {
        limit
    }
}

#[tauri::command]
pub async fn search_fts(
    state: State<'_, HistoryService>,
    history: State<'_, SearchHistory>,
    query: String,
    limit: u32,
) -> Result<Vec<ClipboardEntry>, String> {
    history.push(query.clone());
    let limit = normalize_limit(limit);
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    state

        .search_fts(trimmed, limit)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn search_fuzzy(
    state: State<'_, HistoryService>,
    history: State<'_, SearchHistory>,
    query: String,
    threshold: u8,
    limit: u32,
) -> Result<Vec<ClipboardEntry>, String> {
    history.push(query.clone());
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    let limit = normalize_limit(limit);
    state

        .search_fuzzy(&query, threshold, limit)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn search_regex(
    state: State<'_, HistoryService>,
    history: State<'_, SearchHistory>,
    pattern: String,
    limit: u32,
) -> Result<Vec<ClipboardEntry>, String> {
    if pattern.is_empty() {
        return Ok(Vec::new());
    }
    let result = state

        .search_regex(&pattern, normalize_limit(limit));
    if !matches!(&result, Err(error) if error.code==dzc_core::api::CoreErrorCode::Validation) {
        history.push(pattern);
    }
    result.map_err(Into::into)
}

#[tauri::command]
pub async fn get_search_history(history: State<'_, SearchHistory>) -> Result<Vec<String>, String> {
    Ok(history.snapshot())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(entries: &[(&str, &str)]) -> HistoryService {
        let history = HistoryService::open_desktop(std::path::Path::new(":memory:")).unwrap();
        history.set_desktop_setting("app.privacy_protection", "false").unwrap();
        for (index, &(content, app)) in entries.iter().enumerate() {
            let entry: ClipboardEntry = serde_json::from_value(serde_json::json!({
                "id": 0, "content_type": "text", "content": content, "source_app": app,
                "timestamp": 1_700_000_000 + index, "preview": content, "is_pinned": false
            })).unwrap();
            history.save(&entry, None).unwrap();
        }
        history
    }

    #[test]
    fn test_search_fts_basic() {
        let history = fixture(&[
            ("alpha apple banana foo cherry", "App1"),
            ("delta elephant falcon grape", "App2"),
            ("hello world baz qux", "App3"),
        ]);
        let results = history.search_fts("foo", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("foo"));
        assert!(results[0].content.contains("apple"));
    }

    #[test]
    fn test_search_fuzzy() {
        let history = fixture(&[
            ("hello world greeting message", "App1"),
            ("completely unrelated text", "App2"),
        ]);
        let results = history.search_fuzzy("hell", 1, 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].source_app, "App1");
    }

    #[test]
    fn test_search_regex() {
        let history = fixture(&[
            ("contact user@example.com for details", "App1"),
            ("no email here at all", "App2"),
        ]);
        let results = history.search_regex(r"\w+@\w+", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].source_app, "App1");
    }

    #[test]
    fn test_search_history() {
        let history = SearchHistory::default();
        history.push("hello".to_string());
        history.push("world".to_string());
        history.push("hello".to_string());
        let snapshot = history.snapshot();
        assert_eq!(snapshot.len(), 2, "dedup must collapse 'hello' duplicates");
        assert_eq!(snapshot[0], "hello", "most-recent query floats to front");
        assert_eq!(snapshot[1], "world");
    }
}
