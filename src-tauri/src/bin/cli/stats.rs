//! `dzc stats` — show aggregate statistics about clipboard history.
//!
//! Two output modes:
//! * Human (default): multi-line text block. Easy to eyeball in a
//!   terminal.
//! * `--json`: a flat JSON object with `total`, `by_type`, and
//!   `most_used` keys. Script-friendly.
//!
//! ## Args
//!
//! * `--top <N>` — number of entries to include in the `most_used`
//!   array. Default 5. Pass `0` to suppress the section entirely.
//! * `--json` — JSON output (see `format_json`).
//!
//! ## Output schema (JSON)
//!
//! ```json
//! {
//!   "total": 1234,
//!   "by_type": { "text": 800, "html": 200, "image": 100, ... },
//!   "most_used": [
//!     { "id": 42, "use_count": 17, "preview": "..." },
//!     ...
//!   ]
//! }
//! ```
//!
//! `by_type` always includes every content type that has at least
//! one entry — the map is *sparse* in the sense that it omits types
//! with zero count, not the other way around. The keys are the
//! `content_type` strings as stored in the DB (`text`, `html`,
//! `image`, `file`, `code`, `video`, ...).
//!
//! `most_used` is sorted by `use_count` DESC then `id` DESC (stable
//! secondary order). Ties are broken by ID for determinism.

use std::collections::BTreeMap;

use crate::domain::models::ClipboardEntry;
use crate::infrastructure::repository::clipboard_repo::ClipboardRepository;

/// Default `top` count when `--top` is omitted.
pub const DEFAULT_TOP: i32 = 5;

/// Args for the `stats` subcommand.
pub struct StatsArgs {
    /// Number of entries to show in `most_used`. `None` → use
    /// `DEFAULT_TOP`. A value of `0` suppresses the section.
    pub top: Option<i32>,
    /// Emit JSON instead of human-readable text.
    pub json: bool,
}

/// Run the `stats` subcommand.
///
/// Returns `Ok(())` on success. The output is written to stdout.
/// Errors from the repo (e.g. DB failure) are propagated as
/// `Err(String)`.
pub fn run(args: &StatsArgs, repo: &dyn ClipboardRepository) -> Result<(), String> {
    // 1. Fetch every entry. `get_history(i32::MAX, 0, None)` is the
    //    repo's "all rows" form. Stats are a single full scan; we
    //    don't try to be clever with SQL aggregates here because the
    //    repo trait doesn't expose them and the volume per user is
    //    bounded (thousands, not millions).
    let entries = repo
        .get_history(i32::MAX, 0, None)
        .map_err(|e| format!("stats: get_history failed: {e}"))?;

    // 2. Build the breakdown.
    let total = entries.len();
    let by_type = count_by_type(&entries);

    // 3. Top-N by use_count. Stable sort: use_count DESC, then id DESC.
    let top_n = args.top.unwrap_or(DEFAULT_TOP).max(0) as usize;
    let mut most_used = entries.clone();
    most_used.sort_by(|a, b| b.use_count.cmp(&a.use_count).then_with(|| b.id.cmp(&a.id)));
    most_used.truncate(top_n);

    // 4. Emit.
    if args.json {
        let s = format_json(total, &by_type, &most_used);
        println!("{s}");
    } else {
        print_human(total, &by_type, &most_used);
    }
    Ok(())
}

/// Count entries by `content_type`. Returns a `BTreeMap` so the JSON
/// serialization has stable key order (alphabetical), and the human
/// output has a deterministic layout across runs.
fn count_by_type(entries: &[ClipboardEntry]) -> BTreeMap<String, usize> {
    let mut out: BTreeMap<String, usize> = BTreeMap::new();
    for e in entries {
        *out.entry(e.content_type.clone()).or_insert(0) += 1;
    }
    out
}

/// Format the human-readable multi-line output. Layout:
///
/// ```text
/// Total: 1234 entries
///
/// By type:
///   text: 800
///   html: 200
///   image: 100
///   ...
///
/// Most used (top N):
///   1. [id=42, used=17] preview...
///   2. [id=99, used=12] preview...
/// ```
///
/// The section is omitted (not just empty) when `most_used` is empty
/// so a 0-top run prints only the totals.
fn print_human(
    total: usize,
    by_type: &BTreeMap<String, usize>,
    most_used: &[ClipboardEntry],
) {
    println!("Total: {total} entries");
    println!();
    println!("By type:");
    if by_type.is_empty() {
        println!("  (none)");
    } else {
        for (kind, count) in by_type {
            println!("  {kind}: {count}");
        }
    }
    if !most_used.is_empty() {
        println!();
        println!("Most used (top {}):", most_used.len());
        for (i, e) in most_used.iter().enumerate() {
            let preview = one_line_preview(&e.preview);
            println!(
                "  {}. [id={}, used={}] {}",
                i + 1,
                e.id,
                e.use_count,
                preview
            );
        }
    }
}

