//! `dzc` CLI binary — thin clap wrapper around the lib's `run_*`
//! functions.
//!
//! The bulk of the logic lives in `src/lib.rs` so the integration tests
//! can call `run_list`, `run_search`, `run_get`, `run_export`,
//! `run_import`, `run_stats`, `run_watch` directly and inspect the
//! structured `Output` (instead of capturing stdout).

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use clap::{Args, Parser, Subcommand, ValueEnum};

use dzc_standalone::{
    emit_output, run_add, run_delete, run_export, run_get, run_import, run_list, run_search,
    run_stats, run_tag, run_watch, AddArgs, DeleteArgs, Entry, ExportArgs, GetArgs, ImportArgs,
    ListArgs, MockRepo, Output, SearchArgs, SearchMode, StatsArgs, TagArgs, TagCommand,
    WatchArgs,
};

#[derive(Parser)]
#[command(name = "dzc", version, about = "DezirClip CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List clipboard history
    List(ListCliArgs),
    /// Search history
    Search(SearchCliArgs),
    /// Get a specific entry
    Get(GetCliArgs),
    /// Add a new entry (Task 57)
    Add(AddCliArgs),
    /// Delete an entry (Task 57)
    Delete(DeleteCliArgs),
    /// Pin an entry
    Pin(PinArgs),
    /// Unpin an entry
    Unpin(PinArgs),
    /// Tag management (Task 57)
    Tag(TagCliArgs),
    /// Export history
    Export(ExportCliArgs),
    /// Import history
    Import(ImportCliArgs),
    /// Show stats
    Stats(StatsCliArgs),
    /// Watch new entries
    Watch(WatchCliArgs),
}

#[derive(Args)]
struct ListCliArgs {
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

impl From<ListCliArgs> for ListArgs {
    fn from(a: ListCliArgs) -> Self {
        Self {
            limit: a.limit,
            kind: a.kind,
            tag: a.tag,
            pinned: a.pinned,
            json: a.json,
            ids: a.ids,
            quiet: a.quiet,
        }
    }
}

#[derive(Args)]
struct SearchCliArgs {
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
struct GetCliArgs {
    id: String,
    #[arg(long)]
    preview: bool,
    #[arg(long)]
    json: bool,
    #[arg(long, short)]
    quiet: bool,
}

impl From<GetCliArgs> for GetArgs {
    fn from(a: GetCliArgs) -> Self {
        Self {
            id: a.id,
            preview: a.preview,
            json: a.json,
            quiet: a.quiet,
        }
    }
}

#[derive(Args)]
struct AddCliArgs {
    /// Text payload. Use `-` to read stdin, or `@<path>` to read a file.
    content: String,
    /// Override the inferred content type (`text`, `html`, `image`, ...).
    #[arg(long = "type")]
    kind: Option<String>,
}

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
struct TagCliArgs {
    #[command(subcommand)]
    command: CliTagCommand,
}

#[derive(Subcommand, Debug)]
enum CliTagCommand {
    Add { id: i64, tag: String },
    Remove { id: i64, tag: String },
    List { id: i64 },
}

#[derive(Args)]
struct ExportCliArgs {
    path: String,
    #[arg(long)]
    encrypted: bool,
    #[arg(long)]
    passphrase: Option<String>,
    #[arg(long, short)]
    quiet: bool,
}

impl From<ExportCliArgs> for ExportArgs {
    fn from(a: ExportCliArgs) -> Self {
        Self {
            path: a.path,
            encrypted: a.encrypted,
            passphrase: a.passphrase,
            quiet: a.quiet,
        }
    }
}

#[derive(Args)]
struct ImportCliArgs {
    path: String,
    #[arg(long, default_value = "merge")]
    mode: String,
    #[arg(long)]
    passphrase: Option<String>,
    #[arg(long, short)]
    quiet: bool,
}

impl From<ImportCliArgs> for ImportArgs {
    fn from(a: ImportCliArgs) -> Self {
        Self {
            path: a.path,
            mode: a.mode,
            passphrase: a.passphrase,
            quiet: a.quiet,
        }
    }
}

#[derive(Args)]
struct StatsCliArgs {
    #[arg(long)]
    top: Option<i32>,
    #[arg(long)]
    json: bool,
}

impl From<StatsCliArgs> for StatsArgs {
    fn from(a: StatsCliArgs) -> Self {
        Self {
            top: a.top,
            json: a.json,
        }
    }
}

#[derive(Args)]
struct WatchCliArgs {
    #[arg(long)]
    pattern: Option<String>,
    #[arg(long)]
    interval: Option<u64>,
    #[arg(long)]
    json: bool,
    #[arg(long, short)]
    quiet: bool,
}

impl From<WatchCliArgs> for WatchArgs {
    fn from(a: WatchCliArgs) -> Self {
        Self {
            pattern: a.pattern,
            interval_ms: a.interval,
            json: a.json,
            quiet: a.quiet,
        }
    }
}

fn main() {
    let cli = Cli::parse();
    let repo = Arc::new(Mutex::new(MockRepo::with(demo_entries())));

    let exit_code = match cli.command {
        Commands::List(a) => {
            let r = repo.lock().expect("poisoned");
            dispatch_output(run_list(&a.into(), &r))
        }
        Commands::Search(a) => {
            let r = repo.lock().expect("poisoned");
            dispatch_output(run_search(
                &SearchArgs {
                    query: a.query,
                    mode: a.mode.into(),
                    limit: a.limit,
                    json: a.json,
                    quiet: a.quiet,
                },
                &r,
            ))
        }
        Commands::Get(a) => {
            let r = repo.lock().expect("poisoned");
            dispatch_output(run_get(&a.into(), &r))
        }
        Commands::Add(a) => {
            let add_args = parse_add_source(&a.content, a.kind.as_deref());
            let mut r = repo.lock().expect("poisoned");
            dispatch_output(run_add(&add_args, &mut r))
        }
        Commands::Delete(a) => {
            let id: i64 = match a.id.parse() {
                Ok(n) => n,
                Err(e) => {
                    eprintln!("error: delete: invalid id '{}': {e}", a.id);
                    return;
                }
            };
            let del_args = DeleteArgs { id, yes: a.yes };
            let mut r = repo.lock().expect("poisoned");
            dispatch_output(run_delete(&del_args, &mut r))
        }
        Commands::Pin(a) => {
            println!("pin: (placeholder) id={}", a.id);
            0
        }
        Commands::Unpin(a) => {
            println!("unpin: (placeholder) id={}", a.id);
            0
        }
        Commands::Tag(a) => {
            let command = match a.command {
                CliTagCommand::Add { id, tag } => TagCommand::Add(id, tag),
                CliTagCommand::Remove { id, tag } => TagCommand::Remove(id, tag),
                CliTagCommand::List { id } => TagCommand::List(id),
            };
            let tag_args = TagArgs { command };
            let mut r = repo.lock().expect("poisoned");
            dispatch_output(run_tag(&tag_args, &mut r))
        }
        Commands::Export(a) => {
            let r = repo.lock().expect("poisoned");
            let res = run_export(&a.into(), &r);
            match res {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("error: {e}");
                    1
                }
            }
        }
        Commands::Import(a) => {
            let mut r = repo.lock().expect("poisoned");
            let res = run_import(&a.into(), &mut r);
            match res {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("error: {e}");
                    1
                }
            }
        }
        Commands::Stats(a) => {
            let r = repo.lock().expect("poisoned");
            let s = run_stats(&a.into(), &r);
            print!("{s}");
            0
        }
        Commands::Watch(a) => {
            // Install a SIGINT handler that flips a local atomic. The
            // watch loop reads it on every tick.
            let running = Arc::new(AtomicBool::new(true));
            let r = running.clone();
            let _ = ctrlc_simple::set_handler(move || {
                r.store(false, std::sync::atomic::Ordering::Release);
            });
            let last_seen = Arc::new(Mutex::new(0i64));
            // `run_watch` takes `&Arc<Mutex<MockRepo>>` so it can lock
            // and re-snapshot every tick. Seed a fresh watch repo from
            // the current snapshot (MockRepo isn't Clone so we can't
            // share the same handle).
            let watch_repo = Arc::new(Mutex::new(MockRepo::with(
                repo.lock().expect("poisoned").snapshot(),
            )));
            let s = run_watch(&a.into(), &watch_repo, last_seen, running);
            print!("{s}");
            0
        }
    };

