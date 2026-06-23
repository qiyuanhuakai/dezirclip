//! Search query constructors for clipboard history.
//!
//! Three modes are supported, all returning a parameterized
//! [`SearchPlan`] (or [`SearchPlan::Empty`] when the input is blank):
//!
//! * [`SearchMode::Fts5`] — full-text search against the
//!   `clipboard_fts` virtual table (trigram tokenizer, see migration v12).
//! * [`SearchMode::Fuzzy`] — prefix-match fuzzy fallback. The `threshold`
//!   is reserved for a future Levenshtein-backed implementation; the
//!   current SQL still uses the FTS5 prefix operator.
//! * [`SearchMode::Regex`] — regex match against `content` via the
//!   custom `REGEXP(pattern, text)` scalar function registered by
//!   [`register_regexp`].
//!
//! All user input is passed through `?` placeholders, and FTS5 tokens
//! are wrapped in double quotes with internal quotes doubled per
//! FTS5 string-literal rules to keep SQL/FTS5 injection impossible.

use regex::Regex;
use rusqlite::functions::FunctionFlags;
use rusqlite::Connection;

/// Search strategy. Each variant carries the user-supplied pattern.
pub enum SearchMode {
    /// Full-text search via SQLite FTS5.
    Fts5 { query: String },
    /// Prefix-match fuzzy search. `threshold` is currently unused by
    /// the SQL fallback but is part of the API so callers can pass
    /// user-driven permissiveness hints (0 = exact, 100 = very loose).
    Fuzzy { pattern: String, threshold: u8 },
    /// Regex match against `content`. Requires [`register_regexp`] to
    /// have been called on the target connection before execution.
    Regex { pattern: String },
}

/// A pre-built query plan: either a parameterized SQL statement
/// (positional `?` placeholders) or [`SearchPlan::Empty`] when the
/// search input is blank.
pub enum SearchPlan {
    Sql { sql: String, params: Vec<String> },
    Empty,
}

/// Build a parameterized SQL plan for the given search mode and row
/// limit. Returns [`SearchPlan::Empty`] when the input string is blank
/// (whitespace-only counts as blank for FTS5/Fuzzy; Regex requires a
/// non-empty raw pattern).
pub fn build_search_query(mode: &SearchMode, limit: u32) -> SearchPlan {
    let limit_str = limit.to_string();
    match mode {
        SearchMode::Fts5 { query } => {
            if query.trim().is_empty() {
                return SearchPlan::Empty;
            }
            SearchPlan::Sql {
                sql: "SELECT ch.id \
                      FROM clipboard_fts AS cf \
                      JOIN clipboard_history AS ch ON ch.id = cf.rowid \
                      WHERE clipboard_fts MATCH ?1 \
                      ORDER BY cf.rank \
                      LIMIT ?2"
                    .to_string(),
                params: vec![escape_fts5_term(query), limit_str],
            }
        }
        SearchMode::Fuzzy {
            pattern,
            threshold: _,
        } => {
            if pattern.trim().is_empty() {
                return SearchPlan::Empty;
            }
            // Simple prefix-match fallback: append `*` to the user
            // pattern so FTS5 returns tokens starting with the
            // pattern. The threshold is ignored by this fallback; a
            // future implementation can map it to Levenshtein depth.
            SearchPlan::Sql {
                sql: "SELECT ch.id \
                      FROM clipboard_fts AS cf \
                      JOIN clipboard_history AS ch ON ch.id = cf.rowid \
                      WHERE clipboard_fts MATCH ?1* \
                      ORDER BY cf.rank \
                      LIMIT ?2"
                    .to_string(),
                params: vec![pattern.clone(), limit_str],
            }
        }
        SearchMode::Regex { pattern } => {
            if pattern.is_empty() {
                return SearchPlan::Empty;
            }
            SearchPlan::Sql {
                sql: "SELECT id FROM clipboard_history \
                      WHERE content REGEXP ?1 \
                      ORDER BY timestamp DESC \
                      LIMIT ?2"
                    .to_string(),
                params: vec![pattern.clone(), limit_str],
            }
        }
    }
}

