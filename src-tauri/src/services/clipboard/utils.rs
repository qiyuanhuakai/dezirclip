use crate::database::save_image_to_file;
use crate::domain::models::ClipboardEntry;
use base64::{engine::general_purpose, Engine as _};
use regex::Regex;
use std::borrow::Cow;
use std::path::Path;
use std::sync::OnceLock;
use urlencoding::decode;

const CONTENT_PREVIEW_MAX_CHARS: usize = 2000;
const HTML_PREVIEW_MAX_CHARS: usize = 5000;
const HTML_PREVIEW_MAX_ROWS: usize = 10;
const HTML_TRUNCATION_SUFFIX: &str = "... [HTML Truncated]";
pub const RICH_IMAGE_FALLBACK_PREFIX: &str = "<!--TIEZ_RICH_IMAGE:";
pub const RICH_IMAGE_FALLBACK_SUFFIX: &str = "-->";

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

pub fn attach_rich_image_fallback(html: &str, payload: &str) -> String {
    let mut out = String::with_capacity(
        html.len()
            + RICH_IMAGE_FALLBACK_PREFIX.len()
            + RICH_IMAGE_FALLBACK_SUFFIX.len()
            + payload.len()
            + 1,
    );
    out.push_str(html.trim_end());
    out.push('\n');
    out.push_str(RICH_IMAGE_FALLBACK_PREFIX);
    out.push_str(payload);
    out.push_str(RICH_IMAGE_FALLBACK_SUFFIX);
    out
}

pub fn split_rich_html_and_image_fallback(html: &str) -> (String, Option<String>) {
    if let Some(start) = html.rfind(RICH_IMAGE_FALLBACK_PREFIX) {
        let marker_start = start + RICH_IMAGE_FALLBACK_PREFIX.len();
        if let Some(end_rel) = html[marker_start..].find(RICH_IMAGE_FALLBACK_SUFFIX) {
            let marker_end = marker_start + end_rel;
            let mut cleaned = String::with_capacity(html.len());
            cleaned.push_str(&html[..start]);
            cleaned.push_str(&html[marker_end + RICH_IMAGE_FALLBACK_SUFFIX.len()..]);
            let payload = html[marker_start..marker_end].trim().to_string();
            return (cleaned.trim().to_string(), Some(payload));
        }
    }
    (html.to_string(), None)
}

pub fn externalize_rich_image_fallback(html: &str, data_dir: &Path) -> String {
    let (clean_html, payload_opt) = split_rich_html_and_image_fallback(html);
    let Some(payload) = payload_opt else {
        return html.to_string();
    };

    if !payload.starts_with("data:image/") {
        return html.to_string();
    }

    if let Some(saved_path) = save_image_to_file(&payload, data_dir) {
        let base_html = if clean_html.trim().is_empty() {
            html
        } else {
            clean_html.as_str()
        };
        return attach_rich_image_fallback(base_html, &saved_path);
    }

    html.to_string()
}

/// Caps an entry's text and HTML so a huge clipboard payload does not cross
/// the IPC boundary whole, and reports whether anything was actually capped.
///
/// An image carries its entire data URL and is never capped, so it is the case
/// where the cap does nothing — and where copying the entry to find that out
/// costs the most.
pub fn entry_needs_ui_truncation(entry: &ClipboardEntry) -> bool {
    let content_over = entry.content.chars().count() > CONTENT_PREVIEW_MAX_CHARS
        && matches!(
            entry.content_type.as_str(),
            "text" | "code" | "url" | "rich_text"
        );
    let html_over = entry
        .html_content
        .as_deref()
        .is_some_and(|html| html.chars().count() > HTML_PREVIEW_MAX_CHARS);
    content_over || html_over
}

/// Prepares the payload for the `clipboard-updated` event.
///
/// Entries that need no capping are borrowed rather than copied. The emit path
/// used to clone the whole entry before calling this, so every clipboard event
/// duplicated the full data URL of a screenshot — megabytes — only to hand back
/// the same bytes for the image types this function never touches.
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
            capped.content
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

