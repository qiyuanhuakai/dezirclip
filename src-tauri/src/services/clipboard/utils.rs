use crate::domain::models::ClipboardEntry;
use std::borrow::Cow;

const CONTENT_PREVIEW_MAX_CHARS: usize = 2000;
const HTML_PREVIEW_MAX_CHARS: usize = 5000;
const HTML_PREVIEW_MAX_ROWS: usize = 10;
const HTML_TRUNCATION_SUFFIX: &str = "... [HTML Truncated]";

fn truncate_chars_with_suffix(text: &str, max_chars: usize, suffix: &str) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let cut = text
        .char_indices()
        .nth(max_chars)
        .map(|(idx, _)| idx)
        .unwrap_or(text.len());
    let mut out = String::with_capacity(cut + suffix.len());
    out.push_str(&text[..cut]);
    out.push_str(suffix);
    out
}

/// Text and HTML are bounded for explicit summary reads. In-memory images keep
/// their complete data URL; capture notifications themselves contain only an ID.
pub fn entry_needs_ui_truncation(entry: &ClipboardEntry) -> bool {
    let content_over = matches!(
        entry.content_type.as_str(),
        "text" | "code" | "url" | "rich_text"
    ) && entry.content.chars().count() > CONTENT_PREVIEW_MAX_CHARS;
    let html_over = entry
        .html_content
        .as_deref()
        .is_some_and(|html| html.chars().count() > HTML_PREVIEW_MAX_CHARS);
    content_over || html_over
}

/// Borrow summaries that need no capping, avoiding another complete image copy.
pub fn truncate_entry_for_ui(entry: &ClipboardEntry) -> Cow<'_, ClipboardEntry> {
    if !entry_needs_ui_truncation(entry) {
        return Cow::Borrowed(entry);
    }

    let mut capped = entry.clone();
    if capped.content.chars().count() > CONTENT_PREVIEW_MAX_CHARS
        && matches!(
            capped.content_type.as_str(),
            "text" | "code" | "url" | "rich_text"
        )
    {
        capped.content = format!(
            "{}... [Truncated for speed]",
            capped
                .content
                .chars()
                .take(CONTENT_PREVIEW_MAX_CHARS)
                .collect::<String>()
        );
    }

    if let Some(ref html) = capped.html_content {
        if html.chars().count() > HTML_PREVIEW_MAX_CHARS {
            capped.html_content = truncate_html_for_preview(html);
        }
    }

    Cow::Owned(capped)
}

