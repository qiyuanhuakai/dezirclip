//! Widget rendering is separate from history mutations and terminal input.

use super::{action_name, fit_columns, frontend, preview_lines, BrowseOptions, ClipboardEntry};
use crate::infrastructure::terminal_ui::Terminal as InputTerminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, List, ListItem, ListState, Padding, Paragraph, Scrollbar,
    ScrollbarOrientation, ScrollbarState,
};
use ratatui::{Frame, Terminal, TerminalOptions, Viewport};
use std::fs::File;
use unicode_width::UnicodeWidthStr;

const BACKGROUND: Color = Color::Rgb(12, 17, 26);
const SURFACE: Color = Color::Rgb(17, 26, 39);
const BORDER: Color = Color::Rgb(43, 60, 79);
const TEXT: Color = Color::Rgb(218, 231, 244);
const MUTED: Color = Color::Rgb(126, 150, 173);
const ACCENT: Color = Color::Rgb(103, 231, 199);
const BLUE: Color = Color::Rgb(146, 184, 255);
const GOLD: Color = Color::Rgb(244, 198, 116);
const SELECTED: Color = Color::Rgb(28, 57, 64);

pub(super) struct Renderer {
    terminal: Terminal<CrosstermBackend<File>>,
    area: Rect,
    list: ListState,
}

pub(super) struct View<'a> {
    pub options: &'a BrowseOptions,
    pub entries: &'a [ClipboardEntry],
    pub cursor: usize,
    pub preview: &'a str,
    pub preview_offset: usize,
    pub status: &'a str,
}

pub(super) struct Prompt<'a> {
    pub label: &'a str,
    pub value: &'a str,
}

impl Renderer {
    pub fn new(input: &InputTerminal) -> Result<Self, String> {
        let (width, height) = input.size()?;
        let area = Rect::new(0, 0, width, height);
        let backend = CrosstermBackend::new(input.output_clone()?);
        let terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(area),
            },
        )
        .map_err(|error| format!("tui cannot initialize the display: {error}"))?;
        Ok(Self {
            terminal,
            area,
            list: ListState::default(),
        })
    }

    pub fn draw(
        &mut self,
        input: &InputTerminal,
        view: &View<'_>,
        prompt: Option<&Prompt<'_>>,
    ) -> Result<usize, String> {
        let (width, height) = input.size()?;
        let area = Rect::new(0, 0, width, height);
        if self.area != area {
            self.terminal
                .resize(area)
                .map_err(|error| format!("tui cannot resize the display: {error}"))?;
            self.area = area;
        }
        let mut offset = view.preview_offset;
        let list = &mut self.list;
        self.terminal
            .draw(|frame| {
                offset = paint(frame, view, prompt, list);
            })
            .map_err(|error| format!("tui cannot draw the display: {error}"))?;
        Ok(offset)
    }
}

fn panel(title: Line<'_>) -> Block<'_> {
    Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER))
        .style(Style::default().bg(SURFACE).fg(TEXT))
        .title(title)
        .padding(Padding::horizontal(1))
}

fn label(value: &str) -> Span<'_> {
    Span::styled(
        value,
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    )
}

fn muted(value: impl Into<String>) -> Span<'static> {
    Span::styled(value.into(), Style::default().fg(MUTED))
}

fn clean(value: &str, maximum: usize) -> String {
    frontend::display_text(value, maximum)
}

pub(super) fn paint(
    frame: &mut Frame<'_>,
    view: &View<'_>,
    prompt: Option<&Prompt<'_>>,
    list: &mut ListState,
) -> usize {
    let area = frame.area();
    if area.is_empty() {
        return 0;
    }
    frame.render_widget(
        Block::default().style(Style::default().bg(BACKGROUND).fg(TEXT)),
        area,
    );
    if area.width < 36 || area.height < 12 {
        let text = Text::from(vec![
            Line::from(label("  DZC / DezirClip")),
            Line::from(""),
            Line::from("  A little more room, please."),
            Line::from(muted("  Resize to 36 × 12 or larger.")),
            Line::from(muted("  q or Esc to close")),
        ]);
        frame.render_widget(Paragraph::new(text), area);
        if let Some(prompt) = prompt {
            draw_prompt(frame, prompt);
        }
        return view.preview_offset;
    }
    let inner = area.inner(Margin {
        horizontal: 1,
        vertical: u16::from(area.height >= 22),
    });
    let sections = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(if area.height < 20 { 1 } else { 3 }),
        Constraint::Min(3),
        Constraint::Length(2),
    ])
    .spacing(u16::from(area.height >= 22))
    .split(inner);
    draw_masthead(frame, sections[0], view);
    draw_search(frame, sections[1], view);
    let body = sections[2];
    let (history, preview) = if body.width >= 80 {
        let panes = Layout::horizontal([Constraint::Percentage(42), Constraint::Percentage(58)])
            .spacing(1)
            .split(body);
        (panes[0], panes[1])
    } else {
        let panes = Layout::vertical([Constraint::Percentage(43), Constraint::Percentage(57)])
            .spacing(1)
            .split(body);
        (panes[0], panes[1])
    };
    draw_history(frame, history, view, list);
    let offset = draw_preview(frame, preview, view);
    draw_footer(frame, sections[3], view);
    if let Some(prompt) = prompt {
        draw_prompt(frame, prompt);
    }
    offset
}

