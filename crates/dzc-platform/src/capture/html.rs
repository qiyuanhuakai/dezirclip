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
        let content_start = start_idx + "<!--StartFragment-->".len();
        if let Some(end_relative) = raw_text[content_start..].find("<!--EndFragment-->") {
            let fragment = &raw_text[content_start..content_start + end_relative];
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

    #[test]
    fn byte_offsets_preserve_full_html_styles_and_non_ascii_fragment() {
        let html = "<html><head><style>p {color:red}</style></head><body><!--StartFragment--><p>文本</p><!--EndFragment--></body></html>";
        let header = |start_html, end_html, start_fragment, end_fragment| {
            format!("Version:0.9\r\nStartHTML:{start_html:010}\r\nEndHTML:{end_html:010}\r\nStartFragment:{start_fragment:010}\r\nEndFragment:{end_fragment:010}\r\n")
        };
        let start = header(0, 0, 0, 0).len();
        let start_fragment =
            start + html.find("<!--StartFragment-->").unwrap() + "<!--StartFragment-->".len();
        let end_fragment = start + html.find("<!--EndFragment-->").unwrap();
        let raw = format!(
            "{}{html}",
            header(start, start + html.len(), start_fragment, end_fragment)
        );
        assert_eq!(parse_cf_html(raw.as_bytes()).as_deref(), Some(html));
    }

    #[test]
    fn fallback_markers_must_be_ordered_and_plain_text_has_no_html() {
        assert_eq!(
            parse_cf_html(b"prefix<!--StartFragment--><b>text</b><!--EndFragment-->suffix")
                .as_deref(),
            Some("<b>text</b>")
        );
        let malformed = b"<!--EndFragment--><html><!--StartFragment-->";
        assert_eq!(
            parse_cf_html(malformed),
            Some(String::from_utf8_lossy(malformed).into_owned())
        );
        assert_eq!(parse_cf_html(b"not HTML"), None);
    }
}
