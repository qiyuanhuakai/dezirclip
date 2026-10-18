//! External selectors receive sanitized metadata and return only listed candidates.

use super::frontend::{self, BrowseOptions};
use crate::domain::models::ClipboardEntry;
use crate::infrastructure::repository::clipboard_repo::SqliteClipboardRepository;
use clap::ValueEnum;
use std::io::{Read, Write};
use std::process::{Command, Stdio};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum Backend {
    #[default]
    Fzf,
    Dmenu,
    Rofi,
}

pub struct PickArgs {
    pub options: BrowseOptions,
    pub backend: Backend,
}

pub fn run(args: &PickArgs, repo: &SqliteClipboardRepository) -> Result<Option<i64>, String> {
    let entries = frontend::query_rows(&args.options, repo)?;
    if entries.is_empty() {
        return Ok(None);
    }
    let rows: Vec<_> = entries.iter().map(candidate_row).collect();
    let mut command = picker_command(args.backend);
    let choice = run_picker(&mut command, &rows)?;
    choice
        .map(|choice| validate_choice(&choice, &rows, &entries))
        .transpose()
}

fn candidate_row(entry: &ClipboardEntry) -> String {
    format!(
        "{}\t{} [{}] {}",
        entry.id,
        if entry.is_pinned { "*" } else { " " },
        frontend::display_text(&entry.content_type, 30),
        frontend::display_text(&entry.preview, 500)
    )
}

fn picker_command(backend: Backend) -> Command {
    let (name, arguments): (&str, &[&str]) = match backend {
        Backend::Fzf => (
            "fzf",
            &[
                "--no-multi",
                "--delimiter=\t",
                "--with-nth=2..",
                "--prompt=clipboard> ",
            ],
        ),
        Backend::Dmenu => ("dmenu", &["-i", "-p", "clipboard"]),
        Backend::Rofi => ("rofi", &["-dmenu", "-i", "-p", "clipboard"]),
    };
    let mut command = Command::new(name);
    command.args(arguments);
    // fzf options may contain preview commands and executable key bindings.
    command.env_remove("FZF_DEFAULT_OPTS");
    command.env_remove("FZF_DEFAULT_OPTS_FILE");
    command.env_remove("FZF_DEFAULT_COMMAND");
    command
}

fn run_picker(command: &mut Command, rows: &[String]) -> Result<Option<String>, String> {
    const MAX_SELECTION_BYTES: u64 = 64 * 1024;
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Could not start picker; check that it is on PATH: {error}"))?;
    let mut input = rows.join("\n").into_bytes();
    input.push(b'\n');
    let mut pipe = child
        .stdin
        .take()
        .ok_or_else(|| "Could not open picker input".to_string())?;
    let writer = std::thread::spawn(move || pipe.write_all(&input));
    let mut output = Vec::new();
    let read = child
        .stdout
        .take()
        .ok_or_else(|| "Could not open picker output".to_string())?
        .take(MAX_SELECTION_BYTES + 1)
        .read_to_end(&mut output);
    if read.is_err() || output.len() as u64 > MAX_SELECTION_BYTES {
        let _ = child.kill();
        let _ = child.wait();
        let _ = writer.join();
        return Err("Picker returned unreadable or excessive output".to_string());
    }
    let status = child.wait().map_err(|error| error.to_string())?;
    let written = writer
        .join()
        .map_err(|_| "Could not write picker candidates".to_string())?;
    if matches!(status.code(), Some(1 | 130)) {
        return Ok(None);
    }
    if !status.success() {
        return Err(format!("Picker exited unsuccessfully: {status}"));
    }
    if let Err(error) = written {
        if error.kind() != std::io::ErrorKind::BrokenPipe {
            return Err("Could not write picker candidates".to_string());
        }
    }
    String::from_utf8(output)
        .map(Some)
        .map_err(|_| "Picker returned invalid UTF-8".to_string())
}

fn validate_choice(
    selected: &str,
    rows: &[String],
    entries: &[ClipboardEntry],
) -> Result<i64, String> {
    let selected = selected.trim_end_matches(['\r', '\n']);
    rows.iter()
        .position(|row| row == selected)
        .map(|index| entries[index].id)
        .ok_or_else(|| "Picker returned an entry outside the candidate list".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: i64, preview: &str) -> ClipboardEntry {
        serde_json::from_value(serde_json::json!({
            "id": id, "content_type": "text", "content": "payload",
            "source_app": "test", "timestamp": 0, "preview": preview, "is_pinned": false
        }))
        .unwrap()
    }

    #[test]
    fn selected_display_must_exactly_match_a_candidate() {
        let entries = vec![entry(1, "one"), entry(2, "two")];
        let rows: Vec<_> = entries.iter().map(candidate_row).collect();
        assert_eq!(
            validate_choice(&(rows[1].clone() + "\r\n"), &rows, &entries).unwrap(),
            2
        );
        for forged in ["2", "2\tchanged", "3\t  [text] two", "1\n2", ""] {
            assert!(validate_choice(forged, &rows, &entries).is_err());
        }
    }

    #[test]
    fn metadata_controls_cannot_add_rows_or_terminal_commands() {
        let row = candidate_row(&entry(7, "中文\n8\tforged\u{1b}[31m\u{202e}end"));
        assert_eq!(row.matches('\t').count(), 1);
        assert!(!row.contains(['\n', '\r', '\u{1b}', '\u{202e}']));
        assert!(row.starts_with("7\t"));
    }

    #[test]
    fn every_backend_uses_a_direct_argument_vector() {
        for backend in [Backend::Fzf, Backend::Dmenu, Backend::Rofi] {
            let command = picker_command(backend);
            assert!(!command.get_args().any(|argument| argument == "-c"));
            for name in [
                "FZF_DEFAULT_OPTS",
                "FZF_DEFAULT_OPTS_FILE",
                "FZF_DEFAULT_COMMAND",
            ] {
                assert!(command
                    .get_envs()
                    .any(|(key, value)| key == name && value.is_none()));
            }
        }
    }
}