pub fn detect_content_type(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.starts_with("http") || trimmed.starts_with("www.") {
        return "url".to_string();
    }

    let mut score = 0;
    let keywords = [
        "import ",
        "const ",
        "let ",
        "var ",
        "function ",
        "class ",
        "pub fn ",
        "impl ",
        "#include",
        "package ",
        "interface ",
        "namespace ",
        "void ",
        "return ",
        "if (",
        "for (",
        "while (",
        "=>",
    ];

    for k in keywords {
        if text.contains(k) {
            score += 1;
        }
    }

    if text.contains(";") {
        score += 1;
    }
    if text.contains("{") && text.contains("}") {
        score += 1;
    }
    if text.contains("</") && text.contains(">") {
        score += 2;
    }

    if score >= 2 {
        return "code".to_string();
    }

    if trimmed.starts_with("{")
        && trimmed.ends_with("}")
        && text.contains(":")
        && text.contains("\"")
    {
        return "code".to_string();
    }

    "text".to_string()
}

pub fn contains_sensitive_info(text: &str, kinds: &[String], custom_rules: &[String]) -> bool {
    static PHONE_RE: OnceLock<Regex> = OnceLock::new();
    static IDCARD_RE: OnceLock<Regex> = OnceLock::new();
    static EMAIL_RE: OnceLock<Regex> = OnceLock::new();
    static SECRET_RE: OnceLock<Regex> = OnceLock::new();

    if text.len() > 5000 || text.starts_with("data:") {
        return false;
    }

    let has_kind = |k: &str| kinds.iter().any(|t| t == k);

    if has_kind("phone") {
        let re = PHONE_RE.get_or_init(|| {
            Regex::new(r"(?:\+?86)?[-\s\(]*1[3-9]\d{1}[-\s\)]*\d{4}[-\s]*\d{4}").unwrap()
        });
        if re.is_match(text) {
            return true;
        }
    }
    if has_kind("idcard") {
        let re = IDCARD_RE.get_or_init(|| {
            Regex::new(
                r"\b[1-9]\d{5}[1-9]\d{3}((0\d)|(1[0-2]))(([0|1|2]\d)|3[0-1])\d{3}([0-9Xx])\b",
            )
            .unwrap()
        });
        if re.is_match(text) {
            return true;
        }
    }
    if has_kind("email") {
        let re = EMAIL_RE
            .get_or_init(|| Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}").unwrap());
        if re.is_match(text) {
            return true;
        }
    }
    if has_kind("secret") {
        let re = SECRET_RE.get_or_init(|| Regex::new(r"(?ix)((?:sk|pk|ghp|gho|github_pat|AIza|AKIA|ya29)[-_][\w\-]{20,}|(?:password|secret|api[_-]?key|access[_-]?key|token|bearer)[\s:=]+[\w\-]{16,})").unwrap());
        if re.is_match(text) {
            return true;
        }
    }
    if has_kind("password") {
        if text.len() >= 8 && text.len() <= 64 && !text.contains(' ') && !text.contains('\n') {
            let has_upper = text.chars().any(|c| c.is_uppercase());
            let has_lower = text.chars().any(|c| c.is_lowercase());
            let has_digit = text.chars().any(|c| c.is_numeric());
            let has_special = text.chars().any(|c| !c.is_alphanumeric());
            if has_upper && has_lower && has_digit && has_special {
                return true;
            }
        }
    }

    for rule in custom_rules {
        if let Ok(re) = Regex::new(rule) {
            if re.is_match(text) {
                return true;
            }
        }
    }
    false
}

