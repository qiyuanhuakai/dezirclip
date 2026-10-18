//! Enumerate the resource attributes persisted by rich-text capture. This is a
//! deliberately bounded recognizer: anything outside its understood HTML subset
//! makes cleanup conservative rather than claiming to parse arbitrary HTML.

#[derive(Default)]
pub(super) struct HtmlReferences {
    pub values: Vec<String>,
    pub complete: bool,
}

pub(super) fn references(html: &str) -> HtmlReferences {
    let mut result = HtmlReferences {
        complete: true,
        ..Default::default()
    };
    let mut rest = html;
    while let Some(start) = rest.find('<') {
        rest = &rest[start + 1..];
        if let Some(comment) = rest.strip_prefix("!--") {
            let Some(end) = comment.find("-->") else {
                result.complete = false;
                break;
            };
            let body = &comment[..end];
            // Skip only conforming comments. HTML5 can close malformed comments
            // before this terminator, so their apparent contents may be live HTML.
            if body.starts_with('>')
                || body.starts_with("->")
                || body.contains("<!--")
                || body.contains("--!>")
                || body.ends_with("<!-")
                || body.contains('\0')
            {
                result.complete = false;
                return result;
            }
            if let Some(payload) = body.strip_prefix("TIEZ_RICH_IMAGE:") {
                result.values.push(payload.trim().to_owned());
            }
            rest = &comment[end + 3..];
            continue;
        }
        if rest.starts_with('/') {
            let Some(end) = rest.find('>') else {
                result.complete = false;
                break;
            };
            rest = &rest[end + 1..];
            continue;
        }
        let name_end = rest
            .find(|ch: char| !ch.is_ascii_alphanumeric() && !matches!(ch, ':' | '-'))
            .unwrap_or(rest.len());
        if name_end == 0 {
            result.complete = false;
            break;
        }
        let tag = rest[..name_end].to_ascii_lowercase();
        if !matches!(
            tag.as_str(),
            "html"
                | "head"
                | "body"
                | "p"
                | "div"
                | "span"
                | "br"
                | "hr"
                | "strong"
                | "b"
                | "i"
                | "em"
                | "u"
                | "s"
                | "del"
                | "pre"
                | "code"
                | "blockquote"
                | "ol"
                | "ul"
                | "li"
                | "table"
                | "thead"
                | "tbody"
                | "tfoot"
                | "tr"
                | "td"
                | "th"
                | "caption"
                | "colgroup"
                | "col"
                | "h1"
                | "h2"
                | "h3"
                | "h4"
                | "h5"
                | "h6"
                | "a"
                | "img"
        ) {
            result.complete = false;
        }
        rest = &rest[name_end..];
        loop {
            rest = rest.trim_start_matches(|ch: char| ch.is_ascii_whitespace());
            if let Some(tail) = rest.strip_prefix('>') {
                rest = tail;
                break;
            }
            if let Some(tail) = rest.strip_prefix("/>") {
                rest = tail;
                break;
            }
            let name_end = rest
                .find(|ch: char| {
                    ch.is_ascii_whitespace() || matches!(ch, '=' | '>' | '/' | '<' | '\'' | '"')
                })
                .unwrap_or(rest.len());
            if name_end == 0 {
                result.complete = false;
                return result;
            }
            let name = rest[..name_end].to_ascii_lowercase();
            rest = rest[name_end..].trim_start_matches(|ch: char| ch.is_ascii_whitespace());
            let resource = matches!(
                name.as_str(),
                "src" | "href" | "poster" | "data" | "background" | "xlink:href"
            );
            if !resource
                && !matches!(
                    name.as_str(),
                    "alt"
                        | "title"
                        | "class"
                        | "id"
                        | "width"
                        | "height"
                        | "rowspan"
                        | "colspan"
                        | "align"
                        | "valign"
                        | "border"
                        | "cellpadding"
                        | "cellspacing"
                        | "role"
                        | "lang"
                        | "dir"
                )
            {
                result.complete = false;
            }
            let Some(tail) = rest.strip_prefix('=') else {
                if resource {
                    result.complete = false;
                }
                continue;
            };
            rest = tail.trim_start_matches(|ch: char| ch.is_ascii_whitespace());
            let value;
            if let Some(quote) = rest.chars().next().filter(|ch| matches!(ch, '\'' | '"')) {
                rest = &rest[1..];
                let Some(end) = rest.find(quote) else {
                    result.complete = false;
                    return result;
                };
                value = &rest[..end];
                rest = &rest[end + 1..];
            } else {
                let end = rest
                    .find(|ch: char| ch.is_ascii_whitespace() || ch == '>')
                    .unwrap_or(rest.len());
                value = &rest[..end];
                if value.is_empty() || value.contains(['<', '\'', '"', '`', '=']) {
                    result.complete = false;
                }
                rest = &rest[end..];
            }
            if resource {
                match decode_attribute(value) {
                    Some(value) => result.values.push(value),
                    None => result.complete = false,
                }
            }
            if rest.is_empty() {
                result.complete = false;
                return result;
            }
        }
    }
    result
}

fn decode_attribute(mut value: &str) -> Option<String> {
    let mut decoded = String::new();
    while let Some(start) = value.find('&') {
        decoded.push_str(&value[..start]);
        value = &value[start + 1..];
        let end = value.find(';')?;
        let entity = &value[..end];
        let character = match entity {
            "amp" => '&',
            "quot" => '"',
            "apos" => '\'',
            "lt" => '<',
            "gt" => '>',
            _ => {
                let code = if let Some(hex) = entity
                    .strip_prefix("#x")
                    .or_else(|| entity.strip_prefix("#X"))
                {
                    u32::from_str_radix(hex, 16).ok()?
                } else {
                    entity.strip_prefix('#')?.parse::<u32>().ok()?
                };
                char::from_u32(code)?
            }
        };
        decoded.push(character);
        value = &value[end + 1..];
    }
    decoded.push_str(value);
    if decoded.chars().any(char::is_control) {
        return None;
    }
    Some(decoded)
}