fn draw_masthead(frame: &mut Frame<'_>, area: Rect, view: &View<'_>) {
    let columns = Layout::horizontal([
        Constraint::Min(18),
        Constraint::Length(if area.width >= 72 { 28 } else { 0 }),
    ])
    .split(area);
    let heading = Line::from(vec![
        Span::styled(
            " DZC ",
            Style::default()
                .fg(BACKGROUND)
                .bg(ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "  DezirClip",
            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
        ),
        muted(if columns[0].width >= 48 {
            "  /  your clipboard, in focus"
        } else {
            ""
        }),
    ]);
    frame.render_widget(Paragraph::new(heading), columns[0]);
    let count = view.entries.len();
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            muted(format!("{count} on this page  ·  ")),
            label("LOCAL"),
        ]))
        .alignment(Alignment::Right),
        columns[1],
    );
}

fn draw_search(frame: &mut Frame<'_>, area: Rect, view: &View<'_>) {
    if area.height < 3 {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                label(" / Search  "),
                muted(if view.options.query.is_empty() {
                    "press / to find a clip".to_string()
                } else {
                    clean(&view.options.query, 1024)
                }),
            ])),
            area,
        );
        return;
    }
    let block = panel(Line::from(vec![
        label(" / Search "),
        muted("press / to edit "),
    ]));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let value = if view.options.query.is_empty() {
        Span::styled(
            "Find a thought, a link, a snippet…",
            Style::default().fg(MUTED),
        )
    } else {
        Span::styled(clean(&view.options.query, 1024), Style::default().fg(TEXT))
    };
    let mut spans = vec![value];
    if let Some(kind) = &view.options.kind {
        spans.push(muted("  ·  "));
        spans.push(Span::styled(
            format!("type: {}", clean(kind, 40)),
            Style::default().fg(BLUE),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), inner);
}