pub fn embed_local_images(html: &str) -> String {
    let re = match Regex::new(r#"(<img\s+[^>]*src=["'])([^"']+)(["'][^>]*>)"#) {
        Ok(r) => r,
        Err(_) => return html.to_string(),
    };

    re.replace_all(html, |caps: &regex::Captures| {
        let prefix = &caps[1];
        let src = &caps[2];
        let suffix = &caps[3];

        let is_local = src.starts_with("file://")
            || (src.len() > 2
                && src.chars().nth(1) == Some(':')
                && (src.chars().nth(2) == Some('\\') || src.chars().nth(2) == Some('/')));

        if is_local {
            let path_str = if src.starts_with("file://") {
                let raw_path = src.trim_start_matches("file://");
                if raw_path.starts_with('/') && raw_path.chars().nth(2) == Some(':') {
                    &raw_path[1..]
                } else {
                    raw_path
                }
            } else {
                src
            };

            let decoded_path = decode(path_str)
                .map(|p| p.into_owned())
                .unwrap_or(path_str.to_string());
            let clean_path = decoded_path
                .split('?')
                .next()
                .unwrap_or(&decoded_path)
                .split('#')
                .next()
                .unwrap_or(&decoded_path);

            let path = std::path::Path::new(clean_path);
            if path.exists() {
                if let Ok(data) = std::fs::read(path) {
                    let ext = path
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("png")
                        .to_lowercase();
                    let mime = match ext.as_str() {
                        "jpg" | "jpeg" => "image/jpeg",
                        "gif" => "image/gif",
                        "webp" => "image/webp",
                        "bmp" => "image/bmp",
                        "svg" => "image/svg+xml",
                        _ => "image/png",
                    };
                    let b64 = general_purpose::STANDARD.encode(&data);
                    return format!(
                        "{}{}{}",
                        prefix,
                        format!("data:{};base64,{}", mime, b64),
                        suffix
                    );
                }
            }
        }

        format!("{}{}{}", prefix, src, suffix)
    })
    .to_string()
}

pub fn process_local_images_in_html(html: &str, data_dir: &std::path::Path) -> String {
    let attachments_dir = data_dir.join("attachments");
    if !attachments_dir.exists() {
        let _ = std::fs::create_dir_all(&attachments_dir);
    }

    let re = match Regex::new(r#"(<img\s+[^>]*src=["'])([^"']+)(["'][^>]*>)"#) {
        Ok(r) => r,
        Err(_) => return html.to_string(),
    };

    re.replace_all(html, |caps: &regex::Captures| {
        let prefix = &caps[1];
        let src = &caps[2];
        let suffix = &caps[3];

        let is_local = src.starts_with("file://")
            || (src.len() > 2
                && src.chars().nth(1) == Some(':')
                && (src.chars().nth(2) == Some('\\') || src.chars().nth(2) == Some('/')));

        if is_local {
            let path_str = if src.starts_with("file://") {
                let raw_path = src.trim_start_matches("file://");
                if raw_path.starts_with('/') && raw_path.chars().nth(2) == Some(':') {
                    &raw_path[1..]
                } else {
                    raw_path
                }
            } else {
                src
            };

            let decoded_path = decode(path_str)
                .map(|p| p.into_owned())
                .unwrap_or(path_str.to_string());
            let clean_path = decoded_path
                .split('?')
                .next()
                .unwrap_or(&decoded_path)
                .split('#')
                .next()
                .unwrap_or(&decoded_path);
            let path = std::path::Path::new(clean_path);

            if path.starts_with(&attachments_dir) {
                return format!("{}{}{}", prefix, src, suffix);
            }

            if path.exists() {
                if let Ok(data) = std::fs::read(path) {
                    let mut hasher = std::collections::hash_map::DefaultHasher::new();
                    use std::hash::{Hash, Hasher};
                    data.hash(&mut hasher);
                    let hash = hasher.finish();

                    let ext = path
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("png")
                        .to_lowercase();
                    let new_filename = format!("img_{:x}.{}", hash, ext);
                    let new_path = attachments_dir.join(&new_filename);

                    if !new_path.exists() {
                        let _ = std::fs::write(&new_path, &data);
                    }

                    let new_src = new_path.to_string_lossy().replace('\\', "/");
                    let final_src = if new_src.starts_with('/') {
                        format!("file://{}", new_src)
                    } else {
                        format!("file:///{}", new_src)
                    };
                    return format!("{}{}{}", prefix, final_src, suffix);
                }
            }
        }

        format!("{}{}{}", prefix, src, suffix)
    })
    .to_string()
}

