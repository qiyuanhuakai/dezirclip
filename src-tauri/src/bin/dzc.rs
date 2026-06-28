//! `dzc` — clipboard manager CLI binary.
//!
//! Wire-up:
//! * clap parses `Cli` → `Commands::*` (each variant owns its `*Args`).
//! * The matching `cli::<cmd>::run(args, ...)` is invoked.
//! * `list` / `search` / `get` / `add` / `delete` / `tag` are real
//!   implementations (Tasks 56 + 57).
//! * `pin` / `unpin` / `export` / `import` / `stats` / `watch` are
//!   placeholder `println!` for now — Task 58 fills them in.
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
use dezirclip::domain::models::ClipboardEntry;
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
    Pin(PinArgs),
    /// Unpin an entry.
    Unpin(UnpinArgs),
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
}

#[derive(Args)]
struct ListArgs {
    /// Optional positional limit. `-1` returns every row.
    limit: Option<i32>,
    #[arg(long)]
    kind: Option<String>,
    #[arg(long)]
    tag: Option<String>,
    #[arg(long)]
    pinned: bool,
    #[arg(long)]
    json: bool,
    #[arg(long)]
    ids: bool,
    #[arg(long, short)]
    quiet: bool,
}

#[derive(Args)]
struct SearchArgs {
    query: String,
    #[arg(long, value_enum, default_value_t = CliSearchMode::Contains)]
    mode: CliSearchMode,
    #[arg(long)]
    limit: Option<i32>,
    #[arg(long)]
    json: bool,
    #[arg(long, short)]
    quiet: bool,
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

#[derive(Args)]
struct PinArgs {
    id: String,
}

#[derive(Args)]
struct UnpinArgs {
    id: String,
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

fn main() {
    let cli_args = Cli::parse();
    if let Err(e) = run(cli_args) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run(args: Cli) -> Result<(), String> {
    let (clip_repo, tag_repo) = open_repos()?;
    match args.command {
        Commands::List(a) => cli::list::run(
            &cli::list::ListArgs {
                limit: a.limit,
                kind: a.kind,
                tag: a.tag,
                pinned: a.pinned,
                json: a.json,
                ids: a.ids,
                quiet: a.quiet,
            },
            clip_repo.as_ref(),
        ),
        Commands::Search(a) => cli::search::run(
            &cli::search::SearchArgs {
                query: a.query,
                mode: a.mode.into(),
                limit: a.limit,
                json: a.json,
                quiet: a.quiet,
            },
            clip_repo.as_ref(),
        ),
        Commands::Get(a) => cli::get::run(
            &cli::get::GetArgs {
                id: a.id,
                preview: a.preview,
                json: a.json,
                quiet: a.quiet,
            },
            clip_repo.as_ref(),
        ),
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
        Commands::Pin(a) => {
            println!("pin: id={}", a.id);
            Ok(())
        }
        Commands::Unpin(a) => {
            println!("unpin: id={}", a.id);
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

/// Open both the clipboard repo and the tag repo, sharing a single
/// SQLite connection (so writes through one are visible to reads on
/// the other without an extra round-trip). Mirrors `setup.rs:371`
/// where the live app builds them as siblings from the same `DbState`.
fn open_repos() -> Result<(Arc<SqliteClipboardRepository>, Arc<SqliteTagRepository>), String> {
    let path = database_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {parent:?}: {e}"))?;
    }
    let conn = Arc::new(Mutex::new(
        rusqlite::Connection::open(&path).map_err(|e| format!("open {path:?}: {e}"))?,
    ));
    {
        let mut guard = conn
            .lock()
            .map_err(|e| format!("migrations: lock poisoned: {e}"))?;
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
    let dir = data_dir()?;
    let db_path = dir.join("clipboard.db");
    if db_path.exists() {
        return Ok(db_path);
    }
    let legacy_db_path = legacy_data_dir()?.join("clipboard.db");
    if legacy_db_path.exists() {
        return Ok(legacy_db_path);
    }
    Ok(db_path)
}

fn data_dir() -> Result<PathBuf, String> {
    #[cfg(target_os = "linux")]
    {
        if let Some(p) = std::env::var_os("XDG_DATA_HOME") {
            return Ok(app_data_dir(PathBuf::from(p)));
        }
        if let Some(home) = std::env::var_os("HOME") {
            return Ok(app_data_dir(PathBuf::from(home).join(".local/share")));
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(p) = std::env::var_os("APPDATA") {
            return Ok(app_data_dir(PathBuf::from(p)));
        }
    }
    Ok(app_data_dir(std::env::temp_dir()))
}

fn app_data_dir(base: PathBuf) -> PathBuf {
    base.join(APP_IDENTIFIER)
}

fn legacy_app_data_dir(base: PathBuf) -> PathBuf {
    base.join(LEGACY_APP_IDENTIFIER)
}

fn legacy_data_dir() -> Result<PathBuf, String> {
    #[cfg(target_os = "linux")]
    {
        if let Some(p) = std::env::var_os("XDG_DATA_HOME") {
            return Ok(legacy_app_data_dir(PathBuf::from(p)));
        }
        if let Some(home) = std::env::var_os("HOME") {
            return Ok(legacy_app_data_dir(PathBuf::from(home).join(".local/share")));
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(p) = std::env::var_os("APPDATA") {
            return Ok(legacy_app_data_dir(PathBuf::from(p)));
        }
    }
    Ok(legacy_app_data_dir(std::env::temp_dir()))
}

// Re-export so the placeholder `add` etc. can use the domain types
// without an extra `use` once Tasks 57–58 fill them in.
#[allow(dead_code)]
type _Entry = ClipboardEntry;

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
        let path =
            database_path_from_override(Some(PathBuf::from("/tmp/dezirclip-test.db"))).expect("db path");
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
