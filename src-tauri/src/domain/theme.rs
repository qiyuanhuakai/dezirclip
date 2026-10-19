use cssparser::{BasicParseErrorKind, ParseError, Parser, ParserInput, Token};
use serde::{Deserialize, Serialize};

pub const MAX_THEME_BYTES: usize = 256 * 1024;
pub const MAX_CUSTOM_THEMES: usize = 32;
pub const MAX_CSS_DEPTH: usize = 128;
pub const BUILTIN_THEMES: &[&str] = &["mica", "acrylic", "retro", "macos", "scifi", "liquid-glass"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemePackage {
    pub format: String,
    pub version: u32,
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub base: String,
    pub css: String,
}

fn check_label(value: &str, maximum: usize, field: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.chars().count() > maximum
        || value.chars().any(char::is_control)
    {
        return Err(format!(
            "theme {field} must contain 1..{maximum} printable characters"
        ));
    }
    Ok(())
}

pub fn valid_theme_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 48
        && id.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
}

fn inspect_css<'i>(
    input: &mut Parser<'i, '_>,
    css: &'i str,
    depth: usize,
    declarations: bool,
    closing: Option<u8>,
) -> Result<bool, ParseError<'i, String>> {
    if depth > MAX_CSS_DEPTH {
        return Err(input.new_custom_error("theme CSS nesting exceeds 128 blocks".to_string()));
    }
    let mut has_content = false;
    let mut statement_start = true;
    loop {
        let start = input.position();
        let token = match input.next_including_whitespace_and_comments() {
            Ok(token) => token.clone(),
            Err(error) if matches!(error.kind, BasicParseErrorKind::EndOfInput) => {
                if closing.is_some()
                    && css.as_bytes().get(input.position().byte_index()).copied() != closing
                {
                    return Err(
                        input.new_custom_error("theme CSS contains an unclosed block".to_string())
                    );
                }
                break;
            }
            Err(error) => return Err(error.into()),
        };
        if token.is_parse_error() {
            return Err(input.new_custom_error("theme CSS contains an invalid token".to_string()));
        }
        match token {
            Token::WhiteSpace(_) => continue,
            Token::Comment(value) => {
                if !input.slice_from(start).ends_with("*/") {
                    return Err(input
                        .new_custom_error("theme CSS contains an unclosed comment".to_string()));
                }
                let marker = value.trim_start();
                let directive = marker
                    .strip_prefix('#')
                    .or_else(|| marker.strip_prefix('@'))
                    .and_then(|value| value.trim_start().split_once('='))
                    .map(|(name, _)| name.trim());
                if directive.is_some_and(|name| {
                    name.eq_ignore_ascii_case("sourceURL")
                        || name.eq_ignore_ascii_case("sourceMappingURL")
                }) {
                    return Err(input.new_custom_error(
                        "theme CSS cannot reference source maps or source URLs".to_string(),
                    ));
                }
                continue;
            }
            Token::QuotedString(_) => {
                let raw = input.slice_from(start).as_bytes();
                let slashes = raw[..raw.len().saturating_sub(1)]
                    .iter()
                    .rev()
                    .take_while(|byte| **byte == b'\\')
                    .count();
                if raw.len() < 2 || raw.first() != raw.last() || slashes % 2 != 0 {
                    return Err(
                        input.new_custom_error("theme CSS contains an unclosed string".to_string())
                    );
                }
            }
            Token::AtKeyword(name)
                if [
                    "import",
                    "font-face",
                    "namespace",
                    "document",
                    "-moz-document",
                ]
                .iter()
                .any(|blocked| name.eq_ignore_ascii_case(blocked)) =>
            {
                return Err(input.new_custom_error(format!("theme CSS cannot contain @{name}")));
            }
            Token::UnquotedUrl(_) => {
                return Err(
                    input.new_custom_error("theme CSS cannot contain resource URLs".to_string())
                );
            }
            Token::Function(name) => {
                if [
                    "url",
                    "src",
                    "image",
                    "image-set",
                    "-webkit-image-set",
                    "expression",
                ]
                .iter()
                .any(|blocked| name.eq_ignore_ascii_case(blocked))
                {
                    return Err(input.new_custom_error(format!(
                        "theme CSS cannot contain the resource function {name}()"
                    )));
                }
                input.parse_nested_block(|nested| {
                    inspect_css(nested, css, depth + 1, false, Some(b')'))
                })?;
            }
            Token::CurlyBracketBlock => {
                input.parse_nested_block(|nested| {
                    inspect_css(nested, css, depth + 1, true, Some(b'}'))
                })?;
                statement_start = true;
                has_content = true;
                continue;
            }
            Token::ParenthesisBlock => {
                input.parse_nested_block(|nested| {
                    inspect_css(nested, css, depth + 1, false, Some(b')'))
                })?;
            }
            Token::SquareBracketBlock => {
                input.parse_nested_block(|nested| {
                    inspect_css(nested, css, depth + 1, false, Some(b']'))
                })?;
            }
            Token::Ident(name)
                if declarations
                    && statement_start
                    && ["behavior", "-moz-binding"]
                        .iter()
                        .any(|blocked| name.eq_ignore_ascii_case(blocked))
                    && input.try_parse(|parser| parser.expect_colon()).is_ok() =>
            {
                return Err(
                    input.new_custom_error(format!("theme CSS cannot contain the property {name}"))
                );
            }
            Token::Semicolon => {
                statement_start = true;
                has_content = true;
                continue;
            }
            _ => {}
        }
        has_content = true;
        statement_start = false;
    }
    Ok(has_content)
}