#[cfg(target_os = "windows")]
pub fn parse_cf_html(raw: &[u8]) -> Option<String> {
    enum HtmlEncoding {
        Utf8,
        Utf16Le,
    }

    let detect_encoding = |data: &[u8]| -> HtmlEncoding {
        if data.len() >= 2 && data[0] == 0xFF && data[1] == 0xFE {
            return HtmlEncoding::Utf16Le;
        }
        if data.len() % 2 == 0 {
            let zero_count = data.iter().filter(|b| **b == 0).count();
            if zero_count > data.len() / 4 {
                return HtmlEncoding::Utf16Le;
            }
        }
        HtmlEncoding::Utf8
    };

    let decode_bytes = |data: &[u8], encoding: &HtmlEncoding| -> String {
        match encoding {
            HtmlEncoding::Utf8 => String::from_utf8_lossy(data).to_string(),
            HtmlEncoding::Utf16Le => {
                let mut u16_buf = Vec::with_capacity(data.len() / 2);
                let mut i = 0;
                while i + 1 < data.len() {
                    u16_buf.push(u16::from_le_bytes([data[i], data[i + 1]]));
                    i += 2;
                }
                String::from_utf16_lossy(&u16_buf)
            }
        }
    };

    let encoding = detect_encoding(raw);
    let raw_str = decode_bytes(raw, &encoding);
    let mut start_fragment: Option<usize> = None;
    let mut end_fragment: Option<usize> = None;
    let mut start_html: Option<usize> = None;
    let mut end_html: Option<usize> = None;

    for line in raw_str.lines() {
        let trimmed = line.trim();
        if let Some(val) = trimmed.strip_prefix("StartFragment:") {
            if let Ok(pos) = val.trim().parse::<usize>() {
                start_fragment = Some(pos);
            }
        } else if let Some(val) = trimmed.strip_prefix("EndFragment:") {
            if let Ok(pos) = val.trim().parse::<usize>() {
                end_fragment = Some(pos);
            }
        } else if let Some(val) = trimmed.strip_prefix("StartHTML:") {
            if let Ok(pos) = val.trim().parse::<usize>() {
                start_html = Some(pos);
            }
        } else if let Some(val) = trimmed.strip_prefix("EndHTML:") {
            if let Ok(pos) = val.trim().parse::<usize>() {
                end_html = Some(pos);
            }
        }
        if trimmed.starts_with("<") {
            break;
        }
    }

    if let (Some(frag_s), Some(frag_e)) = (start_fragment, end_fragment) {
        if frag_s < frag_e && frag_e <= raw.len() {
            let fragment = decode_bytes(&raw[frag_s..frag_e], &encoding);
            let trimmed = fragment.trim();
            let wrapped_fragment =
                if (trimmed.contains("<tr") || trimmed.contains("<td") || trimmed.contains("<col"))
                    && !trimmed.to_lowercase().contains("<table")
                {
                    format!(
                        "<table style=\"border-collapse: collapse; min-width: 100%;\">{}</table>",
                        fragment
                    )
                } else {
                    fragment.clone()
                };

            if let (Some(html_s), Some(html_e)) = (start_html, end_html) {
                if html_s < html_e && html_e <= raw.len() {
                    let mut full_html = decode_bytes(&raw[html_s..html_e], &encoding);
                    let start_marker = "<!--StartFragment-->";
                    let end_marker = "<!--EndFragment-->";

                    if let Some(start_idx) = full_html.find(start_marker) {
                        let after_start = start_idx + start_marker.len();
                        if let Some(end_rel) = full_html[after_start..].find(end_marker) {
                            let end_idx = after_start + end_rel;
                            full_html = format!(
                                "{}{}{}",
                                &full_html[..after_start],
                                wrapped_fragment,
                                &full_html[end_idx..]
                            );
                        }
                    }

                    return Some(full_html);
                }
            }

            return Some(wrapped_fragment);
        }
    }

    let raw_text = raw_str.to_string();
    if let Some(start_idx) = raw_text.find("<!--StartFragment-->") {
        if let Some(end_idx) = raw_text.find("<!--EndFragment-->") {
            let fragment = &raw_text[start_idx + "<!--StartFragment-->".len()..end_idx];
            return Some(fragment.to_string());
        }
    }
    if raw_text.trim().starts_with("<") {
        return Some(raw_text);
    }
    None
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
        assert!(
            payload
                .html_content
                .as_ref()
                .is_some_and(|h| h.contains(HTML_TRUNCATION_SUFFIX))
        );
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
            payload
                .content
                .chars()
                .filter(|c| *c == '中')
                .count(),
            CONTENT_PREVIEW_MAX_CHARS
        );
    }
}

