//! Native terminal history browser. Selection output is emitted after restoration.

use super::frontend::{self, BrowseOptions};
use crate::domain::models::ClipboardEntry;
use crate::infrastructure::repository::clipboard_repo::{
    ClipboardRepository, SqliteClipboardRepository,
};
use crate::infrastructure::repository::tag_repo::{SqliteTagRepository, TagRepository};
use crate::infrastructure::terminal_ui::Terminal;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use unicode_width::UnicodeWidthChar;

mod render;

const PREVIEW_CHAR_LIMIT: usize = 64_000;
const PREVIEW_LINE_LIMIT: usize = 2_000;

pub fn run(
    options: &BrowseOptions,
    repo: &SqliteClipboardRepository,
    tags: &SqliteTagRepository,
) -> Result<Option<i64>, String> {
    let mut browser = Browser::new(options.clone());
    // Validate queries before touching the terminal.
    browser.refresh(repo)?;
    let mut terminal = Terminal::enter()?;
    let mut renderer = render::Renderer::new(&terminal)?;
    let selection = browser.run(&mut terminal, &mut renderer, repo, tags);
    drop(renderer);
    let restored = terminal.restore();
    selection.and_then(|id| restored.map(|()| id))
}

struct Browser {
    options: BrowseOptions,
    entries: Vec<ClipboardEntry>,
    cursor: usize,
    preview_id: Option<i64>,
    preview: String,
    preview_offset: usize,
    status: String,
}

impl Browser {
    fn new(options: BrowseOptions) -> Self {
        Self {
            options,
            entries: Vec::new(),
            cursor: 0,
            preview_id: None,
            preview: String::new(),
            preview_offset: 0,
            status: String::new(),
        }
    }

    fn refresh(&mut self, repo: &SqliteClipboardRepository) -> Result<(), String> {
        let entries = frontend::query_rows(&self.options, repo)?;
        self.entries = entries;
        self.cursor = self.cursor.min(self.entries.len().saturating_sub(1));
        self.preview_id = None;
        self.preview_offset = 0;
        Ok(())
    }

    fn selected(&self) -> Option<&ClipboardEntry> {
        self.entries.get(self.cursor)
    }

    fn update_preview(&mut self, repo: &SqliteClipboardRepository) -> Result<(), String> {
        let Some(id) = self.selected().map(|entry| entry.id) else {
            self.preview.clear();
            self.preview
                .push_str("No entries. Press / to change the search.");
            return Ok(());
        };
        if self.preview_id != Some(id) {
            self.preview_id = Some(id);
            self.preview_offset = 0;
            let entry = repo
                .get_entry_by_id(id)?
                .ok_or_else(|| format!("Entry {id} no longer exists; press r to refresh"))?;
            self.preview = if entry.content_type == "image" {
                format!("[image]\n{}", entry.preview)
            } else {
                entry.content.chars().take(PREVIEW_CHAR_LIMIT).collect()
            };
        }
        Ok(())
    }

    fn run(
        &mut self,
        terminal: &mut Terminal,
        renderer: &mut render::Renderer,
        repo: &SqliteClipboardRepository,
        tags: &SqliteTagRepository,
    ) -> Result<Option<i64>, String> {
        loop {
            if let Err(error) = self.update_preview(repo) {
                self.preview = error;
            }
            self.draw(terminal, renderer, None)?;
            let Event::Key(key) = terminal.read()? else {
                continue;
            };
            if key.kind == KeyEventKind::Release {
                continue;
            }
            if matches!(key.code, KeyCode::Char(_))
                && key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                && !is_cancel(key)
            {
                continue;
            }
            if is_cancel(key) || key.code == KeyCode::Char('q') {
                return Ok(None);
            }
            if key.code == KeyCode::Enter {
                if let Some(entry) = self.selected() {
                    return Ok(Some(entry.id));
                }
                continue;
            }
            if let Err(error) = self.handle_key(key, terminal, renderer, repo, tags) {
                self.status = error;
            }
        }
    }

