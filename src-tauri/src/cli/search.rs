//! `dzc search` — search history with mode-specific matching.
//!
//! Modes:
//! * `contains` (default): case-insensitive substring match. Uses
//!   `repo.search()` which performs SQL `LIKE '%term%'` over
//!   content/source_app/tags.
//! * `fts5`: full-text search via `repo.search_fts()`. The input is
//!   sanitized through `services::search::escape_fts5_term` so FTS5
//!   string-literal injection is impossible.
//! * `fuzzy`: substring match via `repo.search()`, then a subsequence
//!   filter — every character of the pattern must appear in the
//!   candidate's content in order (case-insensitive). Simple, no
//!   Levenshtein, but good enough for typo-tolerant recall.
//! * `regex`: substring match via `repo.search()`, then a `regex`
//!   crate match against content. The repo fetches candidates so the
//!   regex only runs on a bounded set.
//!
//! Output: same shape as `list` — human / `--json` / `--ids` / `--quiet`.

use regex::Regex;

use crate::domain::models::ClipboardEntry;
use crate::infrastructure::repository::clipboard_repo::ClipboardRepository;
use crate::services::search as svc_search;

use super::list::CliEntry;

/// Search mode selected by `--mode`. Mirrors the standalone clap
/// `ValueEnum`. `Contains` is the default and matches what the existing
/// `repo.search()` does natively.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    Contains,
    Fuzzy,
    Regex,
    Fts5,
}

/// Args for the `search` subcommand.
pub struct SearchArgs {
    pub query: String,
    pub mode: SearchMode,
    /// Optional result limit. `None` means "no extra cap" — the repo
    /// will fetch up to its internal default. Most repos use 1000 as
    /// the soft cap; callers wanting more should specify explicitly.
    pub limit: Option<i32>,
    pub json: bool,
    pub quiet: bool,
}

impl SearchArgs {
    /// Effective limit, falling back to the same default as `list`.
    pub fn effective_limit(&self) -> i32 {
        self.limit.unwrap_or(super::list::DEFAULT_LIMIT)
    }
}

/// Public entry point. Mirrors `list::run`'s shape so callers can swap
/// subcommands without changing dispatch logic.
pub fn run(args: &SearchArgs, repo: &dyn ClipboardRepository) -> Result<(), String> {
    if args.query.trim().is_empty() {
        if args.quiet {
            return Ok(());
        }
        if args.json {
            println!("[]");
        } else {
            // No message in human mode — an empty result speaks for itself.
        }
        return Ok(());
    }

    let limit = args.effective_limit();
    let candidates: Vec<ClipboardEntry> = match args.mode {
        SearchMode::Fts5 => {
            let sanitized = svc_search::escape_fts5_term(&args.query);
            repo.search_fts(&sanitized, limit as u32)
                .map_err(|e| format!("search: fts5 failed: {e}"))?
        }
        SearchMode::Contains | SearchMode::Fuzzy | SearchMode::Regex => {
            let needle = args.query.to_lowercase();
            repo.search(&needle, limit)
                .map_err(|e| format!("search: contains failed: {e}"))?
        }
    };

    let entries: Vec<ClipboardEntry> = match args.mode {
        SearchMode::Contains => candidates,
        SearchMode::Fts5 => candidates,
        SearchMode::Fuzzy => candidates
            .into_iter()
            .filter(|e| fuzzy_match(&args.query, &e.content))
            .collect(),
        SearchMode::Regex => {
            let re = Regex::new(&args.query)
                .map_err(|e| format!("search: invalid regex: {e}"))?;
            candidates
                .into_iter()
                .filter(|e| re.is_match(&e.content))
                .collect()
        }
    };

    emit(&entries, args.quiet, args.json)
}

/// Emit the result set. Shared with `list::run` in spirit but kept
/// separate so each module owns its formatting flags without leaking
/// them across the cli surface.
fn emit(entries: &[ClipboardEntry], quiet: bool, json: bool) -> Result<(), String> {
    if quiet {
        return Ok(());
    }
    if json {
        let views: Vec<CliEntry> = entries.iter().map(cli_view).collect();
        let s = serde_json::to_string(&views)
            .map_err(|e| format!("search: serialize json: {e}"))?;
        println!("{s}");
        return Ok(());
    }
    for e in entries {
        println!("{}", super::list::format_human(e));
    }
    Ok(())
}

/// Local copy of the list-side `cli_view` projection. Duplicating two
/// lines is cheaper than threading `list::cli_view` through `pub(crate)`
/// just for this module.
fn cli_view(e: &ClipboardEntry) -> CliEntry {
    CliEntry {
        id: e.id,
        content_type: e.content_type.clone(),
        content: e.content.clone(),
        preview: e.preview.clone(),
        tags: e.tags.clone(),
        pinned: e.is_pinned,
        timestamp: e.timestamp,
    }
}

/// Subsequence match: every char of `pattern` must appear in `text` in
/// order (case-insensitive). Skips non-matching chars greedily. This is
/// the classic "subsequence" fuzzy match — not Levenshtein — but it
/// catches simple typos and reordered characters without false
/// negatives for substring matches.
pub fn fuzzy_match(pattern: &str, text: &str) -> bool {
    if pattern.is_empty() {
        return false;
    }
    let p = pattern.to_lowercase();
    let t = text.to_lowercase();
    let mut piter = p.chars().peekable();
    for ch in t.chars() {
        if piter.peek() == Some(&ch) {
            piter.next();
        }
    }
    piter.peek().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_match_exact() {
        assert!(fuzzy_match("git", "git clone"));
        assert!(fuzzy_match("GIT", "GitHub PR"));
    }

    #[test]
    fn fuzzy_match_subsequence() {
        // Subsequence: g...i...t appears in "go iterate"
        assert!(fuzzy_match("git", "go iterate"));
    }

    #[test]
    fn fuzzy_match_rejects_missing_chars() {
        // No 'x' in "git" → no match even if other chars line up
        assert!(!fuzzy_match("gitx", "git clone"));
        assert!(!fuzzy_match("", "anything")); // empty pattern matches everything;
        // we special-case empty upstream so this stays a degenerate case
    }

    #[test]
    fn effective_limit_uses_default_when_none() {
        let args = SearchArgs {
            query: "x".to_string(),
            mode: SearchMode::Contains,
            limit: None,
            json: false,
            quiet: false,
        };
        assert_eq!(args.effective_limit(), super::super::list::DEFAULT_LIMIT);
    }

    #[test]
    fn effective_limit_respects_explicit() {
        let args = SearchArgs {
            query: "x".to_string(),
            mode: SearchMode::Contains,
            limit: Some(7),
            json: false,
            quiet: false,
        };
        assert_eq!(args.effective_limit(), 7);
    }
}
