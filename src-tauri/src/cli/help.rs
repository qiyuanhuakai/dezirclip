//! Categorized terminal help, generated from the same clap schema as parsing.
use clap::Command;
use unicode_width::UnicodeWidthStr;

fn paint(text: &str, code: &str, color: bool) -> String {
    if color { format!("\x1b[{code}m{text}\x1b[0m") } else { text.to_string() }
}

fn category(name: &str) -> &'static str {
    match name {
        "list" | "search" | "get" | "count" | "stats" | "watch" => "History & search",
        "add" | "edit" | "delete" | "clear" | "pin" | "unpin" | "tag" => "Manage entries",
        "clipboard" | "copy" | "pick" | "tui" => "Clipboard & interactive tools",
        "import" | "export" | "theme" => "Backups & themes",
        "service" | "privacy" | "capabilities" | "build-info" => "Service & diagnostics",
        "bash" | "zsh" | "fish" | "nu" | "pwsh" | "mcp" => "Shell & automation",
        _ => "Help",
    }
}

fn argument_description(id: &str, path: &str) -> &'static str {
    match id {
        "query" => "Text or expression to search for in clipboard history.",
        "json" => "Write structured JSON output for scripts.",
        "quiet" => "Suppress informational output.",
        "ids" => "Print matching entry IDs only, one per line.",
        "kind" => "Filter by stored content type, such as text, image, code or url.",
        "tag" => "Tag name to filter, add or remove.",
        "pinned" => "Include only pinned entries.",
        "limit" => "Maximum number of matching entries to return.",
        "offset" => "Skip this many matching entries before returning results.",
        "metadata" => "Omit full content; include bounded preview and metadata.",
        "tsv" => "Print ID and sanitized single-line preview separated by a tab.",
        "exclude_sensitive" => "Exclude sensitive-tagged and encrypted entries before decoding.",
        "preview" => "Return a bounded preview instead of the complete content.",
        "id" => "Clipboard history entry ID.",
        "mode" => "Choose how the search query matches history.",
        "path" if path == "dzc export" => "Destination backup file (.json, or encrypted .dzc/.tiez).",
        "path" if path == "dzc import" => "Backup file to import into local history.",
        "path" => "Local theme package to validate.",
        "mime" => "Clipboard representation, for example text/plain or text/html.",
        "content" => "Text to write; use - for stdin or @<path> for a file.",
        "action" => "What to do with the selected history entry.",
        "host" => "Use an explicitly authorized local Host instead of direct database access.",
        "no_capture" => "Start the local Service without clipboard monitoring.",
        "from_instance" => "Require the expected Host instance before changing its state.",
        "database" => "Explicit local database path for the Service handoff.",
        "directory" => "Destination directory for the shared data profile.",
        "previous_directory" => "Original profile directory used by the recovery operation.",
        "epoch" => "Expected privacy epoch for the authorized profile move.",
        "start_if_missing" => "Start a local Service if no desktop Host is running.",
        "copy" => "Grant the Surface permission to copy selected clipboard entries.",
        "paste" => "Grant the Surface permission to paste into an authorized target.",
        "seconds" => "Lifetime in seconds; accepted range is 1 to 86400.",
        "token" => "Previously issued delegated grant token to revoke.",
        _ => "See this command's documented argument contract.",
    }
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.width() + 1 + word.width() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() { line.push(' '); }
        for ch in word.chars() {
            if line.width() + unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0) > width {
                lines.push(std::mem::take(&mut line));
            }
            line.push(ch);
        }
    }
    if !line.is_empty() { lines.push(line); }
    lines
}

fn panel(out: &mut String, title: &str, rows: &[(String, String)], width: usize, color: bool) {
    if rows.is_empty() { return; }
    let inner = width - 4;
    let title = format!(" {title} ");
    out.push_str(&format!("\n{}{}{}\n", paint("╭─", "90", color), paint(&title, "1;33", color),
        paint(&format!("{}╮", "─".repeat(width.saturating_sub(title.width() + 3))), "90", color)));
    let column = rows.iter().map(|(name, _)| name.width()).max().unwrap_or(0).min(inner / 2).max(12);
    for (name, description) in rows {
        let (names, descriptions) = if name.width() > column || width < 64 {
            (wrap(name, inner), wrap(description, inner.saturating_sub(2)))
        } else { (vec![name.clone()], wrap(description, inner - column - 2)) };
        if name.width() > column || width < 64 {
            for line in names {
                out.push_str(&format!("{} {}{} {}\n", paint("│", "90", color), paint(&line, "1;36", color), " ".repeat(inner - line.width()), paint("│", "90", color)));
            }
            for line in descriptions {
                out.push_str(&format!("{}   {}{} {}\n", paint("│", "90", color), line, " ".repeat(inner - 2 - line.width()), paint("│", "90", color)));
            }
        } else {
            for (index, line) in descriptions.iter().enumerate() {
                let label = if index == 0 { name.as_str() } else { "" };
                out.push_str(&format!("{} {}{}  {}{} {}\n", paint("│", "90", color), paint(label, "1;36", color),
                    " ".repeat(column - label.width()), line, " ".repeat(inner - column - 2 - line.width()), paint("│", "90", color)));
            }
        }
    }
    out.push_str(&format!("{}\n", paint(&format!("╰{}╯", "─".repeat(width - 2)), "90", color)));
}