    fn handle_key(
        &mut self,
        key: KeyEvent,
        terminal: &mut Terminal,
        renderer: &mut render::Renderer,
        repo: &SqliteClipboardRepository,
        tags: &SqliteTagRepository,
    ) -> Result<(), String> {
        let id = self.selected().map(|entry| entry.id);
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                self.cursor = self
                    .cursor
                    .saturating_add(1)
                    .min(self.entries.len().saturating_sub(1));
            }
            KeyCode::Up | KeyCode::Char('k') => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.entries.len().saturating_sub(1),
            KeyCode::PageDown | KeyCode::Char(']') => {
                if self.entries.len() == self.options.limit as usize {
                    self.options.offset = self.options.offset.saturating_add(self.options.limit);
                    self.cursor = 0;
                    self.refresh(repo)?;
                }
            }
            KeyCode::PageUp | KeyCode::Char('[') => {
                self.options.offset = self
                    .options
                    .offset
                    .saturating_sub(self.options.limit)
                    .max(0);
                self.cursor = 0;
                self.refresh(repo)?;
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.preview_offset = self.preview_offset.saturating_add(3);
            }
            KeyCode::Left | KeyCode::Char('h') => {
                self.preview_offset = self.preview_offset.saturating_sub(3);
            }
            KeyCode::Char('/') => {
                if let Some(query) = self.prompt(terminal, renderer, "Search (empty = all): ")? {
                    self.options.query = query;
                    self.options.offset = 0;
                    self.cursor = 0;
                    self.refresh(repo)?;
                }
            }
            KeyCode::Char('r') => self.refresh(repo)?,
            KeyCode::Char('c') if id.is_some() => {
                let id = id.unwrap();
                frontend::copy_entry(id, repo)?;
                self.status = format!("Copied entry {id}");
            }
            KeyCode::Char('p') if id.is_some() => {
                let id = id.unwrap();
                let entry = repo
                    .get_entry_by_id(id)?
                    .ok_or_else(|| format!("Entry {id} no longer exists"))?;
                repo.toggle_pin(id, !entry.is_pinned)?;
                self.refresh(repo)?;
                self.cursor = self
                    .entries
                    .iter()
                    .position(|entry| entry.id == id)
                    .unwrap_or(0);
                self.status = format!(
                    "{} entry {id}",
                    if entry.is_pinned {
                        "Unpinned"
                    } else {
                        "Pinned"
                    }
                );
            }
            KeyCode::Char('t' | 'u') if id.is_some() => {
                let id = id.unwrap();
                let adding = key.code == KeyCode::Char('t');
                let verb = if adding { "add" } else { "remove" };
                if let Some(tag) = self.prompt(terminal, renderer, &format!("Tag to {verb}: "))? {
                    let tag = tag.trim();
                    if !tag.is_empty() {
                        let entry = repo
                            .get_entry_by_id(id)?
                            .ok_or_else(|| format!("Entry {id} no longer exists"))?;
                        tags.update_entry_tags(id, changed_tags(entry.tags, tag, adding))?;
                        self.refresh(repo)?;
                        self.cursor = self
                            .entries
                            .iter()
                            .position(|entry| entry.id == id)
                            .unwrap_or(0);
                        self.status = format!("Tag {verb} complete");
                    }
                }
            }
            KeyCode::Char('d') if id.is_some() => {
                let id = id.unwrap();
                if self
                    .prompt(
                        terminal,
                        renderer,
                        &format!("Delete entry {id}? Type yes: "),
                    )?
                    .as_deref()
                    == Some("yes")
                {
                    repo.delete(id, None)?;
                    self.refresh(repo)?;
                    self.status = format!("Deleted entry {id}");
                } else {
                    self.status = "Deletion cancelled".to_string();
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn draw(
        &mut self,
        terminal: &Terminal,
        renderer: &mut render::Renderer,
        prompt: Option<&render::Prompt<'_>>,
    ) -> Result<(), String> {
        self.preview_offset = renderer.draw(
            terminal,
            &render::View {
                options: &self.options,
                entries: &self.entries,
                cursor: self.cursor,
                preview: &self.preview,
                preview_offset: self.preview_offset,
                status: &self.status,
            },
            prompt,
        )?;
        Ok(())
    }

    fn prompt(
        &mut self,
        terminal: &mut Terminal,
        renderer: &mut render::Renderer,
        label: &str,
    ) -> Result<Option<String>, String> {
        let mut value = String::new();
        loop {
            self.draw(
                terminal,
                renderer,
                Some(&render::Prompt {
                    label,
                    value: &value,
                }),
            )?;
            match terminal.read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => {
                    if is_cancel(key) {
                        return Ok(None);
                    }
                    match key.code {
                        KeyCode::Enter => return Ok(Some(value)),
                        KeyCode::Backspace => {
                            value.pop();
                        }
                        KeyCode::Char(character)
                            if !key
                                .modifiers
                                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                                && value.chars().count() < 1024 =>
                        {
                            value.push(character);
                        }
                        _ => {}
                    }
                }
                Event::Paste(text) => {
                    let room = 1024usize.saturating_sub(value.chars().count());
                    value.push_str(&frontend::display_text(&text, room));
                }
                _ => {}
            }
        }
    }
}

fn action_name(action: frontend::Action) -> &'static str {
    match action {
        frontend::Action::Print => "print",
        frontend::Action::Copy => "copy",
        frontend::Action::Id => "id",
        frontend::Action::Insert => "insert",
        frontend::Action::Osc52 => "OSC52",
    }
}

