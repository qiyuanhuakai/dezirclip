//! `dzc` — clipboard manager CLI binary.
//!
//! Wire-up:
//! * clap parses `Cli` → `Commands::*` (each variant owns its `*Args`).
//! * The matching `cli::<cmd>::run(args, ...)` is invoked.
//! * History commands share the repository; clipboard commands use native APIs.
//!
//! History access opens the same SQLite file the app
//! uses and goes through the same repository types, so it needs no IPC, works
//! while the app is stopped, and picks up its writes the next time the app
//! reads the database. It also means a `dzc` write is invisible to a running
//! window until something triggers a re-read.
//!
//! DB initialization:
//! This binary lives in the `dezirclip` crate and shares its
//! `database::init_db` + `repository::migrations::run` plumbing. The
//! repo handles are built lazily on first use (see `open_repos`) so
//! `--help` and parse-error paths don't touch the filesystem.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use clap::{Args, Parser, Subcommand, ValueEnum};

use dezirclip::cli;
use dezirclip::cli::search::SearchMode;
use dezirclip::infrastructure::repository::clipboard_repo::SqliteClipboardRepository;
use dezirclip::infrastructure::repository::migrations;
use dezirclip::infrastructure::repository::tag_repo::SqliteTagRepository;

const APP_IDENTIFIER: &str = "io.github.qiyuanhuakai.dezirclip";
const LEGACY_APP_IDENTIFIER: &str = "com.tiez.clipboard";

#[derive(Parser)]
#[command(name = "dzc", version, about = "DezirClip CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List clipboard history.
    List(ListArgs),
    /// Search history by query.
    Search(SearchArgs),
    /// Get a single entry by ID (or `latest`).
    Get(GetArgs),
    /// Add a new entry from text, stdin (`-`), or file (`@<path>`).
    Add(AddCliArgs),
    /// Delete an entry. Requires `--yes` to confirm.
    Delete(DeleteCliArgs),
    /// Pin an entry.
    Pin(PinCliArgs),
    /// Unpin an entry.
    Unpin(PinCliArgs),
    /// Replace an entry's content. Content sources match `add`.
    Edit(EditCliArgs),
    /// Clear the unprotected history. Requires `--yes`.
    Clear(ClearCliArgs),
    /// Print the total number of stored entries.
    Count(CountCliArgs),
    /// Add / remove / list tags on an entry.
    Tag(TagCliArgs),
    /// Export history to a file.
    Export(ExportArgs),
    /// Import history from a file.
    Import(ImportArgs),
    /// Show stats.
    Stats(StatsArgs),
    /// Watch new entries as they arrive.
    Watch(WatchArgs),
    /// Read or write the current native system clipboard (independent of history).
    Clipboard(ClipboardArgs),
    /// Restore a history entry to the system clipboard; never simulates keys.
    Copy(CopyArgs),
    /// Select history using the built-in interface or fzf/dmenu/rofi.
    Pick(PickCliArgs),
    /// Browse and manage history in the terminal.
    Tui(BrowseCliArgs),
    /// Serve bounded, read-only local history tools over MCP stdio.
    Mcp,
    /// Validate an importable local CSS theme without opening history or applying it.
    Theme(ThemeCliArgs),
    /// Print Bash Ctrl-G integration for loading with eval.
    Bash,
    /// Print Zsh Ctrl-G integration for loading with eval.
    Zsh,
    /// Print Fish Ctrl-G integration for loading with source.
    Fish,
    /// Print Nushell integration, or install it at an explicit .nu path.
    #[command(alias = "nushell")]
    Nu(ShellInstallArgs),
    /// Print PowerShell Ctrl-G integration for loading with Invoke-Expression.
    #[command(alias = "powershell")]
    Pwsh,
    #[command(hide = true)]
    ClipboardOwner,
}

#[derive(Args)]
struct BrowseCliArgs {
    #[arg(long, default_value = "")]
    query: String,
    #[arg(long)]
    kind: Option<String>,
    #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(i32).range(1..=200))]
    limit: i32,
    #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(i32).range(0..))]
    offset: i32,
    #[arg(long, value_enum, default_value_t = cli::frontend::Action::Copy)]
    action: cli::frontend::Action,
    /// Explicit single-layer tmux wrapping; requires --action osc52.
    #[arg(long, value_enum, default_value_t = Osc52Passthrough::None)]
    osc52_passthrough: Osc52Passthrough,
}