/// Inspect decoded CSS tokens without rewriting the original stylesheet.
pub fn validate_theme_css(css: &str) -> Result<(), String> {
    if css.len() > MAX_THEME_BYTES {
        return Err("theme CSS exceeds 256 KiB".into());
    }
    let mut source = ParserInput::new(css);
    let mut parser = Parser::new(&mut source);
    let has_content = inspect_css(&mut parser, css, 0, false, None)
        .map_err(|error| format!("invalid theme CSS: {error:?}"))?;
    if !has_content {
        return Err("theme CSS must contain stylesheet rules".into());
    }
    Ok(())
}

impl ThemePackage {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_THEME_BYTES {
            return Err("theme file exceeds 256 KiB".into());
        }
        let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
        let mut package: Self = serde_json::from_slice(bytes)
            .map_err(|error| format!("invalid theme JSON: {error}"))?;
        package.validate()?;
        Ok(package)
    }

    pub fn validate(&mut self) -> Result<(), String> {
        if self.format != "dezirclip-theme" || self.version != 1 {
            return Err("unsupported theme format or version".into());
        }
        if !valid_theme_id(&self.id) {
            return Err("theme id must be 1..48 lowercase letters, digits or hyphens".into());
        }
        self.name = self.name.trim().to_string();
        check_label(&self.name, 80, "name")?;
        self.author = self
            .author
            .take()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        self.description = self
            .description
            .take()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        if let Some(author) = &self.author {
            check_label(author, 80, "author")?;
        }
        if let Some(description) = &self.description {
            check_label(description, 240, "description")?;
        }
        if !BUILTIN_THEMES.contains(&self.base.as_str()) {
            return Err("unknown base theme".into());
        }
        validate_theme_css(&self.css)?;
        if serde_json::to_vec(self)
            .map_err(|error| error.to_string())?
            .len()
            > MAX_THEME_BYTES
        {
            return Err("theme file exceeds 256 KiB".into());
        }
        Ok(())
    }

    pub fn export_bytes(&self) -> Result<Vec<u8>, String> {
        let pretty = serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?;
        if pretty.len() <= MAX_THEME_BYTES {
            return Ok(pretty);
        }
        let compact = serde_json::to_vec(self).map_err(|error| error.to_string())?;
        if compact.len() > MAX_THEME_BYTES {
            return Err("theme file exceeds 256 KiB".into());
        }
        Ok(compact)
    }
}

