//! List history with filters and privacy exclusions applied before pagination.
//! Metadata queries omit stored content; JSON payload queries return complete entries.

use serde::Serialize;

use crate::domain::models::ClipboardEntry;
use crate::api::HistoryQuery;
use crate::api::HistoryService;

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
    pub metadata: bool,
    pub tsv: bool,
    pub offset: i32,
    pub exclude_sensitive: bool,
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
pub fn run(args: &ListArgs, repo: &HistoryService) -> Result<(), String> {
    let limit = checked_limit(args.limit)?;
    let entries = super::access::query(repo, &HistoryQuery {
        limit,
        offset: args.offset,
        kind: args.kind.as_deref(),
        tag: args.tag.as_deref(),
        pinned: args.pinned,
        metadata: args.metadata,
        exclude_sensitive: args.exclude_sensitive,
        ..Default::default()
    })?;
    emit(
        &entries,
        args.quiet,
        args.json,
        args.ids,
        args.tsv,
        args.metadata,
    )
}

pub fn checked_limit(limit: Option<i32>) -> Result<i32, String> {
    match limit.unwrap_or(DEFAULT_LIMIT) {
        -1 => Ok(UNLIMITED),
        n if n >= 0 => Ok(n),
        _ => Err("limit must be -1 or a nonnegative integer".to_string()),
    }
}

#[derive(Serialize)]
pub struct OutputEntry<'a> {
    id: i64,
    #[serde(rename = "type")]
    content_type: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<&'a str>,
    preview: &'a str,
    tags: &'a [String],
    pinned: bool,
    timestamp: i64,
    mime: &'static str,
    sensitive: bool,
}

pub fn json_view(entry: &ClipboardEntry, metadata: bool) -> Result<OutputEntry<'_>, String> {
    Ok(OutputEntry {
        id: entry.id,
        content_type: &entry.content_type,
        content: (!metadata).then_some(entry.content.as_str()),
        preview: &entry.preview,
        tags: &entry.tags,
        pinned: entry.is_pinned,
        timestamp: entry.timestamp,
        mime: super::payload::mime_for(entry),
        sensitive: crate::database::has_sensitive_tag(&entry.tags),
    })
}

pub fn emit(
    entries: &[ClipboardEntry],
    quiet: bool,
    json: bool,
    ids: bool,
    tsv: bool,
    metadata: bool,
) -> Result<(), String> {
    if quiet {
        return Ok(());
    }
    if json {
        let values: Result<Vec<_>, _> = entries.iter().map(|e| json_view(e, metadata)).collect();
        println!(
            "{}",
            serde_json::to_string(&values?).map_err(|e| e.to_string())?
        );
    } else {
        for entry in entries {
            if ids {
                println!("{}", entry.id);
            } else if tsv {
                println!(
                    "{}\t{}",
                    entry.id,
                    preview_line(&entry.preview)
                        .chars()
                        .filter(|c| !c.is_control())
                        .collect::<String>()
                );
            } else {
                println!("{}", format_human(entry));
            }
        }
    }
    Ok(())
}

/// Project a domain entry onto the CLI-facing subset.
#[cfg(test)]
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
        assert!(
            line.starts_with("\u{1F4CC} \u{1F310}"),
            "wrong prefix: {line}"
        );
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