pub fn truncate_html_for_preview(html: &str) -> Option<String> {
    if html.trim().is_empty() {
        return None;
    }

    if html.chars().count() <= HTML_PREVIEW_MAX_CHARS {
        return Some(html.to_string());
    }

    let trimmed = html.trim();
    let lower = trimmed.to_ascii_lowercase();
    let table_pos = lower.find("<table");
    let tr_pos = lower.find("<tr");
    let start_pos = match (table_pos, tr_pos) {
        (Some(t), Some(r)) => Some(std::cmp::min(t, r)),
        (Some(t), None) => Some(t),
        (None, Some(r)) => Some(r),
        (None, None) => None,
    };

    if let Some(start) = start_pos {
        let slice = &trimmed[start..];
        let lower_slice = &lower[start..];
        let mut end_rel = 0usize;
        let mut rows = 0usize;
        let mut search_idx = 0usize;

        while rows < HTML_PREVIEW_MAX_ROWS {
            if let Some(pos) = lower_slice[search_idx..].find("</tr") {
                let close_start = search_idx + pos;
                let close_end = lower_slice[close_start..]
                    .find('>')
                    .map(|p| close_start + p + 1)
                    .unwrap_or(close_start + 4);
                end_rel = close_end;
                rows += 1;
                search_idx = close_end;
            } else {
                break;
            }
        }

        if end_rel == 0 {
            end_rel = slice
                .char_indices()
                .nth(HTML_PREVIEW_MAX_CHARS)
                .map(|(i, _)| i)
                .unwrap_or(slice.len());
        }

        let mut out = slice[..end_rel].to_string();
        if lower_slice.starts_with("<tr") {
            out = format!(
                "<table style=\"border-collapse: collapse; min-width: 100%;\">{}</table>",
                out
            );
        } else if lower_slice.starts_with("<table") {
            if !out.to_ascii_lowercase().contains("</table") {
                out.push_str("</table>");
            }
        }

        if out.chars().count() > HTML_PREVIEW_MAX_CHARS {
            out = truncate_chars_with_suffix(&out, HTML_PREVIEW_MAX_CHARS, HTML_TRUNCATION_SUFFIX);
        }

        return Some(out);
    }

    Some(truncate_chars_with_suffix(
        trimmed,
        HTML_PREVIEW_MAX_CHARS,
        HTML_TRUNCATION_SUFFIX,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;

    fn entry_of(content_type: &str, content: String, html: Option<String>) -> ClipboardEntry {
        ClipboardEntry {
            id: 1,
            content_type: content_type.to_string(),
            content,
            html_content: html,
            source_app: "app".to_string(),
            source_app_path: None,
            timestamp: 0,
            preview: "p".to_string(),
            is_pinned: false,
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

    /// An image is the case that makes the borrow worth having: the payload is
    /// a whole data URL and no cap applies to it, so any copy of the entry on
    /// this path is pure waste.
    #[test]
    fn image_payload_is_borrowed_not_copied() {
        let url = format!("data:image/png;base64,{}", "A".repeat(1 << 20));
        let entry = entry_of("image", url.clone(), None);

        let payload = truncate_entry_for_ui(&entry);

        assert!(
            matches!(payload, Cow::Borrowed(_)),
            "an image entry must not be copied on the emit path"
        );
        assert_eq!(payload.content, url);
    }

    #[test]
    fn short_text_is_borrowed() {
        let entry = entry_of("text", "hello".to_string(), None);
        let payload = truncate_entry_for_ui(&entry);
        assert!(matches!(payload, Cow::Borrowed(_)));
        assert_eq!(payload.content, "hello");
    }

    #[test]
    fn long_text_is_capped_and_owned() {
        let body = "x".repeat(CONTENT_PREVIEW_MAX_CHARS + 500);
        let entry = entry_of("text", body, None);

        let payload = truncate_entry_for_ui(&entry);

        assert!(matches!(payload, Cow::Owned(_)));
        assert!(payload.content.ends_with("... [Truncated for speed]"));
        assert_eq!(
            payload.content.chars().filter(|c| *c == 'x').count(),
            CONTENT_PREVIEW_MAX_CHARS
        );
    }

    #[test]
    fn long_text_at_exactly_the_limit_is_left_alone() {
        let body = "x".repeat(CONTENT_PREVIEW_MAX_CHARS);
        let entry = entry_of("text", body.clone(), None);

        let payload = truncate_entry_for_ui(&entry);

        assert!(matches!(payload, Cow::Borrowed(_)));
        assert_eq!(payload.content, body);
    }

    #[test]
    fn overlong_body_of_an_uncapped_type_is_left_alone() {
        // Images and files are never capped: the UI needs the real payload.
        let body = "x".repeat(CONTENT_PREVIEW_MAX_CHARS * 3);
        let entry = entry_of("file", body.clone(), None);

        assert!(!entry_needs_ui_truncation(&entry));
        assert!(matches!(truncate_entry_for_ui(&entry), Cow::Borrowed(_)));
        assert_eq!(truncate_entry_for_ui(&entry).content, body);
    }

    #[test]
    fn long_html_is_capped_even_when_the_body_is_short() {
        let html = format!("<p>{}</p>", "y".repeat(HTML_PREVIEW_MAX_CHARS * 2));
        let html_len = html.len();
        let entry = entry_of("rich_text", "body".to_string(), Some(html));

        let payload = truncate_entry_for_ui(&entry);

        assert!(matches!(payload, Cow::Owned(_)));
        assert_eq!(payload.content, "body", "the body was already short");
        let capped = payload
            .html_content
            .as_ref()
            .expect("capped html is present");
        assert!(capped.len() < html_len);
        assert!(capped.contains(HTML_TRUNCATION_SUFFIX));
    }

    #[test]
    fn both_fields_capped_in_one_pass() {
        let body = "x".repeat(CONTENT_PREVIEW_MAX_CHARS + 1);
        let html = "<p>z</p>".repeat(HTML_PREVIEW_MAX_CHARS);
        let entry = entry_of("rich_text", body, Some(html));

        let payload = truncate_entry_for_ui(&entry);

        assert!(payload.content.ends_with("... [Truncated for speed]"));
        assert!(payload
            .html_content
            .as_ref()
            .is_some_and(|h| h.contains(HTML_TRUNCATION_SUFFIX)));
    }

    #[test]
    fn needs_truncation_agrees_with_what_the_cap_does() {
        for (ct, len, expect) in [
            ("text", CONTENT_PREVIEW_MAX_CHARS, false),
            ("text", CONTENT_PREVIEW_MAX_CHARS + 1, true),
            ("code", CONTENT_PREVIEW_MAX_CHARS + 1, true),
            ("url", CONTENT_PREVIEW_MAX_CHARS + 1, true),
            ("rich_text", CONTENT_PREVIEW_MAX_CHARS + 1, true),
            ("image", CONTENT_PREVIEW_MAX_CHARS + 1, false),
            ("file", CONTENT_PREVIEW_MAX_CHARS + 1, false),
            ("video", CONTENT_PREVIEW_MAX_CHARS + 1, false),
        ] {
            let entry = entry_of(ct, "x".repeat(len), None);
            assert_eq!(
                entry_needs_ui_truncation(&entry),
                expect,
                "{ct} at {len} chars"
            );
            assert_eq!(
                matches!(truncate_entry_for_ui(&entry), Cow::Borrowed(_)),
                !expect,
                "borrow-vs-own disagrees with the cap for {ct} at {len} chars"
            );
        }
    }

    #[test]
    fn multibyte_content_is_capped_by_characters_not_bytes() {
        let body = "中".repeat(CONTENT_PREVIEW_MAX_CHARS + 10);
        let entry = entry_of("text", body, None);

        let payload = truncate_entry_for_ui(&entry);

        assert!(payload.content.ends_with("... [Truncated for speed]"));
        assert_eq!(
            payload.content.chars().filter(|c| *c == '中').count(),
            CONTENT_PREVIEW_MAX_CHARS
        );
    }
}