pub fn parse_theme_library(raw: Option<&str>) -> Result<Vec<ThemePackage>, String> {
    let raw = raw.unwrap_or("[]");
    if raw.len() > MAX_THEME_BYTES * MAX_CUSTOM_THEMES + MAX_CUSTOM_THEMES + 1 {
        return Err("saved theme library exceeds its size limit".into());
    }
    let mut themes: Vec<ThemePackage> = serde_json::from_str(raw)
        .map_err(|error| format!("invalid saved theme library: {error}"))?;
    if themes.len() > MAX_CUSTOM_THEMES {
        return Err("theme library exceeds 32 themes".into());
    }
    let mut ids = std::collections::HashSet::new();
    for theme in &mut themes {
        theme.validate()?;
        if !ids.insert(theme.id.clone()) {
            return Err("duplicate theme id in library".into());
        }
    }
    Ok(themes)
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn sample() -> ThemePackage {
        ThemePackage {
            format: "dezirclip-theme".into(),
            version: 1,
            id: "sample".into(),
            name: "示例".into(),
            author: None,
            description: None,
            base: "mica".into(),
            css: ":root.light-mode{--dc-accent:#156c44}.dark-mode .dc-tag-grid{grid-template-columns:1fr 1fr}".into(),
        }
    }

    #[test]
    fn preserves_complete_css_and_normalizes_only_metadata() {
        let mut theme = sample();
        theme.name = " 示例 ".into();
        theme.author = Some(" ".into());
        theme.css = "  :root { --dc-accent: #ABCDEF; }\r\n.dark-mode .dc-tag-entry:hover { transform:scale(1.01) }  ".into();
        let original = theme.css.clone();
        theme.validate().unwrap();
        assert_eq!(theme.css, original);
        assert_eq!(theme.author, None);
        assert_eq!(theme.name, "示例");
        let mut bytes = vec![0xef, 0xbb, 0xbf];
        bytes.extend(theme.export_bytes().unwrap());
        assert_eq!(ThemePackage::parse(&bytes).unwrap(), theme);
    }

    #[test]
    fn allows_layout_materials_states_modes_and_nested_rules() {
        let css = r#"
            /* A resource word in documentation, url(https://example.invalid), is text. */
            @layer custom {
              @media (prefers-color-scheme: dark) {
                :root.dark-mode { --background: linear-gradient(120deg,#123,#456); }
              }
              @supports (display: grid) {
                .dc-tag-grid { display:grid; grid-template-columns:repeat(2,minmax(0,1fr)); }
                .dc-tag-entry { backdrop-filter:blur(12px); &:hover { transform:translateY(-1px); } }
              }
            }
            @keyframes pulse { from { opacity:.8 } to { opacity:1 } }
            [data-label="url(https://example.invalid)"]::after { content:"@import is only text"; }
            .behavior:hover { color:var(--dc-text-primary,#abc); width:calc(100% - 2px); }
        "#;
        validate_theme_css(css).unwrap();
    }

    #[test]
    fn rejects_resource_rules_functions_urls_and_escaped_spellings() {
        for css in [
            "@import 'https://example.invalid/theme.css';",
            r#"@\69mport/**/"https://example.invalid/theme.css";"#,
            "@FONT-FACE { font-family:Example; src:local(Arial) }",
            r#"@f\6f nt-face { font-family:Example; }"#,
            "@namespace x 'https://example.invalid/ns';",
            "@document domain('example.invalid') {body{color:red}}",
            "@-moz-document domain('example.invalid') {body{color:red}}",
            ".x{background:url(https://example.invalid/x)}",
            r#".x{background:u\72l('https://example.invalid/x')}"#,
            ".x{--resource:URL('file:///private/image.png')}",
            ".x{background:var(--x,src('https://example.invalid/x'))}",
            ".x{background:image('https://example.invalid/x',red)}",
            ".x{background:IMAGE-SET('https://example.invalid/x' 1x)}",
            r#".x{background:-webkit-image-\73 et('https://example.invalid/x' 1x)}"#,
            ".x{color:expression(alert(1))}",
            ".x{background:url(data:image/png;base64,AAAA)}",
            ".x{mask:url(#local-symbol)}",
            "/*# sourceMappingURL=https://example.invalid/x.map */ .x{color:red}",
            "/*# sourceURL=https://example.invalid/x.css */ .x{color:red}",
            ".x{color:red} /* @ sourceMappingURL = https://example.invalid/x.map */",
        ] {
            assert!(validate_theme_css(css).is_err(), "accepted {css}");
        }
    }

    #[test]
    fn rejects_binding_declarations_without_rejecting_identifiers_in_text() {
        for css in [
            ".x{behavior:'https://example.invalid/x'}",
            r#".x{b\65 havior/**/:none}"#,
            ".x{color:red;BEHAVIOR:inherit}",
            ".x{-moz-binding:initial}",
            "@layer a{@supports(display:grid){.x{color:red;-moz-binding:inherit}}}",
        ] {
            assert!(validate_theme_css(css).is_err(), "accepted {css}");
        }
        validate_theme_css(".behavior:hover{--behavior:'-moz-binding';content:'behavior:'}")
            .unwrap();
    }

    #[test]
    fn rejects_bad_tokens_and_bounded_nesting() {
        for css in [
            ".x{color:'bad\nstring'}",
            ".x{color:red)}",
            ".x{background:url(\"bad\nurl)}",
        ] {
            assert!(validate_theme_css(css).is_err(), "accepted {css}");
        }
        for css in [
            ".x{color:red",
            "@media all{.x{color:red}",
            ".x{width:calc(1px + 2px",
            "[x=\"value\"",
            ".x{content:\"unclosed",
            r#".x{content:"unclosed\""#,
            "/*unclosed",
        ] {
            assert!(validate_theme_css(css).is_err(), "accepted {css}");
        }
        assert!(validate_theme_css(" /* comments only */ ").is_err());
        let nested = format!(
            "{}color:red{}",
            "{".repeat(MAX_CSS_DEPTH + 1),
            "}".repeat(MAX_CSS_DEPTH + 1)
        );
        assert!(validate_theme_css(&nested).is_err());
    }

    #[test]
    fn rejects_old_contract_unknown_fields_invalid_metadata_and_library_duplicates() {
        let mut value = serde_json::to_value(sample()).unwrap();
        value["modes"] = serde_json::json!({"light":{},"dark":{}});
        assert!(ThemePackage::parse(&serde_json::to_vec(&value).unwrap()).is_err());
        value.as_object_mut().unwrap().remove("modes");
        value["url"] = "https://example.invalid/theme.css".into();
        assert!(ThemePackage::parse(&serde_json::to_vec(&value).unwrap()).is_err());
        for id in ["../escape", "-sample", "sample-", "Sample"] {
            let mut theme = sample();
            theme.id = id.into();
            assert!(theme.validate().is_err());
        }
        let mut theme = sample();
        theme.name = "bad\nname".into();
        assert!(theme.validate().is_err());
        let mut theme = sample();
        theme.base = "unknown".into();
        assert!(theme.validate().is_err());
        assert!(parse_theme_library(Some(
            &serde_json::to_string(&vec![sample(), sample()]).unwrap()
        ))
        .is_err());
        assert!(parse_theme_library(Some(
            &serde_json::to_string(&vec![sample(); MAX_CUSTOM_THEMES + 1]).unwrap()
        ))
        .is_err());
    }

    #[test]
    fn exact_size_boundary_exports_a_reimportable_package() {
        let mut theme = sample();
        let overhead = serde_json::to_vec(&theme).unwrap().len();
        theme.css.push_str(&" ".repeat(MAX_THEME_BYTES - overhead));
        theme.validate().unwrap();
        let bytes = theme.export_bytes().unwrap();
        assert_eq!(bytes.len(), MAX_THEME_BYTES);
        assert_eq!(ThemePackage::parse(&bytes).unwrap(), theme);
        theme.css.push(' ');
        assert!(theme.validate().is_err());
        assert!(ThemePackage::parse(&vec![b' '; MAX_THEME_BYTES + 1]).is_err());
    }

    #[test]
    fn published_skill_starter_uses_the_native_import_contract() {
        let theme = ThemePackage::parse(include_bytes!(
            "../../../skills/dezirclip-theme/assets/forest.theme.json"
        ))
        .unwrap();
        assert_eq!(theme.id, "forest");
        assert!(theme.css.contains(".theme-custom"));
        assert!(theme.css.contains(".dark-mode"));
    }
}