fn is_cancel(key: KeyEvent) -> bool {
    key.code == KeyCode::Esc
        || (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c'))
}

fn changed_tags(mut tags: Vec<String>, tag: &str, adding: bool) -> Vec<String> {
    if adding {
        if !tags.iter().any(|existing| existing == tag) {
            tags.push(tag.to_string());
        }
    } else {
        tags.retain(|existing| existing != tag);
    }
    tags
}

fn fit_columns(text: &str, columns: usize) -> String {
    let mut width = 0usize;
    text.chars()
        .take_while(|character| {
            width = width.saturating_add(character.width().unwrap_or(0));
            width <= columns
        })
        .collect()
}

fn preview_lines(text: &str, columns: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut width = 0usize;
    let mut characters = text.chars().take(PREVIEW_CHAR_LIMIT).peekable();
    while let Some(character) = characters.next() {
        if character == '\n' || character == '\r' {
            if character == '\r' && characters.peek() == Some(&'\n') {
                characters.next();
            }
            lines.push(std::mem::take(&mut line));
            width = 0;
        } else {
            let clean = if character == '\t' {
                "    ".to_string()
            } else {
                frontend::display_text(&character.to_string(), 1)
            };
            for character in clean.chars() {
                let char_width = character.width().unwrap_or(0);
                if char_width > columns {
                    continue;
                }
                if width + char_width > columns {
                    lines.push(std::mem::take(&mut line));
                    width = 0;
                }
                line.push(character);
                width += char_width;
            }
        }
        if lines.len() >= PREVIEW_LINE_LIMIT {
            lines.truncate(PREVIEW_LINE_LIMIT);
            return lines;
        }
    }
    if lines.len() < PREVIEW_LINE_LIMIT {
        lines.push(line);
    }
    if lines.iter().all(|line| line.is_empty()) {
        vec!["(empty)".to_string()]
    } else {
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_sanitizes_controls_preserving_line_breaks_and_unicode_columns() {
        let lines = preview_lines("你🙂ab\r\n\tend\x1b]52;c;bad\x07\0\u{202e}", 4);
        assert_eq!(&lines[..3], &["你🙂", "ab", "    "]);
        assert!(lines
            .iter()
            .all(|line| line.chars().all(|c| !c.is_control() && c != '\u{202e}')));
        assert!(lines.iter().all(|line| line
            .chars()
            .map(|c| c.width().unwrap_or(0))
            .sum::<usize>()
            <= 4));
        assert_eq!(preview_lines("a\rb\r\nc\nd", 8), ["a", "b", "c", "d"]);
    }

    #[test]
    fn truncation_respects_utf8_wide_and_combining_characters() {
        assert_eq!(fit_columns("你🙂abc", 3), "你");
        assert_eq!(fit_columns("e\u{301}你z", 3), "e\u{301}你");
        assert_eq!(fit_columns("你", 0), "");
        assert_eq!(
            preview_lines("\n".repeat(3000).as_str(), 10).len(),
            PREVIEW_LINE_LIMIT
        );
        assert_eq!(preview_lines("", 10), ["(empty)"]);
    }

    #[test]
    fn tag_changes_are_exact_idempotent_and_do_not_execute_values() {
        let tag = "$(echo unsafe); 'quoted' 中文";
        let tags = changed_tags(vec!["work".to_string()], tag, true);
        assert_eq!(changed_tags(tags.clone(), tag, true), tags);
        assert_eq!(changed_tags(tags, tag, false), ["work"]);
        assert_eq!(
            changed_tags(vec!["work".to_string()], "absent", false),
            ["work"]
        );
    }

    #[test]
    fn cancellation_is_explicit_and_plain_c_remains_copy() {
        assert!(is_cancel(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
        assert!(is_cancel(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL
        )));
        assert!(!is_cancel(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::NONE
        )));
    }
}