fn draw_history(frame: &mut Frame<'_>, area: Rect, view: &View<'_>, state: &mut ListState) {
    let page = view.options.offset / view.options.limit.max(1) + 1;
    let position = if view.entries.is_empty() {
        0
    } else {
        view.cursor + 1
    };
    let block = panel(Line::from(vec![
        label(" History "),
        muted(format!("{position}/{} ", view.entries.len())),
    ]))
    .title_bottom(
        Line::from(muted(format!(" page {page}  ·  PgUp / PgDn "))).alignment(Alignment::Right),
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if view.entries.is_empty() {
        frame.render_widget(
            Paragraph::new(Text::from(vec![
                Line::from(""),
                Line::from(Span::styled(
                    "Nothing here yet",
                    Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                )),
                Line::from(muted(if view.options.query.is_empty() {
                    "Copy something to get started."
                } else {
                    "Try another search with /."
                })),
            ]))
            .alignment(Alignment::Center),
            inner,
        );
        return;
    }
    let compact = inner.height < 7;
    let items = view
        .entries
        .iter()
        .map(|entry| {
            let kind = type_label(&entry.content_type);
            let title = clean(&entry.preview, 500);
            let title = if title.trim().is_empty() {
                "(empty entry)".to_string()
            } else {
                title
            };
            let columns = usize::from(inner.width.saturating_sub(9));
            let title = if title.width() > columns {
                format!("{}…", fit_columns(&title, columns.saturating_sub(1)))
            } else {
                title
            };
            let first = Line::from(vec![
                Span::styled(
                    format!("{kind:<5} "),
                    Style::default()
                        .fg(type_color(&entry.content_type))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(title),
            ]);
            if compact {
                return ListItem::new(first);
            }
            let source = if entry.source_app.is_empty() {
                "Unknown app".to_string()
            } else {
                clean(&entry.source_app, 24)
            };
            let tags = entry
                .tags
                .iter()
                .take(2)
                .map(|tag| format!("#{}", clean(tag, 14)))
                .collect::<Vec<_>>()
                .join(" ");
            ListItem::new(vec![
                first,
                Line::from(vec![
                    muted(format!(
                        "      {source}  ·  {}",
                        entry_time(entry.timestamp)
                    )),
                    Span::styled(
                        if entry.is_pinned {
                            "  PIN".to_string()
                        } else {
                            String::new()
                        },
                        Style::default().fg(GOLD),
                    ),
                    Span::styled(format!("  {tags}"), Style::default().fg(ACCENT)),
                ]),
                Line::from(""),
            ])
        })
        .collect::<Vec<_>>();
    state.select(Some(view.cursor.min(view.entries.len() - 1)));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_symbol("› ")
            .highlight_style(
                Style::default()
                    .bg(SELECTED)
                    .fg(TEXT)
                    .add_modifier(Modifier::BOLD),
            )
            .scroll_padding(1),
        inner,
        state,
    );
}

fn draw_preview(frame: &mut Frame<'_>, area: Rect, view: &View<'_>) -> usize {
    let selected = view.entries.get(view.cursor);
    let title = Line::from(vec![
        label(" Preview "),
        muted(
            selected
                .map(|entry| format!("#{} ", entry.id))
                .unwrap_or_default(),
        ),
    ]);
    let block = panel(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let Some(entry) = selected else {
        frame.render_widget(
            Paragraph::new(Text::from(vec![
                Line::from(""),
                Line::from(Span::styled(
                    "A little space for your next idea.",
                    Style::default().fg(MUTED),
                )),
                Line::from(""),
                Line::from(muted("Your selected clip appears here.")),
            ]))
            .alignment(Alignment::Center),
            inner,
        );
        return 0;
    };
    let compact = inner.height < 9;
    let meta_height = if inner.height <= 3 {
        0
    } else if compact {
        1
    } else {
        3
    };
    let sections = Layout::vertical([
        Constraint::Length(meta_height),
        Constraint::Length(u16::from(meta_height > 0)),
        Constraint::Min(0),
    ])
    .split(inner);
    let tags = entry
        .tags
        .iter()
        .take(8)
        .map(|tag| format!(" #{} ", clean(tag, 36)))
        .collect::<String>();
    let mut metadata = vec![Line::from(vec![
        Span::styled(
            format!(" {} ", type_label(&entry.content_type)),
            Style::default()
                .fg(BACKGROUND)
                .bg(type_color(&entry.content_type))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            if entry.is_pinned { "  PINNED" } else { "" },
            Style::default().fg(GOLD),
        ),
        muted(format!("  {}", clean(&entry.source_app, 36))),
    ])];
    if !compact {
        metadata.push(Line::from(muted(format!(
            "{}  ·  {} characters",
            entry_time(entry.timestamp),
            view.preview.chars().count()
        ))));
        metadata.push(Line::from(vec![
            muted("tags "),
            Span::styled(
                if tags.is_empty() {
                    " no tags  ·  t to add".to_string()
                } else {
                    tags
                },
                Style::default().fg(ACCENT),
            ),
        ]));
    }
    frame.render_widget(Paragraph::new(metadata), sections[0]);
    frame.render_widget(
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(BORDER)),
        sections[1],
    );
    let content_area = sections[2];
    if content_area.is_empty() {
        return 0;
    }
    let numbered = matches!(entry.content_type.as_str(), "code") && content_area.width >= 24;
    let content_width = content_area
        .width
        .saturating_sub(if numbered { 6 } else { 1 })
        .max(1);
    let lines = preview_lines(view.preview, usize::from(content_width));
    let visible = usize::from(content_area.height);
    let offset = view.preview_offset.min(lines.len().saturating_sub(visible));
    let text = lines
        .iter()
        .enumerate()
        .skip(offset)
        .take(visible)
        .map(|(index, line)| {
            let mut spans = Vec::new();
            if numbered {
                spans.push(muted(format!("{:>3}  ", index + 1)));
            }
            spans.extend(highlight_text(line, &view.options.query));
            Line::from(spans)
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(text), content_area);
    if lines.len() > visible && content_area.width > 1 {
        let mut state = ScrollbarState::new(lines.len())
            .position(offset)
            .viewport_content_length(visible);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .thumb_style(Style::default().fg(ACCENT))
                .track_style(Style::default().fg(BORDER)),
            content_area,
            &mut state,
        );
    }
    offset
}

fn highlight_text<'a>(text: &'a str, query: &str) -> Vec<Span<'a>> {
    if query.is_empty() {
        return vec![Span::styled(text, Style::default().fg(TEXT))];
    }
    let mut spans = Vec::new();
    let mut rest = text;
    while let Some(index) = rest.find(query) {
        if index > 0 {
            spans.push(Span::styled(&rest[..index], Style::default().fg(TEXT)));
        }
        spans.push(Span::styled(
            &rest[index..index + query.len()],
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ));
        rest = &rest[index + query.len()..];
    }
    spans.push(Span::styled(rest, Style::default().fg(TEXT)));
    spans
}

