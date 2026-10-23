//! Common history queries and output actions for the native terminal frontends.
use crate::domain::models::ClipboardEntry;
use crate::infrastructure::osc52::{self, Passthrough};
use crate::api::{HistoryQuery, HistoryService};
use std::io::Write;

#[derive(clap::ValueEnum, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Action {
    Print,
    #[default]
    Copy,
    Id,
    Insert,
    Osc52,
}

#[derive(Clone, Debug)]
pub struct BrowseOptions {
    pub query: String,
    pub kind: Option<String>,
    pub limit: i32,
    pub offset: i32,
    pub action: Action,
    pub passthrough: Passthrough,
}

pub fn query_rows(
    options: &BrowseOptions,
    repo: &HistoryService,
) -> Result<Vec<ClipboardEntry>, String> {
    if !(1..=200).contains(&options.limit) || options.offset < 0 {
        return Err("limit must be between 1 and 200; offset must be nonnegative".to_string());
    }
    super::access::query(repo, &HistoryQuery {
        limit: options.limit,
        offset: options.offset,
        kind: options.kind.as_deref(),
        metadata: true,
        search: (!options.query.trim().is_empty()).then_some((options.query.as_str(), crate::api::HistorySearchMode::Contains)),
        ..Default::default()
    }).map_err(Into::into)
}

pub fn display_text(value: &str, max_chars: usize) -> String {
    value.chars().filter_map(|c| {
        if c.is_whitespace() { return Some(' '); }
        if c.is_control() || matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{206f}' | '\u{feff}') {
            None
        } else { Some(c) }
    }).take(max_chars).collect()
}

pub fn copy_entry(id: i64, repo: &HistoryService) -> Result<(), String> {
    let permit = repo.privacy().authorize_selected_copy(id,None,
        crate::api::SensitiveAccess::Denied, None, super::access::now()).map_err(|error| error.to_string())?;
    let payload = repo.clipboard_payload_authorized(id, &permit, super::access::now()).map_err(|error| error.to_string())?;
    dzc_platform::runtime::standalone_clipboard_service().write(payload).map_err(Into::into)
}

pub fn apply_action(
    id: i64,
    options: &BrowseOptions,
    repo: &HistoryService,
) -> Result<(), String> {
    if options.action == Action::Copy {
        return copy_entry(id, repo);
    }
    if options.action == Action::Id {
        return writeln!(std::io::stdout().lock(), "{id}").map_err(|e| e.to_string());
    }
    let entry = super::get::resolve(&id.to_string(), false, repo)?;
    if !matches!(
        entry.content_type.as_str(),
        "text" | "code" | "url" | "html" | "rich_text"
    ) {
        return Err(
            "this action requires a text entry; use --action copy for images and files".to_string(),
        );
    }
    if options.action == Action::Osc52 {
        return osc52::write_text(&entry.content, options.passthrough);
    }
    write_text(
        &entry.content,
        options.action,
        &mut std::io::stdout().lock(),
    )
}

fn write_text(content: &str, action: Action, output: &mut impl Write) -> Result<(), String> {
    if action == Action::Insert && content.contains('\0') {
        return Err("shell input buffers cannot contain NUL bytes; use --action copy".to_string());
    }
    output
        .write_all(content.as_bytes())
        .and_then(|()| output.flush())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays_cannot_inject_rows_or_terminal_controls() {
        let text = display_text("one\n42\t\x1b[31m\u{202e}two\0", 100);
        assert_eq!(text, "one 42 [31mtwo");
        assert_eq!(display_text("你好🙂abc", 3), "你好🙂");
    }

    #[test]
    fn raw_output_is_exact_and_failed_insert_is_empty() {
        let payload = " 中文🙂\r\nsecond\r\n\n";
        let mut out = Vec::new();
        write_text(payload, Action::Print, &mut out).unwrap();
        assert_eq!(out, payload.as_bytes());
        out.clear();
        assert!(write_text("before\0after", Action::Insert, &mut out).is_err());
        assert!(out.is_empty());
    }
}
