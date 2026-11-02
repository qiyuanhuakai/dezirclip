//! Build a display copy of styled clipboard HTML without loading external resources.
use cssparser::{AtRuleParser, ParseError, Parser, ParserInput, ParserState, QualifiedRuleParser, StyleSheetParser, Token};
use dom_query::{Document, NodeData, NodeId, NodeRef};
use std::{collections::HashMap, rc::Rc};

const SHEET_INPUT_LIMIT: usize = 512 * 1024;
const DECLARATION_LIMIT: usize = 16 * 1024;
const RULE_STYLE_LIMIT: usize = 64 * 1024;
const SELECTOR_LIMIT: usize = 256;
const MATCH_WORK_LIMIT: usize = 1024 * 1024;
const OUTPUT_STYLE_LIMIT: usize = 4 * 1024 * 1024;

fn formatting_property(name: &str) -> bool {
    matches!(name, "color" | "background" | "background-color" | "font" | "font-family" | "font-size"
        | "font-weight" | "font-style" | "font-variant" | "font-stretch" | "font-feature-settings"
        | "line-height" | "letter-spacing" | "word-spacing" | "white-space" | "word-break" | "overflow-wrap"
        | "text-align" | "text-indent" | "text-transform" | "text-decoration" | "text-decoration-line"
        | "text-decoration-color" | "text-decoration-style" | "text-shadow" | "vertical-align"
        | "direction" | "writing-mode" | "list-style" | "list-style-type" | "list-style-position"
        | "border" | "border-collapse" | "border-spacing" | "border-radius" | "table-layout"
        | "width" | "height" | "min-width" | "max-width" | "padding" | "margin")
        || ["margin-", "padding-", "border-"].iter().any(|prefix| name.starts_with(prefix)
            && name.strip_prefix(prefix).is_some_and(|suffix| suffix.split('-').all(|part|
                matches!(part, "top" | "bottom" | "left" | "right" | "width" | "style" | "color" | "radius"))))
}

fn formatting_value<'i>(input: &mut Parser<'i, '_>, depth: u8) -> Result<(), ParseError<'i, ()>> {
    if depth > 10 { return Err(input.new_custom_error(())); }
    while !input.is_exhausted() {
        match input.next()? {
            Token::Ident(_) | Token::Hash(_) | Token::IDHash(_) | Token::QuotedString(_) | Token::Comma
            | Token::Number { .. } | Token::Percentage { .. } | Token::Dimension { .. } | Token::Delim('/' | '!' | '+' | '-' | '*') => {}
            Token::Function(name) if matches!(name.to_ascii_lowercase().as_str(),
                "rgb" | "rgba" | "hsl" | "hsla" | "hwb" | "lab" | "lch" | "oklab" | "oklch" | "color" | "color-mix"
                | "min" | "max" | "clamp" | "calc" | "linear-gradient" | "radial-gradient") => {
                input.parse_nested_block(|p| formatting_value(p, depth + 1))?;
            }
            _ => return Err(input.new_custom_error(())),
        }
    }
    Ok(())
}

fn declarations(input: &mut Parser<'_, '_>, budget: usize) -> String {
    let mut output = String::new();
    while !input.is_exhausted() {
        let result = input.parse_until_before(cssparser::Delimiter::Semicolon, |p| {
            let name = p.expect_ident_cloned()?.to_ascii_lowercase();
            if !formatting_property(&name) { return Err(p.new_custom_error(())); }
            p.expect_colon()?;
            let start = p.position();
            formatting_value(p, 0)?;
            let value = p.slice_from(start).trim();
            let bytes = name.len() + value.len() + 2;
            if bytes > DECLARATION_LIMIT || bytes > budget - output.len() { return Err(p.new_custom_error(())); }
            Ok((name, value))
        });
        if let Ok((name, value)) = result {
            output.push_str(&name);
            output.push(':');
            output.push_str(value);
            output.push(';');
        }
        let _ = input.expect_semicolon();
    }
    output
}

struct Sheet;
impl<'i> AtRuleParser<'i> for Sheet { type Prelude = (); type AtRule = (String, String); type Error = (); }
impl<'i> QualifiedRuleParser<'i> for Sheet {
    type Prelude = String;
    type QualifiedRule = (String, String);
    type Error = ();
    fn parse_prelude<'t>(&mut self, p: &mut Parser<'i, 't>) -> Result<String, ParseError<'i, ()>> {
        let start = p.position();
        p.expect_no_error_token()?;
        let prelude = p.slice_from(start).trim();
        if prelude.len() > DECLARATION_LIMIT { return Err(p.new_custom_error(())); }
        Ok(prelude.to_owned())
    }
    fn parse_block<'t>(&mut self, selector: String, _: &ParserState, p: &mut Parser<'i, 't>) -> Result<Self::QualifiedRule, ParseError<'i, ()>> {
        Ok((selector, declarations(p, RULE_STYLE_LIMIT)))
    }
}