fn draw_footer(frame: &mut Frame<'_>, area: Rect, view: &View<'_>) {
    let primary = Line::from(vec![
        Span::styled(
            format!(" Enter {} ", action_name(view.options.action)),
            Style::default()
                .fg(BACKGROUND)
                .bg(ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        muted(if area.width < 68 {
            "  / search  q close"
        } else {
            "  ↑↓ select  / search  ←→ preview  q close"
        }),
    ]);
    let secondary = if view.status.is_empty() {
        Line::from(muted(if area.width < 68 {
            " p pin  t/u tags  c copy  d delete"
        } else {
            " p pin   t/u tags   c copy   d delete   r refresh   [/] pages"
        }))
    } else {
        Line::from(vec![
            Span::styled(" ● ", Style::default().fg(ACCENT)),
            Span::styled(clean(view.status, 500), Style::default().fg(GOLD)),
        ])
    };
    frame.render_widget(Paragraph::new(vec![primary, secondary]), area);
}

fn draw_prompt(frame: &mut Frame<'_>, prompt: &Prompt<'_>) {
    let area = frame.area();
    let width = area.width.saturating_sub(6).min(72).max(1);
    let height = area.height.min(7);
    let popup = Rect::new(
        (area.width - width) / 2,
        (area.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, popup);
    let destructive = prompt.label.starts_with("Delete entry");
    let accent = if destructive { GOLD } else { ACCENT };
    let block = panel(Line::from(Span::styled(
        if destructive {
            " Confirm deletion "
        } else {
            " Edit "
        },
        Style::default().fg(accent).add_modifier(Modifier::BOLD),
    )))
    .border_style(Style::default().fg(accent));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let clean_value = clean(prompt.value, 1024);
    let available = usize::from(inner.width.saturating_sub(3));
    let tail = tail_columns(&clean_value, available);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                clean(prompt.label, 200),
                Style::default().fg(TEXT),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled("› ", Style::default().fg(accent)),
                Span::raw(tail.clone()),
            ]),
            Line::from(muted("Enter apply   Esc cancel")),
        ]),
        inner,
    );
    if inner.height >= 3 && inner.width >= 2 {
        frame.set_cursor_position((inner.x + 2 + tail.width() as u16, inner.y + 2));
    }
}

fn tail_columns(text: &str, columns: usize) -> String {
    if text.width() <= columns {
        return text.to_string();
    }
    let mut width = 0;
    let tail = text
        .chars()
        .rev()
        .take_while(|character| {
            width += unicode_width::UnicodeWidthChar::width(*character).unwrap_or(0);
            width <= columns
        })
        .collect::<String>();
    tail.chars().rev().collect()
}

fn type_label(kind: &str) -> &str {
    match kind {
        "code" => "CODE",
        "url" => "LINK",
        "image" => "IMAGE",
        "html" | "rich_text" => "RICH",
        "file" | "files" => "FILE",
        "video" => "VIDEO",
        _ => "TEXT",
    }
}

fn type_color(kind: &str) -> Color {
    match kind {
        "code" => BLUE,
        "url" => ACCENT,
        "image" | "video" => Color::Rgb(201, 164, 244),
        "file" | "files" => GOLD,
        _ => MUTED,
    }
}