/// Format the JSON output. Uses `serde_json` so the escaping is
/// correct; the call site doesn't need to worry about quotes or
/// control chars in the preview text.
fn format_json(
    total: usize,
    by_type: &BTreeMap<String, usize>,
    most_used: &[ClipboardEntry],
) -> String {
    // Build a struct shape that serializes to the documented schema.
    // Using a derived `Serialize` keeps escaping correct.
    #[derive(serde::Serialize)]
    struct MostUsed<'a> {
        id: i64,
        use_count: i32,
        preview: &'a str,
    }
    #[derive(serde::Serialize)]
    struct Out<'a> {
        total: usize,
        by_type: &'a BTreeMap<String, usize>,
        most_used: Vec<MostUsed<'a>>,
    }
    let payload = Out {
        total,
        by_type,
        most_used: most_used
            .iter()
            .map(|e| MostUsed {
                id: e.id,
                use_count: e.use_count,
                preview: &e.preview,
            })
            .collect(),
    };
    serde_json::to_string(&payload)
        .unwrap_or_else(|e| format!("{{\"error\":\"json serialize failed: {e}\"}}"))
}

/// Collapse a multi-line preview into one line for the human
/// summary. The repo's `preview` is single-line by construction, but
/// it could contain newlines if an entry's content has them — we
/// defend against that here so the output stays aligned.
fn one_line_preview(s: &str) -> String {
    let first = s.lines().next().unwrap_or("");
    // Cap to ~80 chars to keep the line from wrapping in a typical
    // 100-col terminal. The repo's `preview` is already truncated to
    // ~500 chars; we apply a tighter cap for the stats view.
    if first.chars().count() > 80 {
        let truncated: String = first.chars().take(77).collect();
        format!("{truncated}...")
    } else {
        first.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: i64, kind: &str, use_count: i32, preview: &str) -> ClipboardEntry {
        let mut e = ClipboardEntry {
            id,
            content_type: kind.to_string(),
            content: preview.to_string(),
            html_content: None,
            source_app: String::new(),
            source_app_path: None,
            timestamp: 0,
            preview: preview.to_string(),
            is_pinned: false,
            tags: vec![],
            use_count,
            is_external: false,
            pinned_order: 0,
            file_preview_exists: false,
            content_kinds: vec![],
            ocr_text: None,
            ocr_status: None,
        };
        e.use_count = use_count;
        e
    }

    #[test]
    fn count_by_type_groups_correctly() {
        let entries = vec![
            entry(1, "text", 0, "a"),
            entry(2, "text", 0, "b"),
            entry(3, "html", 0, "c"),
            entry(4, "image", 0, "d"),
        ];
        let m = count_by_type(&entries);
        assert_eq!(m.get("text"), Some(&2));
        assert_eq!(m.get("html"), Some(&1));
        assert_eq!(m.get("image"), Some(&1));
        assert_eq!(m.len(), 3);
    }

    #[test]
    fn format_json_contains_required_keys() {
        let entries = vec![
            entry(1, "text", 5, "hello"),
            entry(2, "html", 2, "<b>world</b>"),
        ];
        let total = entries.len();
        let by_type = count_by_type(&entries);
        let mut most_used = entries.clone();
        most_used.sort_by(|a, b| b.use_count.cmp(&a.use_count));
        let s = format_json(total, &by_type, &most_used);
        assert!(s.contains("\"total\":2"), "got: {s}");
        assert!(s.contains("\"by_type\""), "got: {s}");
        assert!(s.contains("\"most_used\""), "got: {s}");
        assert!(s.contains("\"text\":1"), "got: {s}");
        assert!(s.contains("\"html\":1"), "got: {s}");
        assert!(s.contains("\"id\":1"), "got: {s}");
        assert!(s.contains("\"use_count\":5"), "got: {s}");
    }

    #[test]
    fn one_line_preview_truncates_long_input() {
        let long = "a".repeat(120);
        let out = one_line_preview(&long);
        assert!(out.chars().count() <= 80, "preview too long: {} chars", out.chars().count());
        assert!(out.ends_with("..."), "long preview should be truncated with ellipsis: {out}");
    }

    #[test]
    fn one_line_preview_collapses_newlines() {
        let out = one_line_preview("first line\nsecond line");
        assert_eq!(out, "first line");
    }
}