impl BrowseCliArgs {
    fn options(self) -> Result<cli::frontend::BrowseOptions, String> {
        if self.action != cli::frontend::Action::Osc52
            && self.osc52_passthrough != Osc52Passthrough::None
        {
            return Err("--osc52-passthrough tmux requires --action osc52".to_string());
        }
        Ok(cli::frontend::BrowseOptions {
            query: self.query,
            kind: self.kind,
            limit: self.limit,
            offset: self.offset,
            action: self.action,
            passthrough: match self.osc52_passthrough {
                Osc52Passthrough::None => dezirclip::infrastructure::osc52::Passthrough::None,
                Osc52Passthrough::Tmux => dezirclip::infrastructure::osc52::Passthrough::Tmux,
            },
        })
    }
}

#[derive(Args)]
struct PickCliArgs {
    #[command(flatten)]
    options: BrowseCliArgs,
    /// Use an external picker; defaults to the built-in terminal interface.
    #[arg(long, value_enum)]
    backend: Option<cli::pick::Backend>,
}

#[derive(Args)]
struct ShellInstallArgs {
    /// Write to this .nu file; an existing different file is left unchanged.
    #[arg(long, value_name = "PATH")]
    install: Option<PathBuf>,
}

#[derive(Args)]
struct ThemeCliArgs {
    #[command(subcommand)]
    command: ThemeCommand,
}