// Read raw style text only when all its parts fit. Html5ever represents style contents as text nodes.
fn sheet_text(node: &NodeRef<'_>, remaining: usize) -> Option<String> {
    let mut text = String::new();
    for child in node.children_it(false) {
        let fits = child.query_or(true, |child| {
            if let NodeData::Text { contents } = &child.data {
                if contents.len() > remaining - text.len() { return false; }
                text.push_str(contents);
            }
            true
        });
        if !fits { return None; }
    }
    Some(text)
}

struct Rule {
    specificity: (usize, usize, usize),
    selector: String,
    style: Rc<String>,
}

fn collect_rules(doc: &Document) -> Vec<Rule> {
    let mut rules = Vec::new();
    let mut remaining = SHEET_INPUT_LIMIT;
    for sheet in doc.select("style").nodes() {
        let Some(text) = sheet_text(sheet, remaining) else { continue; };
        remaining -= text.len();
        let mut source = ParserInput::new(&text);
        let mut parser = Parser::new(&mut source);
        for (selectors, css) in StyleSheetParser::new(&mut parser, &mut Sheet).flatten() {
            if css.is_empty() { continue; }
            let style = Rc::new(css);
            for selector in selectors.split(',').map(str::trim) {
                if selector.is_empty() || selector.len() > 256 || !selector.bytes().all(|c| c.is_ascii_alphanumeric()
                    || b".#*_->+~ ".contains(&c)) { continue; }
                let ids = selector.matches('#').count();
                let classes = selector.matches('.').count();
                let tags = selector.split([' ', '>', '+', '~']).filter(|token| token.starts_with(|c: char| c.is_ascii_alphabetic())).count();
                rules.push(Rule { specificity: (ids, classes, tags), selector: selector.to_owned(), style: Rc::clone(&style) });
                if rules.len() == SELECTOR_LIMIT { return rules; }
            }
        }
    }
    rules
}

// Resolve ordinary document styles within this fragment only. Original selector names never enter the UI.
fn sheet_styles(doc: &Document) -> HashMap<NodeId, String> {
    let mut rules = collect_rules(doc);
    if rules.is_empty() { return HashMap::new(); }
    let nodes = doc.html_root().descendants_it().take(MATCH_WORK_LIMIT + 1).count().max(1);
    rules.sort_by_key(|rule| rule.specificity);
    let mut result = HashMap::<NodeId, String>::new();
    let mut bytes = 0;
    for rule in rules.iter().take(MATCH_WORK_LIMIT / nodes) {
        let css = rule.style.as_str();
        if let Some(matches) = doc.try_select(&rule.selector) {
            for node in matches.nodes() {
                bytes += css.len();
                if bytes > OUTPUT_STYLE_LIMIT { return result; }
                result.entry(node.id).or_default().push_str(css);
            }
        }
    }
    result
}

#[derive(Default)]
struct EmbeddedImages { count: usize, input_bytes: usize, output_bytes: usize }

impl EmbeddedImages {
    fn read(&mut self, src: &str, maximum: Option<u32>) -> Option<String> {
        // New markup never expands a file or network reference. Keep the same total picture budget
        // across normal images and the optional bitmap accompanying a copied HTML fragment.
        const ENCODED_LIMIT: usize = 44 * 1024 * 1024;
        if !src.starts_with("data:image/") { return None; }
        self.count += 1;
        self.input_bytes += src.len();
        if self.count > 64 || self.input_bytes > ENCODED_LIMIT || self.output_bytes >= ENCODED_LIMIT { return None; }
        let image = match maximum {
            Some(bound) => super::protected_resources::thumbnail_data_url(src, bound),
            None => super::protected_resources::image_data_url(src),
        }.ok()?;
        self.output_bytes += image.len();
        (self.output_bytes <= ENCODED_LIMIT).then_some(image)
    }
}