pub fn render(command: &Command, path: &str, width: usize, color: bool) -> String {
    let width = width.clamp(40, 120);
    let usage = command.clone().bin_name(path.to_string()).render_usage().to_string();
    let mut out = format!("{} {}\n", paint("Usage:", "1;33", color), paint(usage.strip_prefix("Usage: ").unwrap_or(&usage), "1;36", color));
    if let Some(about) = command.get_long_about().or(command.get_about()) {
        out.push('\n');
        out.push_str(&wrap(&about.to_string(), width).join("\n"));
        out.push('\n');
    }
    if path == "dzc" {
        out.push_str(&paint(&wrap("Run dzc with no arguments to open the interactive TUI.", width).join("\n"), "32", color));
        out.push('\n');
    }
    let groups = if path == "dzc" { vec!["History & search", "Manage entries", "Clipboard & interactive tools", "Backups & themes", "Service & diagnostics", "Shell & automation", "Help"] } else { vec!["Commands"] };
    for group in groups {
        let rows = command.get_subcommands().filter(|sub| !sub.is_hide_set() && (path != "dzc" || category(sub.get_name()) == group))
            .map(|sub| (sub.get_name().to_string(), sub.get_about().map(ToString::to_string).unwrap_or_default())).collect::<Vec<_>>();
        panel(&mut out, group, &rows, width, color);
    }
    let mut positionals = Vec::new();
    let mut options = Vec::new();
    for arg in command.get_arguments().filter(|arg| !arg.is_hide_set()) {
        let mut labels = Vec::new();
        if let Some(short) = arg.get_short() { labels.push(format!("-{short}")); }
        if let Some(long) = arg.get_long() { labels.push(format!("--{long}")); }
        let mut label = labels.join(", ");
        if arg.get_num_args().is_some_and(|values| values.takes_values()) {
            let names = arg.get_value_names().map(|names| names.iter().map(ToString::to_string).collect::<Vec<_>>()).unwrap_or_else(|| vec![arg.get_id().to_string().to_uppercase()]);
            if !label.is_empty() { label.push(' '); }
            label.push_str(&format!("<{}>", names.join("> <")));
        }
        let mut description = arg.get_long_help().or(arg.get_help()).map(ToString::to_string)
            .unwrap_or_else(|| argument_description(arg.get_id().as_str(), path).to_string());
        let defaults = arg.get_default_values().iter().map(|value| value.to_string_lossy()).filter(|value| !value.is_empty() && value != "false").collect::<Vec<_>>();
        if !defaults.is_empty() { description.push_str(&format!(" [default: {}]", defaults.join(", "))); }
        let values = arg.get_possible_values().into_iter().filter(|value| !value.is_hide_set()).map(|value| value.get_name().to_string()).collect::<Vec<_>>();
        if !values.is_empty() { description.push_str(&format!(" [choices: {}]", values.join(", "))); }
        if description.is_empty() { description = "Value accepted by this command.".to_string(); }
        if arg.is_positional() { positionals.push((label, description)); } else { options.push((label, description)); }
    }
    panel(&mut out, "Arguments", &positionals, width, color);
    panel(&mut out, "Options", &options, width, color);
    if path == "dzc" {
        panel(&mut out, "Examples", &[
            ("dzc".into(), "Browse, search and select clipboard history.".into()),
            ("dzc search \"meeting\"".into(), "Find matching clipboard entries.".into()),
            ("dzc list --json".into(), "Read structured output for scripts.".into()),
            ("dzc help service".into(), "Explore a command and its options.".into()),
        ], width, color);
    }
    out
}

pub fn decorate(mut command: Command, path: &str, width: usize, color: bool) -> Command {
    for child in command.get_subcommands_mut() {
        *child = decorate(child.clone(), &format!("{path} {}", child.get_name()), width, color);
    }
    let text = render(&command, path, width, color);
    command.override_help(text)
}