#[derive(Subcommand)]
enum ThemeCommand {
    /// Check the package and reject CSS that references external resources.
    Validate {
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

fn run_theme(args: ThemeCliArgs) -> Result<(), String> {
    use dezirclip::domain::theme::{ThemePackage, MAX_THEME_BYTES};
    match args.command {
        ThemeCommand::Validate { path, json } => {
            let bytes = dezirclip::infrastructure::local_file::read_bounded_regular(&path, MAX_THEME_BYTES)
                .map_err(|error| error.to_string())?;
            let theme = ThemePackage::parse(&bytes)?;
            if json {
                println!("{}", serde_json::json!({
                    "valid": true, "id": theme.id, "name": theme.name,
                    "base": theme.base, "bytes": bytes.len()
                }));
            } else {
                println!("Valid DezirClip CSS theme: {}", theme.id);
            }
            Ok(())
        }
    }
}

#[derive(Args)]
#[command(allow_negative_numbers = true)]
struct ListArgs {
    /// Optional positional limit. `-1` returns every row.
    limit: Option<i32>,
    #[arg(long)]
    kind: Option<String>,
    #[arg(long)]
    tag: Option<String>,
    #[arg(long)]
    pinned: bool,
    #[arg(long, conflicts_with = "tsv")]
    json: bool,
    #[arg(long, conflicts_with_all = ["json", "tsv"])]
    ids: bool,
    #[arg(long, short)]
    quiet: bool,
    /// Omit content payloads from JSON; bounded preview and metadata only.
    #[arg(long)]
    metadata: bool,
    /// ID and sanitized single-line preview separated by a tab.
    #[arg(long)]
    tsv: bool,
    #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(i32).range(0..))]
    offset: i32,
    /// Exclude sensitive-tagged and encrypted records before decoding.
    #[arg(long)]
    exclude_sensitive: bool,
}

#[derive(Args)]
struct SearchArgs {
    query: String,
    /// Restrict results to a stored content type before pagination.
    #[arg(long)]
    kind: Option<String>,
    #[arg(long, value_enum, default_value_t = CliSearchMode::Contains)]
    mode: CliSearchMode,
    #[arg(long)]
    limit: Option<i32>,
    #[arg(long)]
    json: bool,
    #[arg(long, short)]
    quiet: bool,
    #[arg(long, conflicts_with_all = ["json", "tsv"])]
    ids: bool,
    #[arg(long)]
    metadata: bool,
    #[arg(long, conflicts_with = "json")]
    tsv: bool,
    #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(i32).range(0..))]
    offset: i32,
    #[arg(long)]
    exclude_sensitive: bool,
}

#[derive(ValueEnum, Clone, Copy)]
enum CliSearchMode {
    Contains,
    Fuzzy,
    Regex,
    Fts5,
}

impl From<CliSearchMode> for SearchMode {
    fn from(m: CliSearchMode) -> Self {
        match m {
            CliSearchMode::Contains => SearchMode::Contains,
            CliSearchMode::Fuzzy => SearchMode::Fuzzy,
            CliSearchMode::Regex => SearchMode::Regex,
            CliSearchMode::Fts5 => SearchMode::Fts5,
        }
    }
}

#[derive(Args)]
struct GetArgs {
    /// Numeric ID, or the literal `latest`.
    id: String,
    #[arg(long)]
    preview: bool,
    #[arg(long)]
    json: bool,
    #[arg(long, short)]
    quiet: bool,
    /// Write stored content bytes without appending a newline.
    #[arg(long, conflicts_with_all = ["json", "mime"])]
    raw: bool,
    /// Export the requested representation without appending a newline.
    #[arg(long, conflicts_with_all = ["json", "preview"])]
    mime: Option<String>,
    #[arg(long)]
    exclude_sensitive: bool,
}

/// CLI args for `add`. The positional `content` is dispatched into one
/// of three modes by `parse_add_source`:
/// * plain text     → `text: Some(content)`
/// * `-`            → `stdin: true`
/// * `@<path>`      → `file: Some(path)`
#[derive(Args)]
struct AddCliArgs {
    /// Text payload. Use `-` to read stdin, or `@<path>` to read a file.
    content: String,
    /// Override the inferred content type (`text`, `html`, `image`, ...).
    #[arg(long = "type")]
    kind: Option<String>,
}

/// CLI args for `delete`. Requires `--yes` to confirm; the bin never
/// prompts because non-interactive use is the primary case.
#[derive(Args)]
struct DeleteCliArgs {
    id: String,
    /// Confirm the deletion. Without this flag, `delete` refuses.
    #[arg(long, short)]
    yes: bool,
}

/// CLI args shared by `pin` and `unpin`.
#[derive(Args)]
struct PinCliArgs {
    id: String,
    /// Suppress the confirmation line; emit only an exit code.
    #[arg(long, short)]
    quiet: bool,
}

/// CLI args for `edit`. The content token resolves exactly like `add`'s, so
/// `-` reads stdin and `@<path>` reads a file.
#[derive(Args)]
struct EditCliArgs {
    id: String,
    /// Replacement content. Use `-` to read stdin, or `@<path>` for a file.
    content: String,
    /// Suppress the confirmation line; emit only an exit code.
    #[arg(long, short)]
    quiet: bool,
}

/// CLI args for `clear`. `--yes` is mandatory: the CLI never prompts.
#[derive(Args)]
struct ClearCliArgs {
    /// Confirm the clear. Without this flag, `clear` refuses.
    #[arg(long, short)]
    yes: bool,
    /// Suppress the summary line; emit only an exit code.
    #[arg(long, short)]
    quiet: bool,
}

/// CLI args for `count`.
#[derive(Args)]
struct CountCliArgs {
    /// Emit `{"count": <n>}` instead of the bare integer.
    #[arg(long)]
    json: bool,
}

/// CLI args for `tag` — three explicit subcommands instead of `--add`/
/// `--remove` Vec flags. Cleaner UX and lets clap validate that
/// exactly one subcommand was given.
#[derive(Args)]
struct TagCliArgs {
    #[command(subcommand)]
    command: CliTagCommand,
}

#[derive(Subcommand)]
enum CliTagCommand {
    /// Append `<TAG>` to the entry's tag list.
    Add { id: i64, tag: String },
    /// Remove `<TAG>` from the entry's tag list.
    Remove { id: i64, tag: String },
    /// Print the entry's tags, one per line.
    List { id: i64 },
}

#[derive(Args)]
struct ExportArgs {
    path: String,
    /// Force encrypted output. Auto-detected when `path` ends in `.dzc` or `.tiez`.
    #[arg(long)]
    encrypted: bool,
    /// Passphrase for encrypted output. Required when `--encrypted` is
    /// set or the path is encrypted; must be at least 12 characters.
    #[arg(long)]
    passphrase: Option<String>,
    /// Suppress the success summary; emit only an exit code.
    #[arg(long, short)]
    quiet: bool,
}

#[derive(Args)]
struct ImportArgs {
    path: String,
    /// Import strategy. `merge` (default) overwrites on id collision;
    /// `replace` clears the table first.
    #[arg(long, default_value = "merge")]
    mode: String,
    /// Passphrase for encrypted backup files. Required when the input is
    /// encrypted; ignored for plaintext JSON.
    #[arg(long)]
    passphrase: Option<String>,
    /// Suppress the success summary; emit only an exit code.
    #[arg(long, short)]
    quiet: bool,
}

#[derive(Args)]
struct StatsArgs {
    /// Number of entries to include in the `most_used` array.
    /// Default 5. Pass `0` to suppress the section.
    #[arg(long)]
    top: Option<i32>,
    /// Emit a single JSON object instead of the human-readable block.
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct WatchArgs {
    /// Case-insensitive substring filter applied to new entries'
    /// content (and `preview` as a fallback).
    #[arg(long)]
    pattern: Option<String>,
    /// Poll interval in milliseconds. Default 500 (G32 hard constraint).
    /// Clamped to `[50, 60_000]`.
    #[arg(long)]
    interval: Option<u64>,
    /// Emit each new entry as a JSON object on its own line (NDJSON).
    #[arg(long)]
    json: bool,
    /// Suppress the startup banner; entries are still printed.
    #[arg(long, short)]
    quiet: bool,
}

#[derive(Args)]
struct CopyArgs {
    id: String,
    #[arg(long, short)]
    quiet: bool,
    /// Destination; OSC 52 writes text to the controlling terminal.
    #[arg(long, value_enum, default_value_t = ClipboardTarget::Native)]
    target: ClipboardTarget,
    /// Wrap one tmux layer; only applies to --target osc52.
    #[arg(long, value_enum, default_value_t = Osc52Passthrough::None)]
    osc52_passthrough: Osc52Passthrough,
}

#[derive(ValueEnum, Clone, Copy, PartialEq, Eq)]
enum ClipboardTarget {
    Native,
    Osc52,
}

#[derive(ValueEnum, Clone, Copy, PartialEq, Eq)]
enum Osc52Passthrough {
    None,
    Tmux,
}

fn osc52_passthrough(
    target: ClipboardTarget,
    passthrough: Osc52Passthrough,
) -> Result<dezirclip::infrastructure::osc52::Passthrough, String> {
    use dezirclip::infrastructure::osc52::Passthrough;
    if target == ClipboardTarget::Native && passthrough != Osc52Passthrough::None {
        return Err("--osc52-passthrough tmux requires --target osc52".to_string());
    }
    Ok(match passthrough {
        Osc52Passthrough::None => Passthrough::None,
        Osc52Passthrough::Tmux => Passthrough::Tmux,
    })
}

#[derive(Args)]
struct ClipboardArgs {
    #[command(subcommand)]
    command: ClipboardCommand,
}

#[derive(Subcommand)]
enum ClipboardCommand {
    /// Read native clipboard bytes to stdout, without a trailing newline.
    Read {
        #[arg(long, default_value = "text/plain")]
        mime: String,
    },
    /// Write literal text, stdin (-), or file (@path) to the native clipboard.
    Write {
        #[arg(default_value = "-")]
        content: String,
        #[arg(long, default_value = "text/plain")]
        mime: String,
        /// OSC 52 accepts text/plain only, with a 100000-byte UTF-8 limit.
        #[arg(long, value_enum, default_value_t = ClipboardTarget::Native)]
        target: ClipboardTarget,
        /// Wrap one tmux layer; only applies to --target osc52.
        #[arg(long, value_enum, default_value_t = Osc52Passthrough::None)]
        osc52_passthrough: Osc52Passthrough,
    },
    /// List supported readable MIME representations of the current clipboard.
    Formats {
        #[arg(long)]
        json: bool,
    },
}

fn native_payload(
    content: Vec<u8>,
    mime: &str,
) -> Result<dezirclip::infrastructure::native_clipboard::Payload, String> {
    use dezirclip::infrastructure::native_clipboard::Payload;
    if mime == "image/png" {
        return Ok(Payload::Image(content));
    }
    let text =
        String::from_utf8(content).map_err(|e| format!("clipboard input is not UTF-8: {e}"))?;
    match mime {
        "text/plain" => Ok(Payload::Text(text)),
        "text/html" => Ok(Payload::Html {
            html: text.clone(),
            text,
        }),
        "text/uri-list" => Ok(Payload::Files(cli::payload::files(&text)?)),
        _ => Err(format!("unsupported clipboard MIME: {mime}")),
    }
}

fn run_clipboard(args: ClipboardArgs) -> Result<(), String> {
    use dezirclip::infrastructure::native_clipboard;
    use std::io::{Read, Write};
    match args.command {
        ClipboardCommand::Read { mime } => std::io::stdout()
            .lock()
            .write_all(&native_clipboard::read(&mime)?)
            .map_err(|e| e.to_string()),
        ClipboardCommand::Formats { json } => {
            let formats = native_clipboard::formats()?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string(&formats).map_err(|e| e.to_string())?
                );
            } else {
                for mime in formats {
                    println!("{mime}");
                }
            }
            Ok(())
        }
        ClipboardCommand::Write {
            content,
            mime,
            target,
            osc52_passthrough: passthrough,
        } => {
            let passthrough = osc52_passthrough(target, passthrough)?;
            if target == ClipboardTarget::Osc52 && mime != "text/plain" {
                return Err("OSC 52 only supports --mime text/plain".to_string());
            }
            let bytes = if content == "-" {
                let mut bytes = Vec::new();
                if target == ClipboardTarget::Osc52 {
                    std::io::stdin()
                        .take(dezirclip::infrastructure::osc52::MAX_TEXT_BYTES as u64 + 1)
                        .read_to_end(&mut bytes)
                        .map_err(|e| e.to_string())?;
                } else {
                    std::io::stdin()
                        .read_to_end(&mut bytes)
                        .map_err(|e| e.to_string())?;
                }
                bytes
            } else if let Some(path) = content.strip_prefix('@') {
                if target == ClipboardTarget::Osc52 {
                    let mut bytes = Vec::new();
                    std::fs::File::open(path)
                        .map_err(|e| e.to_string())?
                        .take(dezirclip::infrastructure::osc52::MAX_TEXT_BYTES as u64 + 1)
                        .read_to_end(&mut bytes)
                        .map_err(|e| e.to_string())?;
                    bytes
                } else {
                    std::fs::read(path).map_err(|e| e.to_string())?
                }
            } else {
                content.into_bytes()
            };
            if target == ClipboardTarget::Osc52 {
                if bytes.len() > dezirclip::infrastructure::osc52::MAX_TEXT_BYTES {
                    return Err(format!(
                        "OSC 52 text exceeds the {}-byte UTF-8 limit; nothing was sent",
                        dezirclip::infrastructure::osc52::MAX_TEXT_BYTES
                    ));
                }
                let text = String::from_utf8(bytes)
                    .map_err(|e| format!("OSC 52 input is not UTF-8: {e}"))?;
                return dezirclip::infrastructure::osc52::write_text(&text, passthrough);
            }
            native_clipboard::write(native_payload(bytes, &mime)?)
        }
    }
}

fn main() {
    let cli_args = Cli::parse();
    if let Err(e) = run(cli_args) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run(args: Cli) -> Result<(), String> {
    let command = match args.command {
        Commands::Mcp => return cli::mcp::serve(|| open_repos().map(|(repo, _)| repo)),
        Commands::Theme(args) => return run_theme(args),
        Commands::Bash => return cli::shell::run(cli::shell::Shell::Bash, None),
        Commands::Zsh => return cli::shell::run(cli::shell::Shell::Zsh, None),
        Commands::Fish => return cli::shell::run(cli::shell::Shell::Fish, None),
        Commands::Nu(args) => {
            return cli::shell::run(cli::shell::Shell::Nu, args.install.as_deref())
        }
        Commands::Pwsh => return cli::shell::run(cli::shell::Shell::Pwsh, None),
        Commands::Clipboard(a) => return run_clipboard(a),
        Commands::ClipboardOwner => {
            return dezirclip::infrastructure::native_clipboard::serve_owner()
        }
        command => command,
    };
    let (clip_repo, tag_repo) = open_repos()?;
    match command {
        Commands::Pick(args) => {
            let options = args.options.options()?;
            let selected = if let Some(backend) = args.backend {
                cli::pick::run(
                    &cli::pick::PickArgs {
                        options: options.clone(),
                        backend,
                    },
                    clip_repo.as_ref(),
                )?
            } else {
                cli::tui::run(&options, clip_repo.as_ref(), tag_repo.as_ref())?
            };
            finish_selection(selected, &options, clip_repo.as_ref())
        }
        Commands::Tui(args) => {
            let options = args.options()?;
            let selected = cli::tui::run(&options, clip_repo.as_ref(), tag_repo.as_ref())?;
            finish_selection(selected, &options, clip_repo.as_ref())
        }
        Commands::Mcp
        | Commands::Theme(_)
        | Commands::Bash
        | Commands::Zsh
        | Commands::Fish
        | Commands::Nu(_)
        | Commands::Pwsh => {
            unreachable!("stdio and shell initialization dispatched before history access")
        }
        Commands::List(a) => cli::list::run(
            &cli::list::ListArgs {
                limit: a.limit,
                kind: a.kind,
                tag: a.tag,
                pinned: a.pinned,
                json: a.json,
                ids: a.ids,
                quiet: a.quiet,
                metadata: a.metadata,
                tsv: a.tsv,
                offset: a.offset,
                exclude_sensitive: a.exclude_sensitive,
            },
            clip_repo.as_ref(),
        ),
        Commands::Search(a) => cli::search::run(
            &cli::search::SearchArgs {
                query: a.query,
                kind: a.kind,
                mode: a.mode.into(),
                limit: a.limit,
                json: a.json,
                quiet: a.quiet,
                ids: a.ids,
                metadata: a.metadata,
                tsv: a.tsv,
                offset: a.offset,
                exclude_sensitive: a.exclude_sensitive,
            },
            clip_repo.as_ref(),
        ),
        Commands::Get(a) => cli::get::run(
            &cli::get::GetArgs {
                id: a.id,
                preview: a.preview,
                json: a.json,
                quiet: a.quiet,
                raw: a.raw,
                mime: a.mime,
                exclude_sensitive: a.exclude_sensitive,
            },
            clip_repo.as_ref(),
        ),
        Commands::Copy(a) => {
            use dezirclip::infrastructure::native_clipboard::{self, Payload};
            let passthrough = osc52_passthrough(a.target, a.osc52_passthrough)?;
            let entry = cli::get::resolve(&a.id, false, clip_repo.as_ref())?;
            if a.target == ClipboardTarget::Osc52 {
                if !matches!(
                    entry.content_type.as_str(),
                    "text" | "code" | "url" | "rich_text" | "html"
                ) {
                    return Err(format!(
                        "OSC 52 only supports text history entries; entry {} has type {}",
                        entry.id, entry.content_type
                    ));
                }
                return dezirclip::infrastructure::osc52::write_text(&entry.content, passthrough);
            }
            let payload = match entry.content_type.as_str() {
                "image" => Payload::Image(cli::payload::image_bytes(&entry.content)?),
                "file" | "files" | "video" => Payload::Files(cli::payload::files(&entry.content)?),
                "html" => Payload::Html {
                    html: entry.html_content.unwrap_or_else(|| entry.content.clone()),
                    text: entry.content,
                },
                _ => {
                    if let Some(html) = entry.html_content {
                        Payload::Html {
                            html,
                            text: entry.content,
                        }
                    } else {
                        Payload::Text(entry.content)
                    }
                }
            };
            native_clipboard::write(payload)?;
            if !a.quiet {
                println!("Copied: id={}", entry.id);
            }
            Ok(())
        }
        Commands::Clipboard(_) | Commands::ClipboardOwner => {
            unreachable!("clipboard dispatched before opening history")
        }
        Commands::Add(a) => {
            let add_args = parse_add_source(&a.content, a.kind.as_deref());
            let id = cli::add::run(&add_args, clip_repo.as_ref())?;
            println!("Added: id={id}");
            Ok(())
        }
        Commands::Delete(a) => {
            let del_args = cli::delete::DeleteArgs {
                id: a.id,
                yes: a.yes,
            };
            cli::delete::run(&del_args, clip_repo.as_ref())
        }
        Commands::Tag(a) => {
            let command = match a.command {
                CliTagCommand::Add { id, tag } => cli::tag::TagCommand::Add { id, tag },
                CliTagCommand::Remove { id, tag } => cli::tag::TagCommand::Remove { id, tag },
                CliTagCommand::List { id } => cli::tag::TagCommand::List { id },
            };
            let tag_args = cli::tag::TagArgs { command };
            cli::tag::run(&tag_args, clip_repo.as_ref(), tag_repo.as_ref())
        }
        Commands::Pin(a) => report_pin(&a, true, clip_repo.as_ref()),
        Commands::Unpin(a) => report_pin(&a, false, clip_repo.as_ref()),
        Commands::Edit(a) => {
            let id = cli::edit::run(
                &cli::edit::EditArgs {
                    id: a.id.clone(),
                    source: parse_add_source(&a.content, None),
                },
                clip_repo.as_ref(),
            )?;
            if !a.quiet {
                println!("Edited: id={id}");
            }
            Ok(())
        }
        Commands::Clear(a) => {
            let removed =
                cli::clear::run(&cli::clear::ClearArgs { yes: a.yes }, clip_repo.as_ref())?;
            if !a.quiet {
                println!("Cleared {removed} entries");
            }
            Ok(())
        }
        Commands::Count(a) => {
            let total = cli::count::run(clip_repo.as_ref())?;
            println!("{}", cli::count::render(total, a.json));
            Ok(())
        }
        Commands::Export(a) => cli::export::run(
            &cli::export::ExportArgs {
                path: a.path,
                encrypted: a.encrypted,
                passphrase: a.passphrase,
                quiet: a.quiet,
            },
            clip_repo.as_ref(),
        ),
        Commands::Import(a) => cli::import::run(
            &cli::import::ImportArgs {
                path: a.path,
                mode: a.mode,
                passphrase: a.passphrase,
                quiet: a.quiet,
            },
            clip_repo.as_ref(),
        ),
        Commands::Stats(a) => cli::stats::run(
            &cli::stats::StatsArgs {
                top: a.top,
                json: a.json,
            },
            clip_repo.as_ref(),
        ),
        Commands::Watch(a) => {
            let result = cli::watch::run(
                &cli::watch::WatchArgs {
                    pattern: a.pattern,
                    interval_ms: a.interval,
                    json: a.json,
                    quiet: a.quiet,
                },
                clip_repo.as_ref(),
            );
            if result.is_err() {
                result
            } else if cli::watch::was_stopped_by_signal() {
                // Clean SIGINT exit → conventional 130 (matches
                // `kill -INT $pid` / POSIX shells). 0 is reserved for
                // a real success path; SIGINT is the only way `run`
                // returns `Ok` here.
                std::process::exit(130);
            } else {
                Ok(())
            }
        }
    }
}

fn finish_selection(
    selected: Option<i64>,
    options: &cli::frontend::BrowseOptions,
    repo: &SqliteClipboardRepository,
) -> Result<(), String> {
    if let Some(id) = selected {
        cli::frontend::apply_action(id, options, repo)
    } else {
        std::process::exit(1);
    }
}

/// Translate the positional `content` arg into the typed `AddArgs` the
/// cli module consumes. The mapping is:
/// * `content == "-"`                          → `stdin: true`
/// * `content.starts_with('@')`                → `file: Some(stripped)`
/// * anything else                             → `text: Some(content)`
fn parse_add_source(content: &str, kind: Option<&str>) -> cli::add::AddArgs {
    if content == "-" {
        return cli::add::AddArgs {
            text: None,
            stdin: true,
            file: None,
            kind: kind.map(String::from),
        };
    }
    if let Some(stripped) = content.strip_prefix('@') {
        return cli::add::AddArgs {
            text: None,
            stdin: false,
            file: Some(stripped.to_string()),
            kind: kind.map(String::from),
        };
    }
    cli::add::AddArgs {
        text: Some(content.to_string()),
        stdin: false,
        file: None,
        kind: kind.map(String::from),
    }
}

/// Run `pin` / `unpin` and print the confirmation line unless `--quiet`.
///
/// `pinned` is what selects the verb, so both clap variants share one code
/// path and therefore cannot drift in what they do.
fn report_pin(
    args: &PinCliArgs,
    pinned: bool,
    repo: &dyn dezirclip::infrastructure::repository::clipboard_repo::ClipboardRepository,
) -> Result<(), String> {
    let id = cli::pin::run(
        &cli::pin::PinArgs {
            id: args.id.clone(),
            pinned,
        },
        repo,
    )?;
    if !args.quiet {
        println!("{}: id={id}", cli::pin::op_name(pinned));
    }
    Ok(())
}

/// Open both the clipboard repo and the tag repo, sharing a single
/// SQLite connection (so writes through one are visible to reads on
/// the other without an extra round-trip). Mirrors `setup.rs:371`
/// where the live app builds them as siblings from the same `DbState`.
fn open_repos() -> Result<(Arc<SqliteClipboardRepository>, Arc<SqliteTagRepository>), String> {
    let path = database_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {parent:?}: {e}"))?;
    }
    let conn = rusqlite::Connection::open(&path).map_err(|e| format!("open {path:?}: {e}"))?;
    // The app is usually running and holds its own connection to this file.
    // Without a busy timeout SQLite fails the first statement that collides
    // with a write in progress ("database is locked") instead of waiting, which
    // turned any concurrent `dzc add` into a spurious failure. Neither side
    // sets a timeout, so the CLI is the only place it can be fixed.
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| format!("busy_timeout: {e}"))?;
    let conn = Arc::new(Mutex::new(conn));
    {
        let mut guard = conn
            .lock()
            .map_err(|e| format!("migrations: lock poisoned: {e}"))?;
        migrations::ensure_supported_schema(&guard).map_err(|e| format!("migrations: {e}"))?;
        migrations::run_migrations(&mut guard).map_err(|e| format!("migrations: {e}"))?;
    }
    let clip_repo = SqliteClipboardRepository::new(conn.clone());
    let tag_repo = SqliteTagRepository::new(conn);
    Ok((Arc::new(clip_repo), Arc::new(tag_repo)))
}