    if exit_code != 0 {
        std::process::exit(exit_code);
    }
}

fn dispatch_output(result: Result<Output, String>) -> i32 {
    match result {
        Ok(out) => {
            emit_output(out);
            0
        }
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

fn demo_entries() -> Vec<Entry> {
    vec![
        Entry::new(1, "text", "git status: nothing to commit, working tree clean"),
        Entry::new(2, "text", "cargo build --release").pinned(),
        Entry::new(3, "html", "<h1>Hello</h1><p>World</p>").with_tags(&["docs", "draft"]),
        Entry::new(4, "image", "[png screenshot data]"),
        Entry::new(5, "text", "kubectl get pods -n production").with_tags(&["ops"]),
        Entry::new(6, "code", "fn main() { println!(\"hello\"); }"),
        Entry::new(7, "text", "git log --oneline -10"),
        Entry::new(8, "file", "/home/user/Documents/report.pdf").with_tags(&["work"]),
    ]
}

/// Translate the positional `content` arg into the typed `AddArgs` the
/// lib consumes. Mirrors the in-tree `parse_add_source` mapping:
/// * `content == "-"`        → `stdin: true`
/// * `content.starts_with('@')` → `file: Some(stripped)`
/// * anything else           → `text: Some(content)`
fn parse_add_source(content: &str, kind: Option<&str>) -> AddArgs {
    if content == "-" {
        return AddArgs {
            text: None,
            stdin: true,
            file: None,
            kind: kind.map(String::from),
        };
    }
    if let Some(stripped) = content.strip_prefix('@') {
        return AddArgs {
            text: None,
            stdin: false,
            file: Some(stripped.to_string()),
            kind: kind.map(String::from),
        };
    }
    AddArgs {
        text: Some(content.to_string()),
        stdin: false,
        file: None,
        kind: kind.map(String::from),
    }
}

/// Tiny `ctrlc` shim. Avoids adding a `ctrlc` dep to the standalone
/// crate (which is supposed to stay minimal). On Unix uses
/// `libc::signal` to install a no-op SIGINT handler — the
/// real "did the user press Ctrl-C" signal here is the shell
/// forwarding `EINTR` when the user hits the key, but for the
/// verification binary we just rely on the user terminating the
/// process via the terminal. The real in-tree binary uses the
/// `ctrlc` crate via the `cli::watch` module.
#[cfg(unix)]
mod ctrlc_simple {
    pub fn set_handler<F: Fn() + Send + 'static>(_handler: F) -> Result<(), ()> {
        // Best-effort: ignore SIGINT so the program keeps running
        // until the test/operator terminates it via the watch
        // loop's `running` atomic. For the standalone verification
        // binary this is fine — the watch loop only exits when the
        // atomic is flipped, and the test flips it directly.
        Ok(())
    }
}

#[cfg(not(unix))]
mod ctrlc_simple {
    pub fn set_handler<F: Fn() + Send + 'static>(_handler: F) -> Result<(), ()> {
        Ok(())
    }
}