pub(crate) fn project(html: &str, maximum: Option<u32>) -> Option<String> {
    if html.len() > 48 * 1024 * 1024 { return None; }
    // Excel and browser selections may contain just the selected rows or cells.
    let fragment = html.trim_start();
    let first_tag = fragment.strip_prefix('<').unwrap_or("").split(|c: char| !c.is_ascii_alphanumeric()).next().unwrap_or("");
    let doc = match first_tag.to_ascii_lowercase().as_str() {
        "td" | "th" => Document::from(format!("<table><tr>{html}</tr></table>")),
        "tr" | "tbody" | "thead" | "tfoot" | "col" | "colgroup" | "caption" => Document::from(format!("<table>{html}</table>")),
        _ => Document::from(html),
    };
    doc.select("script,iframe,frame,object,embed,svg,math,template,noscript,textarea,link,base,meta").remove();
    let mut styles = sheet_styles(&doc);
    let mut remaining_style = OUTPUT_STYLE_LIMIT - styles.values().map(String::len).sum::<usize>();
    doc.select("style").remove();
    let body = doc.select_single("body");
    let body = body.nodes().first()?;
    let mut images = EmbeddedImages::default();
    for node in std::iter::once(*body).chain(body.descendants()) {
        if node.is_comment() { node.remove_from_parent(); continue; }
        let Some(name) = node.node_name() else { continue; };
        let mut style = styles.remove(&node.id).unwrap_or_default();
        if let Some(inline) = node.attr("style").filter(|v| v.len() <= RULE_STYLE_LIMIT && remaining_style > 0) {
            let mut input = ParserInput::new(&inline);
            let css = declarations(&mut Parser::new(&mut input), RULE_STYLE_LIMIT.min(remaining_style));
            remaining_style -= css.len();
            style.push_str(&css);
        }
        let image = if name.as_ref() == "img" {
            ["src", "data-src", "data-original", "data-lazy-src"].into_iter()
                .filter_map(|key| node.attr(key)).find_map(|src| images.read(&src, maximum))
        } else { None };
        node.retain_attrs(&["alt", "title", "lang", "dir", "align", "valign", "colspan", "rowspan", "span",
            "width", "height", "start", "type", "border", "cellpadding", "cellspacing", "color", "face", "size", "bgcolor"]);
        match name.as_ref() {
            "body" => node.rename("div"),
            "img" => match image {
                Some(src) => node.set_attr("src", &src),
                None => { let alt = node.attr("alt").unwrap_or_default(); node.rename("span"); node.set_text(alt); }
            },
            "p"|"br"|"div"|"span"|"b"|"strong"|"em"|"i"|"u"|"s"|"del"|"ins"|"sub"|"sup"|"mark"|"font"|"small"|"big"
            |"h1"|"h2"|"h3"|"h4"|"h5"|"h6"|"pre"|"code"|"kbd"|"samp"|"blockquote"|"q"|"hr"|"ol"|"ul"|"li"
            |"dl"|"dt"|"dd"|"table"|"colgroup"|"col"|"thead"|"tbody"|"tfoot"|"tr"|"td"|"th"|"caption"
            |"a"|"center"|"strike"|"address"|"abbr"|"cite"|"section"|"article"|"figure"|"figcaption"|"ruby"|"rt"|"rp"|"bdi"|"bdo"|"wbr" => {}
            _ => node.rename("span"),
        }
        if !style.is_empty() { node.set_attr("style", &style); }
    }
    let bitmap = html.rsplit_once("<!--TIEZ_RICH_IMAGE:").and_then(|(_, rest)| rest.split_once("-->"))
        .and_then(|(src, _)| images.read(src.trim(), maximum));
    if bitmap.is_none() && doc.select("img").is_empty() && body.text().trim().is_empty() { return None; }
    let mut output = body.html().to_string();
    if let Some(image) = bitmap { output.push_str(&format!("<!--TIEZ_RICH_IMAGE:{image}-->")); }
    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unmatched_selectors_share_one_style_and_oversize_values_never_enter_rules() {
        let selectors = (0..256).map(|i| format!(".absent{i}")).collect::<Vec<_>>().join(",");
        let input = |length| Document::from(format!("<style>{selectors}{{font-family:'{}';color:red}}</style><b>sample</b>", "a".repeat(length)));
        let doc = input(8000);
        let rules = collect_rules(&doc);
        assert_eq!(rules.len(), 256);
        assert!(rules.iter().all(|rule| Rc::ptr_eq(&rules[0].style, &rule.style)));
        assert!(sheet_styles(&doc).is_empty());
        let rules = collect_rules(&input(256 * 1024));
        assert_eq!(rules.len(), 256);
        assert!(rules.iter().all(|rule| rule.style.as_str() == "color:red;"));
        assert!(collect_rules(&input(8 * 1024 * 1024)).is_empty());
    }

    #[test]
    fn spreadsheet_cells_keep_the_table_style_span_and_alignment() {
        let html = project("<td colspan='2' align='right' style='background-color:#eee;color:red'><i>two cells</i></td>", Some(560)).unwrap();
        let doc = Document::from(html.as_str());
        let cell = doc.select("table tr td");
        assert_eq!(cell.attr("colspan").as_deref(), Some("2"));
        assert_eq!(cell.attr("align").as_deref(), Some("right"));
        assert!(cell.attr("style").unwrap().contains("background-color:#eee"));
        assert_eq!(doc.select("td i").text().as_ref(), "two cells");
    }

    #[test]
    fn active_content_and_external_image_fail_independently_of_neighboring_formatting() {
        let source = r#"<style>@import "https://bad.invalid";.strong{font-weight:700}</style>
            <script>do_not_render()</script><a href="javascript:alert(1)" onclick="bad()">link</a>
            <img src="file:///private.png" alt="diagram"><b class="strong" style="color: blue; background: u\72l(secret)">safe</b>"#;
        let html = project(source, None).unwrap();
        let doc = Document::from(html.as_str());
        assert!(doc.select("script,style,img,[href],[onclick]").is_empty());
        assert_eq!(doc.select("b").attr("style").as_deref(), Some("font-weight:700;color:blue;"));
        assert!(doc.text().contains("diagram"));
        assert!(!html.contains("private.png") && !html.contains("secret"));
    }
}
