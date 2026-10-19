#[cfg(test)]
use rusqlite::Connection;
use tauri::State;

use crate::app_state::SearchHistory;
use crate::database::DbState;
use crate::domain::models::ClipboardEntry;
#[cfg(test)]
use crate::infrastructure::repository::clipboard_repo::SqliteClipboardRepository;
#[cfg(test)]
use crate::services::search::SearchMode;

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
    state: State<'_, DbState>,
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
        .history_service()
        .search_fts(trimmed, limit)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn search_fuzzy(
    state: State<'_, DbState>,
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
        .history_service()
        .search_fuzzy(&query, threshold, limit)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn search_regex(
    state: State<'_, DbState>,
    history: State<'_, SearchHistory>,
    pattern: String,
    limit: u32,
) -> Result<Vec<ClipboardEntry>, String> {
    if pattern.is_empty() {
        return Ok(Vec::new());
    }
    let result = state
        .history_service()
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
    use crate::infrastructure::repository::migrations::run_migrations;
    use std::sync::{Arc, Mutex};

    fn setup_test_db() -> (Arc<Mutex<Connection>>, SqliteClipboardRepository) {
        let mut conn = Connection::open_in_memory().expect("in-memory connection must open");
        run_migrations(&mut conn).expect("migrations failed");
        let arc = Arc::new(Mutex::new(conn));
        let repo = SqliteClipboardRepository::new(arc.clone());
        (arc, repo)
    }

    fn insert_entry(conn: &Connection, content: &str, app: &str, ts: i64) {
        let preview: String = content.chars().take(50).collect();
        conn.execute(
            "INSERT INTO clipboard_history
             (content_type, content, html_content, source_app, timestamp, preview,
              is_pinned, content_hash, tags, is_external, pinned_order, source_app_path)
             VALUES ('text', ?1, NULL, ?2, ?3, ?4, 0, 0, '[]', 0, 0, NULL)",
            rusqlite::params![content, app, ts, preview],
        )
        .expect("insert failed");
    }

    fn execute_plan(arc: &Arc<Mutex<Connection>>, mode: SearchMode, limit: u32) -> Vec<i64> {
        let history = dzc_core::api::HistoryService::from_legacy_connection(arc.clone());
        let entries = match mode {
            SearchMode::Fuzzy { pattern, threshold } => {
                history.search_fuzzy(&pattern, threshold, limit)
            }
            SearchMode::Regex { pattern } => history.search_regex(&pattern, limit),
            SearchMode::Fts5 { query } => history.search_fts(&query, limit),
        }
        .unwrap();
        entries.into_iter().map(|entry| entry.id).collect()
    }

    #[test]
    fn test_search_fts_basic() {
        let (arc, repo) = setup_test_db();
        {
            let conn = arc.lock().expect("lock");
            insert_entry(
                &conn,
                "alpha apple banana foo cherry",
                "App1",
                1_700_000_000,
            );
            insert_entry(&conn, "delta elephant falcon grape", "App2", 1_700_000_001);
            insert_entry(&conn, "hello world baz qux", "App3", 1_700_000_002);
        }
        let results = repo.search_fts("foo", 10).expect("search_fts failed");
        assert_eq!(
            results.len(),
            1,
            "expected exactly 1 entry containing 'foo'"
        );
        assert!(results[0].content.contains("foo"));
        assert!(results[0].content.contains("apple"));
    }

    #[test]
    fn test_search_fuzzy() {
        let (arc, _repo) = setup_test_db();
        {
            let conn = arc.lock().expect("lock");
            insert_entry(&conn, "hello world greeting message", "App1", 1_700_000_000);
            insert_entry(&conn, "completely unrelated text", "App2", 1_700_000_001);
        }
        let ids = execute_plan(
            &arc,
            SearchMode::Fuzzy {
                pattern: "hell".to_string(),
                threshold: 1,
            },
            10,
        );
        assert_eq!(
            ids.len(),
            1,
            "expected 1 entry matched by fuzzy prefix 'hell'"
        );
    }

    #[test]
    fn test_search_regex() {
        let (arc, _repo) = setup_test_db();
        {
            let conn = arc.lock().expect("lock");
            insert_entry(
                &conn,
                "contact user@example.com for details",
                "App1",
                1_700_000_000,
            );
            insert_entry(&conn, "no email here at all", "App2", 1_700_000_001);
        }
        let ids = execute_plan(
            &arc,
            SearchMode::Regex {
                pattern: r"\w+@\w+".to_string(),
            },
            10,
        );
        assert_eq!(ids.len(), 1, "expected 1 entry matched by regex");
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
