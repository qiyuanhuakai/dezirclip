//! `dzc list` — list clipboard history with optional filters.
//!
//! All persistence goes through `ClipboardRepository::get_history`; the
//! `--tag` and `--pinned` filters are applied in-memory after the repo
//! returns candidates because the repo's signature does not yet accept
//! those fields. Filter ordering (type → tag → pinned) is stable so
//! callers see the same result set on identical inputs.
//!
//! Output:
//! * human:  `📋 [text] preview...` / `📌 [html] preview...`
//! * `--json`: JSON array of `CliEntry`
//! * `--ids`:  one ID per line
//! * `--quiet`: silent on success

use serde::Serialize;

use crate::domain::models::ClipboardEntry;
use crate::infrastructure::repository::clipboard_repo::ClipboardRepository;

/// CLI-shaped view of a `ClipboardEntry`. Only the fields surfaced via
/// `dzc` are serialized; internal fields (`html_content`,
/// `source_app_path`, OCR, etc.) are deliberately omitted so the JSON
/// shape stays small and stable across schema changes.
#[derive(Debug, Serialize)]
pub struct CliEntry {
    pub id: i64,
    #[serde(rename = "type")]
    pub content_type: String,
    pub content: String,
    pub preview: String,
    pub tags: Vec<String>,
    pub pinned: bool,
    pub timestamp: i64,
}

/// CLI args for the `list` subcommand. Defined in the binary; the
/// subcommand only consumes a borrowed reference.
pub struct ListArgs {
    /// Optional positional limit. `None` defaults to 20; `Some(-1)` means
    /// "all" (translated to `i32::MAX` for the repo which takes `i32`).
    pub limit: Option<i32>,
    pub kind: Option<String>,
    pub tag: Option<String>,
    pub pinned: bool,
    pub json: bool,
    pub ids: bool,
    pub quiet: bool,
}

/// Default row count when `--limit` is omitted and no positional limit
/// is given. Pinned to match the user-facing help text.
pub const DEFAULT_LIMIT: i32 = 20;

/// Sentinel for "no limit" — the repo expects a positive `i32`.
/// `i32::MAX` is large enough that any realistic clipboard history will
/// fit; the repo applies its own `LIMIT ?` and SQLite caps at 2^31-1.
pub const UNLIMITED: i32 = i32::MAX;

/// Run the `list` subcommand. Returns `Ok(())` on success; any IO or
/// parse failure is propagated as `Err(String)` (no custom error type
/// needed for a CLI surface).
pub fn run(args: &ListArgs, repo: &dyn ClipboardRepository) -> Result<(), String> {
    let raw_limit = args.limit.unwrap_or(DEFAULT_LIMIT);
    let limit = if raw_limit < 0 { UNLIMITED } else { raw_limit };

    let candidates = repo
        .get_history(limit, 0, args.kind.as_deref())
        .map_err(|e| format!("list: get_history failed: {e}"))?;

    let entries: Vec<ClipboardEntry> = candidates
        .into_iter()
        .filter(|e| match &args.tag {
            Some(t) => e.tags.iter().any(|et| et == t),
            None => true,
        })
        .filter(|e| !args.pinned || e.is_pinned)
        .collect();

    if args.quiet {
        return Ok(());
    }
    if args.ids {
        for e in &entries {
            println!("{}", e.id);
        }
        return Ok(());
    }
    if args.json {
        let cli_entries: Vec<CliEntry> = entries.iter().map(cli_view).collect();
        let json = serde_json::to_string(&cli_entries)
            .map_err(|e| format!("list: serialize json: {e}"))?;
        println!("{json}");
        return Ok(());
    }

    for e in &entries {
        println!("{}", format_human(e));
    }
    Ok(())
}

/// Project a domain entry onto the CLI-facing subset.
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

/// Map a content-type string onto the human-display icon. Unknown
/// types fall back to `📋` so we never panic on a new content_type added
/// to the schema.
pub fn icon_for(content_type: &str) -> &'static str {
    match content_type {
        "text" => "\u{1F4CB}",  // 📋
        "html" => "\u{1F310}",  // 🌐
        "image" => "\u{1F5BC}", // 🖼️
        "file" => "\u{1F4C1}",  // 📁
        "code" => "\u{1F4BB}",  // 💻
        "video" => "\u{1F3AC}", // 🎬
        _ => "\u{1F4CB}",       // 📋 fallback
    }
}

/// Format an entry for the default human-readable output.
///
/// Format: `{pin?} {icon} [{type}] {preview}` where `{pin?}` is the
/// `📌` prefix when `is_pinned` is true (so pinned rows stand out in a
/// long list without forcing color).
pub fn format_human(e: &ClipboardEntry) -> String {
    let pin = if e.is_pinned { "\u{1F4CC} " } else { "" }; // 📌
    let icon = icon_for(&e.content_type);
    let preview = preview_line(&e.preview);
    format!("{pin}{icon} [{}] {preview}", e.content_type)
}

/// Reduce multi-line previews to a single line for human display. The
/// raw preview can contain `\n` (e.g. code entries); we keep the first
/// line and collapse subsequent whitespace runs to single spaces.
fn preview_line(preview: &str) -> String {
    preview.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: i64, ct: &str, preview: &str, pinned: bool) -> ClipboardEntry {
        ClipboardEntry {
            id,
            content_type: ct.to_string(),
            content: preview.to_string(),
            html_content: None,
            source_app: "test".to_string(),
            source_app_path: None,
            timestamp: 1_700_000_000 + id,
            preview: preview.to_string(),
            is_pinned: pinned,
            tags: vec![],
            use_count: 0,
            is_external: false,
            pinned_order: 0,
            file_preview_exists: true,
            content_kinds: vec![],
            ocr_text: None,
            ocr_status: None,
        }
    }

    #[test]
    fn icon_for_known_types() {
        assert_eq!(icon_for("text"), "\u{1F4CB}");
        assert_eq!(icon_for("html"), "\u{1F310}");
        assert_eq!(icon_for("image"), "\u{1F5BC}");
        assert_eq!(icon_for("file"), "\u{1F4C1}");
    }

    #[test]
    fn icon_for_unknown_falls_back() {
        assert_eq!(icon_for("never-seen"), "\u{1F4CB}");
    }

    #[test]
    fn format_human_omits_pin_when_unpinned() {
        let e = entry(1, "text", "hello", false);
        let line = format_human(&e);
        assert!(line.starts_with("\u{1F4CB}"), "icon prefix missing: {line}");
        assert!(line.contains("[text]"), "type bracket missing: {line}");
        assert!(line.contains("hello"), "preview missing: {line}");
        assert!(!line.contains("\u{1F4CC}"), "should not show pin: {line}");
    }

    #[test]
    fn format_human_shows_pin_when_pinned() {
        let e = entry(1, "html", "<b>x</b>", true);
        let line = format_human(&e);
        assert!(line.starts_with("\u{1F4CC} \u{1F310}"), "wrong prefix: {line}");
        assert!(line.contains("[html]"));
    }

    #[test]
    fn preview_line_collapses_newlines() {
        assert_eq!(preview_line(""), "");
        assert_eq!(preview_line("one"), "one");
        assert_eq!(preview_line("first\nsecond\tthird"), "first second third");
    }

    #[test]
    fn cli_view_renames_type_field() {
        let e = entry(7, "text", "hi", true);
        let v = cli_view(&e);
        assert_eq!(v.id, 7);
        assert_eq!(v.content_type, "text");
        assert_eq!(v.preview, "hi");
        assert!(v.pinned);
    }
}