fn database_path() -> Result<PathBuf, String> {
    database_path_from_override(
        std::env::var_os("DEZIRCLIP_DB_PATH")
            .or_else(|| std::env::var_os("TIEZ_DB_PATH"))
            .map(PathBuf::from),
    )
}

fn database_path_from_override(path: Option<PathBuf>) -> Result<PathBuf, String> {
    if let Some(path) = path {
        if path.as_os_str().is_empty() {
            return Err("database path override is empty".to_string());
        }
        return Ok(path);
    }
    let executable = std::env::current_exe().ok();
    Ok(dezirclip::infrastructure::data_paths::cli_database_path(
        &data_dir()?,
        &legacy_data_dir()?,
        executable.as_deref().and_then(std::path::Path::parent),
    ))
}

fn data_dir() -> Result<PathBuf, String> {
    dirs::data_dir().map(app_data_dir).ok_or_else(|| {
        "cannot locate the application data directory; set DEZIRCLIP_DB_PATH explicitly".to_string()
    })
}

fn app_data_dir(base: PathBuf) -> PathBuf {
    base.join(APP_IDENTIFIER)
}

fn legacy_app_data_dir(base: PathBuf) -> PathBuf {
    base.join(LEGACY_APP_IDENTIFIER)
}

fn legacy_data_dir() -> Result<PathBuf, String> {
    dirs::data_dir().map(legacy_app_data_dir).ok_or_else(|| {
        "cannot locate the legacy application data directory; set DEZIRCLIP_DB_PATH explicitly"
            .to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_add_source_stdin() {
        let args = parse_add_source("-", Some("text"));
        assert!(args.stdin);
        assert_eq!(args.kind, Some("text".to_string()));
        assert!(args.text.is_none());
        assert!(args.file.is_none());
    }

    #[test]
    fn test_parse_add_source_file() {
        let args = parse_add_source("@/tmp/a.txt", None);
        assert_eq!(args.file, Some("/tmp/a.txt".to_string()));
        assert!(!args.stdin);
        assert!(args.text.is_none());
    }

    #[test]
    fn test_database_path_honors_dezirclip_db_path() {
        let path = database_path_from_override(Some(PathBuf::from("/tmp/dezirclip-test.db")))
            .expect("db path");
        assert_eq!(path, PathBuf::from("/tmp/dezirclip-test.db"));
    }

    #[test]
    fn test_data_dir_uses_tauri_identifier() {
        assert_eq!(
            app_data_dir(PathBuf::from("/tmp/dezirclip-xdg")),
            PathBuf::from("/tmp/dezirclip-xdg").join(APP_IDENTIFIER)
        );
    }
}
