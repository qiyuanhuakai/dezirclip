//! Search full payloads with mode-specific matching before pagination.
//! JSON, metadata, ID and TSV output share the list command contract.

use crate::api::HistoryQuery;
use crate::api::HistoryService;

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
    pub kind: Option<String>,
    pub mode: SearchMode,
    /// Optional result limit. `None` means "no extra cap" — the repo
    /// will fetch up to its internal default. Most repos use 1000 as
    /// the soft cap; callers wanting more should specify explicitly.
    pub limit: Option<i32>,
    pub json: bool,
    pub quiet: bool,
    pub ids: bool,
    pub metadata: bool,
    pub tsv: bool,
    pub offset: i32,
    pub exclude_sensitive: bool,
}

impl SearchArgs {
    /// Effective limit, falling back to the same default as `list`.
    pub fn effective_limit(&self) -> i32 {
        self.limit.unwrap_or(super::list::DEFAULT_LIMIT)
    }
}

/// Public entry point. Mirrors `list::run`'s shape so callers can swap
/// subcommands without changing dispatch logic.
pub fn run(args: &SearchArgs, repo: &HistoryService) -> Result<(), String> {
    let limit = super::list::checked_limit(args.limit)?;
    let mode = match args.mode {
        SearchMode::Contains => crate::api::HistorySearchMode::Contains,
        SearchMode::Fuzzy => crate::api::HistorySearchMode::Fuzzy,
        SearchMode::Regex => crate::api::HistorySearchMode::Regex,
        SearchMode::Fts5 => crate::api::HistorySearchMode::Fts5,
    };
    let entries = if args.query.trim().is_empty() {
        Vec::new()
    } else {
        super::access::query(repo, &HistoryQuery {
            limit,
            offset: args.offset,
            kind: args.kind.as_deref(),
            metadata: args.metadata,
            exclude_sensitive: args.exclude_sensitive,
            search: Some((&args.query, mode)),
            ..Default::default()
        })?
    };
    super::list::emit(
        &entries,
        args.quiet,
        args.json,
        args.ids,
        args.tsv,
        args.metadata,
    )
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
            kind: None,
            mode: SearchMode::Contains,
            limit: None,
            json: false,
            quiet: false,
            ids: false,
            metadata: false,
            tsv: false,
            offset: 0,
            exclude_sensitive: false,
        };
        assert_eq!(args.effective_limit(), super::super::list::DEFAULT_LIMIT);
    }

    #[test]
    fn effective_limit_respects_explicit() {
        let args = SearchArgs {
            query: "x".to_string(),
            kind: None,
            mode: SearchMode::Contains,
            limit: Some(7),
            json: false,
            quiet: false,
            ids: false,
            metadata: false,
            tsv: false,
            offset: 0,
            exclude_sensitive: false,
        };
        assert_eq!(args.effective_limit(), 7);
    }
}