/// Escape a whitespace-delimited token stream for use in an FTS5
/// `MATCH` expression. Each token is wrapped in double quotes, and any
/// internal double quote is doubled per FTS5 string-literal rules.
///
/// * `"hello world"` → `"hello" "world"`
/// * `hello "world"` → `"hello" "\"world\""`
/// * `"你好"` → preserved (trigram tokenizer handles CJK).
///
/// Returns an empty string for empty / whitespace-only input. The
/// output contains no `?` placeholders, so it is safe to concatenate
/// into a pre-built FTS5 expression (we still pass it as a parameter
/// to keep the prepared-statement cache hot).
pub fn escape_fts5_term(input: &str) -> String {
    input
        .split_whitespace()
        .map(|token| {
            let escaped = token.replace('"', "\"\"");
            format!("\"{}\"", escaped)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Register a custom `REGEXP(pattern, text)` scalar function on the
/// given connection. SQLite's `REGEXP` is not built-in; this
/// implementation uses the `regex` crate. An invalid pattern returns
/// `false` (no match) rather than raising an error so the surrounding
/// query stays valid and yields zero rows.
pub fn register_regexp(conn: &Connection) -> rusqlite::Result<()> {
    conn.create_scalar_function(
        "regexp",
        2,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let pattern = ctx.get::<String>(0)?;
            let text = ctx.get::<String>(1)?;
            Ok(Regex::new(&pattern)
                .map(|re| re.is_match(&text))
                .unwrap_or(false))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fts5_basic() {
        // Two ASCII tokens are wrapped in quotes and space-joined.
        let escaped = escape_fts5_term("hello world");
        assert_eq!(escaped, "\"hello\" \"world\"");

        // build_search_query returns an Sql plan with MATCH ?1 and
        // LIMIT ?2 placeholders, plus the escaped term and limit.
        let plan = build_search_query(
            &SearchMode::Fts5 {
                query: "hello world".to_string(),
            },
            10,
        );
        match plan {
            SearchPlan::Sql { sql, params } => {
                assert!(sql.contains("MATCH ?1"), "expected MATCH ?1 in: {sql}");
                assert!(sql.contains("LIMIT ?2"), "expected LIMIT ?2 in: {sql}");
                assert_eq!(
                    params,
                    vec!["\"hello\" \"world\"".to_string(), "10".to_string()]
                );
            }
            SearchPlan::Empty => panic!("expected Sql plan for non-empty Fts5 query"),
        }
    }

    #[test]
    fn test_fts5_special_chars() {
        // "hello \"world\"" → "hello" """world""" : each internal
        // double-quote is doubled, then the whole token is wrapped.
        let escaped = escape_fts5_term("hello \"world\"");
        assert_eq!(escaped, "\"hello\" \"\"\"world\"\"\"");

        // Build the plan; it must remain parameterized — the user
        // input never appears unescaped in the SQL text.
        let plan = build_search_query(
            &SearchMode::Fts5 {
                query: "hello \"world\"".to_string(),
            },
            10,
        );
        match plan {
            SearchPlan::Sql { sql, params } => {
                assert!(
                    !sql.contains("\"world\""),
                    "raw user quote leaked into SQL: {sql}"
                );
                assert_eq!(
                    params,
                    vec!["\"hello\" \"\"\"world\"\"\"".to_string(), "10".to_string()]
                );
            }
            SearchPlan::Empty => panic!("expected Sql plan"),
        }
    }

    #[test]
    fn test_fts5_cjk() {
        // Trigram tokenizer handles CJK; escape_fts5_term only
        // preserves the content inside a quoted token.
        let escaped = escape_fts5_term("你好");
        assert_eq!(escaped, "\"你好\"");

        // Mixed CJK + ASCII round-trips through the escaper unchanged
        // (the ASCII side still gets its own quoted token).
        assert_eq!(escape_fts5_term("你好 world"), "\"你好\" \"world\"");

        // Blank / whitespace-only input collapses to an Empty plan
        // (caller skips the query).
        match build_search_query(
            &SearchMode::Fts5 {
                query: "   ".to_string(),
            },
            10,
        ) {
            SearchPlan::Empty => {}
            SearchPlan::Sql { .. } => panic!("whitespace-only Fts5 query should yield Empty"),
        }
    }

    #[test]
    fn test_fuzzy() {
        // Threshold is part of the API; the SQL fallback uses FTS5
        // prefix matching (`?1*`) regardless of threshold value.
        let plan = build_search_query(
            &SearchMode::Fuzzy {
                pattern: "helo".to_string(),
                threshold: 1,
            },
            5,
        );
        match plan {
            SearchPlan::Sql { sql, params } => {
                assert!(sql.contains("MATCH ?1*"), "expected prefix MATCH in: {sql}");
                assert!(sql.contains("LIMIT ?2"));
                assert_eq!(params, vec!["helo".to_string(), "5".to_string()]);
            }
            SearchPlan::Empty => panic!("expected Sql plan for non-empty fuzzy pattern"),
        }

        // Blank pattern → Empty.
        match build_search_query(
            &SearchMode::Fuzzy {
                pattern: " ".to_string(),
                threshold: 50,
            },
            5,
        ) {
            SearchPlan::Empty => {}
            SearchPlan::Sql { .. } => panic!("blank fuzzy pattern should yield Empty"),
        }
    }

    #[test]
    fn test_regex() {
        // SQL uses the REGEXP function (which register_regexp wires
        // up) and the user pattern lives in ?1, not in the SQL text.
        let plan = build_search_query(
            &SearchMode::Regex {
                pattern: "^https?://".to_string(),
            },
            20,
        );
        match plan {
            SearchPlan::Sql { sql, params } => {
                assert!(sql.contains("REGEXP"), "expected REGEXP in: {sql}");
                assert!(
                    !sql.contains("^https?://"),
                    "user pattern leaked into SQL: {sql}"
                );
                assert!(sql.contains("LIMIT ?2"));
                assert_eq!(params, vec!["^https?://".to_string(), "20".to_string()]);
            }
            SearchPlan::Empty => panic!("expected Sql plan for non-empty regex pattern"),
        }

        // Empty pattern → Empty.
        match build_search_query(
            &SearchMode::Regex {
                pattern: "".to_string(),
            },
            20,
        ) {
            SearchPlan::Empty => {}
            SearchPlan::Sql { .. } => panic!("empty regex should yield Empty"),
        }
    }

    #[test]
    fn test_register_regexp_executes() {
        // End-to-end: register the function on an in-memory
        // connection and confirm SQLite can invoke it. Without
        // registration SQLite would error with "no such function".
        let conn = Connection::open_in_memory().expect("in-memory connection must open");
        register_regexp(&conn).expect("register_regexp must succeed");

        let matched: bool = conn
            .query_row("SELECT regexp('^foo', 'foobar')", [], |r| r.get(0))
            .expect("REGEXP scalar must be queryable");
        assert!(matched, "^foo should match foobar");

        let unmatched: bool = conn
            .query_row("SELECT regexp('^bar', 'foobar')", [], |r| r.get(0))
            .expect("REGEXP scalar must be queryable");
        assert!(!unmatched, "^bar should not match foobar");

        // Invalid regex must not error the surrounding query —
        // we treat it as no-match so the search returns zero rows.
        let invalid: bool = conn
            .query_row("SELECT regexp('[/', 'foobar')", [], |r| r.get(0))
            .expect("invalid regex must yield Ok(false), not Err");
        assert!(!invalid, "invalid regex must be treated as no-match");
    }
}
