//! `dzc add` — create a new clipboard entry.
//!
//! Content sources (mutually exclusive, resolved by the binary from a
//! single positional `content` token):
//! * plain text   — stored as-is
//! * `-`          — read all of stdin until EOF (`read_to_end`)
//! * `@<path>`    — read the entire file contents
//!
//! Content type is inferred as `text` by default; `--type` overrides
//! (accepted aliases: `html`, `image`/`png`/`jpg`/... for image
//! `data:` URIs, `code`, `file`). The CLI never executes or interprets
//! the payload — it is stored verbatim as a clipboard entry. This is
//! the `G29` "pure data only" guardrail: no `--exec`, no shell, no
//! piping through interpreters.
//!
//! Output: prints `Added: id=<n>` on success.
//!
//! Errors:
//! * empty content (after resolving stdin/file/text) → `Err`
//! * unknown `--type` value → passed through (the repo doesn't validate)
//! * missing source (caller bug — neither text/stdin/file set) → `Err`

use std::io::Read;
use std::path::Path;

use crate::domain::models::ClipboardEntry;
use crate::infrastructure::repository::clipboard_repo::ClipboardRepository;

/// Args for the `add` subcommand. The bin resolves the positional
/// `content` token into one of `text` / `stdin` / `file`; this struct
/// keeps the cli module independent of clap's parsing.
pub struct AddArgs {
    /// Literal text payload (when the user passed a non-special token).
    pub text: Option<String>,
    /// `true` when the user passed `-` (read stdin until EOF).
    pub stdin: bool,
    /// File path (when the user passed `@<path>`).
    pub file: Option<String>,
    /// Optional content type override (`text`, `html`, `image`, ...).
    pub kind: Option<String>,
}

/// Preview length kept in the entry's `preview` column. The in-tree
/// `repo.save_with_conn` will further truncate for the history list
/// view, but the canonical preview is stored at this length.
const PREVIEW_CHARS: usize = 500;

/// Run the `add` subcommand. Returns the newly-inserted entry id.
pub fn run(args: &AddArgs, repo: &dyn ClipboardRepository) -> Result<i64, String> {
    let content = read_content(args)?;
    if content.is_empty() {
        return Err("add: empty content (refusing to insert)".to_string());
    }

    let content_type = args
        .kind
        .as_deref()
        .map(normalize_kind)
        .unwrap_or_else(|| "text".to_string());

    let preview = preview_of(&content);
    let entry = ClipboardEntry {
        id: 0,
        content_type,
        content,
        html_content: None,
        source_app: "dzc".to_string(),
        source_app_path: None,
        timestamp: chrono::Utc::now().timestamp_millis(),
        preview,
        is_pinned: false,
        tags: Vec::new(),
        use_count: 0,
        is_external: false,
        pinned_order: 0,
        file_preview_exists: true,
        content_kinds: Vec::new(),
        ocr_text: None,
        ocr_status: None,
    };

    repo.save(&entry, None)
        .map_err(|e| format!("add: save failed: {e}"))
}

/// Resolve the three content sources into a single string. Exactly one
/// of `text` / `stdin` / `file` must be set; this is the binary's job
/// to enforce. The cli surface is defensive: returns an informative
/// error if none are set so future callers can't accidentally insert
/// empty payloads.
fn read_content(args: &AddArgs) -> Result<String, String> {
    if args.stdin {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| format!("add: read stdin: {e}"))?;
        return Ok(buf);
    }
    if let Some(path) = &args.file {
        return std::fs::read_to_string(Path::new(path))
            .map_err(|e| format!("add: read file {path:?}: {e}"));
    }
    if let Some(text) = &args.text {
        return Ok(text.clone());
    }
    Err("add: no content source (set text, stdin=true, or file)".to_string())
}

/// Map `--type` aliases onto the canonical `content_type` strings the
/// repo stores. Unknown values pass through unchanged so a future
/// content type doesn't silently degrade to `text`.
fn normalize_kind(raw: &str) -> String {
    match raw.to_ascii_lowercase().as_str() {
        "text" | "txt" | "plain" => "text".to_string(),
        "html" => "html".to_string(),
        "image" | "img" | "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" => "image".to_string(),
        "code" | "src" => "code".to_string(),
        "file" | "path" => "file".to_string(),
        other => other.to_string(),
    }
}

/// Build the `preview` field: first `PREVIEW_CHARS` characters of
/// `content`. Mirrors the `truncate_chars_with_suffix` helper used by
/// `commands::clipboard_cmd::add_manual_item` but without the ellipsis
/// suffix — the in-tree repo's history viewer applies its own
/// truncation on read, so we keep the full preview byte-faithful here.
fn preview_of(content: &str) -> String {
    content.chars().take(PREVIEW_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_kind_canonical() {
        assert_eq!(normalize_kind("text"), "text");
        assert_eq!(normalize_kind("html"), "html");
        assert_eq!(normalize_kind("image"), "image");
        assert_eq!(normalize_kind("code"), "code");
        assert_eq!(normalize_kind("file"), "file");
    }

    #[test]
    fn normalize_kind_aliases_lowercase() {
        assert_eq!(normalize_kind("TXT"), "text");
        assert_eq!(normalize_kind("PNG"), "image");
        assert_eq!(normalize_kind("JPEG"), "image");
        assert_eq!(normalize_kind("HTML"), "html");
    }

    #[test]
    fn normalize_kind_unknown_passes_through() {
        // Future content types should not silently degrade.
        assert_eq!(normalize_kind("rich_text"), "rich_text");
        assert_eq!(normalize_kind("weird"), "weird");
    }

    #[test]
    fn preview_of_truncates_at_boundary() {
        let long: String = "x".repeat(PREVIEW_CHARS * 2);
        let p = preview_of(&long);
        assert_eq!(p.chars().count(), PREVIEW_CHARS);
    }

    #[test]
    fn preview_of_short_content_unchanged() {
        assert_eq!(preview_of("hi"), "hi");
        assert_eq!(preview_of(""), "");
    }

    #[test]
    fn read_content_text_branch() {
        let args = AddArgs {
            text: Some("hello".into()),
            stdin: false,
            file: None,
            kind: None,
        };
        assert_eq!(read_content(&args).unwrap(), "hello");
    }

    #[test]
    fn read_content_file_branch() {
        let dir = std::env::temp_dir();
        let path = dir.join("dezirclip_c_add_test.txt");
        std::fs::write(&path, "from file").unwrap();
        let args = AddArgs {
            text: None,
            stdin: false,
            file: Some(path.to_string_lossy().to_string()),
            kind: None,
        };
        let s = read_content(&args).unwrap();
        assert_eq!(s, "from file");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn read_content_missing_source_errors() {
        let args = AddArgs {
            text: None,
            stdin: false,
            file: None,
            kind: None,
        };
        assert!(read_content(&args).is_err());
    }
}