fn entry_time(timestamp: i64) -> String {
    chrono::DateTime::from_timestamp_millis(timestamp)
        .map(|time| {
            time.with_timezone(&chrono::Local)
                .format("%b %d · %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "Unknown time".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    fn entry() -> ClipboardEntry {
        serde_json::from_value(serde_json::json!({"id":42,"content_type":"code","content":"fn main() {}",
            "preview":"A useful snippet 中文🙂","source_app":"Terminal","timestamp":1700000000000i64,
            "is_pinned":true,"tags":["work","rust"]})).unwrap()
    }

    fn options() -> BrowseOptions {
        BrowseOptions {
            query: String::new(),
            kind: None,
            limit: 100,
            offset: 0,
            action: frontend::Action::Copy,
            passthrough: crate::infrastructure::osc52::Passthrough::None,
        }
    }

    fn draw(
        width: u16,
        height: u16,
        entries: &[ClipboardEntry],
        query: &str,
        prompt: Option<Prompt<'_>>,
    ) -> ratatui::buffer::Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let mut options = options();
        options.query = query.to_string();
        let view = View {
            options: &options,
            entries,
            cursor: 0,
            preview: "fn main() {\n    println!(\"你好🙂\");\n}",
            preview_offset: 0,
            status: "",
        };
        terminal
            .draw(|frame| {
                paint(frame, &view, prompt.as_ref(), &mut ListState::default());
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn text(buffer: &ratatui::buffer::Buffer) -> String {
        buffer
            .content()
            .chunks(buffer.area.width as usize)
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn wide_layout_has_distinct_history_preview_and_visible_metadata() {
        let buffer = draw(120, 34, &[entry()], "", None);
        let screen = text(&buffer);
        for label in [
            "DezirClip",
            "History",
            "Preview",
            "CODE",
            "Terminal",
            "#work",
            "PINNED",
            "Enter copy",
            "Search",
        ] {
            assert!(screen.contains(label), "missing {label}\n{screen}");
        }
        let history = buffer
            .content()
            .iter()
            .find(|cell| cell.symbol() == "H")
            .unwrap();
        assert_ne!(history.fg, Color::Reset);
        assert!(buffer.content().iter().any(|cell| cell.bg == SELECTED));
        assert!(screen.contains("println!"));
        let history_row = screen
            .lines()
            .position(|line| line.contains("History"))
            .unwrap();
        let preview_row = screen
            .lines()
            .position(|line| line.contains("Preview"))
            .unwrap();
        assert_eq!(history_row, preview_row, "wide panes must sit side by side");
    }

    #[test]
    fn compact_and_tiny_layouts_fit_and_keep_an_exit_visible() {
        for (width, height) in [(64, 24), (40, 16), (36, 12), (25, 8)] {
            let buffer = draw(width, height, &[entry()], "", None);
            let screen = text(&buffer);
            assert_eq!(
                buffer.content().len(),
                usize::from(width) * usize::from(height)
            );
            assert!(screen.contains("close"), "{screen}");
            assert!(screen.contains("DZC"), "{screen}");
            if width >= 36 {
                assert!(screen.contains("History"), "{screen}");
                assert!(screen.contains("Preview"), "{screen}");
                assert!(screen.contains("fn main"), "{screen}");
                let history_row = screen
                    .lines()
                    .position(|line| line.contains("History"))
                    .unwrap();
                let preview_row = screen
                    .lines()
                    .position(|line| line.contains("Preview"))
                    .unwrap();
                assert!(preview_row > history_row, "narrow panes must stack");
            }
        }
    }

    #[test]
    fn empty_search_and_destructive_prompt_are_deliberate_states() {
        let empty = text(&draw(100, 30, &[], "missing", None));
        assert!(empty.contains("Nothing here yet"));
        assert!(empty.contains("Try another search"));
        let prompted = text(&draw(
            100,
            30,
            &[entry()],
            "",
            Some(Prompt {
                label: "Delete entry 42? Type yes:",
                value: "yes",
            }),
        ));
        assert!(prompted.contains("Confirm deletion"));
        assert!(prompted.contains("Delete entry 42? Type yes:"));
        assert!(prompted.contains("Enter apply   Esc cancel"));
    }

    #[test]
    fn untrusted_metadata_and_long_unicode_prompt_cannot_inject_controls() {
        let mut hostile = entry();
        hostile.source_app = "app\x1b]52;c;bad\x07\u{202e}".to_string();
        hostile.preview = "bad\nextra row\x1b[31m".to_string();
        hostile.tags = vec!["work\r\nforged".to_string()];
        let buffer = draw(
            100,
            30,
            &[hostile],
            "",
            Some(Prompt {
                label: "Tag to add:",
                value: &"中文🙂".repeat(200),
            }),
        );
        assert!(buffer.content().iter().all(|cell| cell
            .symbol()
            .chars()
            .all(|c| !c.is_control() && c != '\u{202e}')));
        assert_eq!(tail_columns("中文🙂abc", 5), "🙂abc");
        assert_eq!(fit_columns("中文🙂", 3), "中");
    }

    #[test]
    fn minimized_and_single_cell_terminals_accept_prompts_without_panicking() {
        for (width, height) in [(0, 0), (0, 10), (10, 0), (1, 1)] {
            let buffer = draw(
                width,
                height,
                &[],
                "",
                Some(Prompt {
                    label: "Tag to add:",
                    value: "中文🙂",
                }),
            );
            assert_eq!(
                buffer.content().len(),
                usize::from(width) * usize::from(height)
            );
        }
    }
}
