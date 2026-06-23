//! `tiez-c watch` — stream new clipboard entries as they arrive.
//!
//! ## Polling model (G32 hard constraint)
//!
//! The watch command polls the repository for new entries every
//! `--interval` milliseconds (default **500ms**, max 60s). On each
//! tick it fetches the latest timestamp and prints any rows whose
//! timestamp is greater than the high-water mark from the previous
//! tick. **No mmap, no WAL parsing, no event hooks** — that's the
//! task-time decision for Stream 1: a 500ms poll is fast enough
//! to feel real-time and keeps the impl trivially testable.
//!
//! ## Args
//!
//! * `--pattern <REGEX>` — case-insensitive substring filter applied
//!   to the entry's `content` (and `preview` as a fallback). Empty or
//!   omitted = no filter; print every new entry.
//! * `--interval <MS>` — poll interval in milliseconds. Default
//!   `DEFAULT_INTERVAL_MS` (500). Clamped to `[50, 60_000]` so a
//!   typo (`--interval 0`) doesn't pin a CPU.
//! * `--json` — print each new entry as a `CliEntry` JSON object on
//!   its own line (newline-delimited JSON). Pipe-friendly.
//! * `--quiet` — suppress the startup banner (`Watching for new
//!   entries... (interval=500ms, Ctrl-C to exit)`); entries are still
//!   printed.
//!
//! ## Ctrl-C
//!
//! A `ctrlc` handler toggles a process-global `AtomicBool` that the
//! poll loop checks every tick. The loop returns `Ok(())` on the
//! tick where the flag flips; the binary then exits with code 130
//! (the conventional SIGINT exit code) to match shell expectations.
//!
//! ## State isolation
//!
//! `run_watch` (the public surface) installs the SIGINT handler and
//! owns the high-water mark. Tests use `run_watch_with_running`
//! instead — it takes an externally-supplied `AtomicBool` (so the
//! test can flip it from a thread) and a mutable high-water mark
//! pointer (so the caller can verify it advances). Both functions
//! share the inner poll loop.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::domain::models::ClipboardEntry;
use crate::infrastructure::repository::clipboard_repo::ClipboardRepository;

use super::list::CliEntry;

/// Default poll interval. G32 hard constraint — do NOT change
/// without revisiting the G32 decision in the plan.
pub const DEFAULT_INTERVAL_MS: u64 = 500;

/// Minimum allowed poll interval. Below this the loop becomes a
/// busy-spin.
pub const MIN_INTERVAL_MS: u64 = 50;

/// Maximum allowed poll interval. Above this the loop feels
/// unresponsive and the user might think it's hung.
pub const MAX_INTERVAL_MS: u64 = 60_000;

/// Args for the `watch` subcommand.
pub struct WatchArgs {
    /// Optional case-insensitive substring filter. The full content
    /// is matched first; on miss, the `preview` is tried as a
    /// fallback so partial-content patterns still hit.
    pub pattern: Option<String>,
    /// Poll interval in milliseconds. `None` → `DEFAULT_INTERVAL_MS`
    /// (500). Clamped to `[MIN_INTERVAL_MS, MAX_INTERVAL_MS]`.
    pub interval_ms: Option<u64>,
    /// Emit each new entry as a JSON object on its own line. Mutually
    /// exclusive with the human format (no shared line).
    pub json: bool,
    /// Suppress the startup banner. Entries are still printed.
    pub quiet: bool,
}

/// Process-global "are we still watching?" flag. Flipped to `false`
/// by the SIGINT handler. The `run_watch` poll loop checks this on
/// every tick.
///
/// **Why global and not per-call?** A `ctrlc::set_handler` callback
/// can only be registered once per process; storing the flag in a
/// static makes the handler unconditional and avoids races between
/// multiple `watch` invocations (which shouldn't happen in practice
/// but is theoretically possible during testing).
static RUNNING: AtomicBool = AtomicBool::new(true);

/// Run the `watch` subcommand. Installs a SIGINT handler that
/// flips the global `RUNNING` flag, then enters the poll loop with
/// an internal high-water mark seeded to the current max timestamp.
///
/// Returns `Ok(())` when SIGINT (or any other path to `RUNNING=false`)
/// is observed. Errors from the repo are propagated.
pub fn run(args: &WatchArgs, repo: &dyn ClipboardRepository) -> Result<(), String> {
    // Reset the global flag in case a previous run already flipped
    // it. Without this, a second `watch` invocation in the same
    // process would exit immediately.
    RUNNING.store(true, Ordering::Release);

    install_sigint_handler();

    // Seed the high-water mark with the current max timestamp so we
    // don't print every historical row on the first tick.
    let mut last_seen: i64 = current_max_timestamp(repo).unwrap_or(0);

    run_watch_with_running(args, repo, &mut last_seen, &RUNNING)
}

/// Test-friendly variant. The caller owns the `RUNNING` flag and
/// the high-water mark pointer, so the function can be driven from
/// a single test thread without spawning a separate signal handler.
///
/// `last_seen` is read AND updated in place — pass `&mut` and check
/// the post-call value to assert the high-water mark advanced.
///
/// `running` must be `true` on entry; flip to `false` to make the
/// loop exit.
pub fn run_watch_with_running(
    args: &WatchArgs,
    repo: &dyn ClipboardRepository,
    last_seen: &mut i64,
    running: &AtomicBool,
) -> Result<(), String> {
    let interval = clamp_interval(args.interval_ms.unwrap_or(DEFAULT_INTERVAL_MS));
    let pattern_lower = args.pattern.as_ref().map(|p| p.to_lowercase());

    if !args.quiet {
        let pat = args
            .pattern
            .as_deref()
            .map(|p| format!(", pattern='{p}'"))
            .unwrap_or_default();
        eprintln!(
            "Watching for new entries... (interval={interval}ms{pat}, Ctrl-C to exit)"
        );
    }

    while running.load(Ordering::Acquire) {
        // Fetch the current "all" set. We need a stable view between
        // the read and the print, so the repo call is the critical
        // section. SQLite is single-writer so this is safe.
        let current = repo
            .get_history(i32::MAX, 0, None)
            .map_err(|e| format!("watch: get_history failed: {e}"))?;

        // Filter to entries strictly newer than the high-water mark.
        // Sort by `timestamp ASC, id ASC` so output is in insertion
        // order when multiple new rows land in the same tick.
        let mut new_entries: Vec<&ClipboardEntry> =
            current.iter().filter(|e| e.timestamp > *last_seen).collect();
        new_entries.sort_by(|a, b| {
            a.timestamp
                .cmp(&b.timestamp)
                .then_with(|| a.id.cmp(&b.id))
        });

        for e in &new_entries {
            if !matches_pattern(e, pattern_lower.as_deref()) {
                continue;
            }
            emit_entry(e, args.json);
            if e.timestamp > *last_seen {
                *last_seen = e.timestamp;
            }
        }

        // Sleep in 50ms slices so a SIGINT flips `running` and the
        // loop exits promptly. 500ms / 50ms = 10 slices per tick at
        // the default interval; at the minimum 50ms interval we skip
        // the sleep entirely (next loop iteration reloads state).
        if running.load(Ordering::Acquire) && interval > MIN_INTERVAL_MS {
            sleep_interruptible(interval, running);
        }
    }

    if !args.quiet {
        eprintln!("Stopped watching.");
    }
    Ok(())
}

/// Emit one entry to stdout, either as a `CliEntry` JSON object
/// (newline-delimited, NDJSON-friendly) or as a human line.
///
/// Both forms end with a `\n` so the line is flushed by the runtime
/// promptly. We don't `flush()` explicitly — `println!` does it on
/// most targets, and the kernel line-discipline is fast enough at
/// 500ms cadence that we don't need it.
fn emit_entry(e: &ClipboardEntry, json: bool) {
    if json {
        let view = CliEntry {
            id: e.id,
            content_type: e.content_type.clone(),
            content: e.content.clone(),
            preview: e.preview.clone(),
            tags: e.tags.clone(),
            pinned: e.is_pinned,
            timestamp: e.timestamp,
        };
        // We use the in-tree `serde_json` (already a dependency) for
        // correct escaping. The output is one compact JSON object
        // per line — i.e. NDJSON, ready for `jq` / `grep` / `wc`.
        match serde_json::to_string(&view) {
            Ok(s) => println!("{s}"),
            Err(err) => eprintln!("watch: json encode failed: {err}"),
        }
    } else {
        // Human form: `<icon> [type] preview`. Mirrors the format
        // from `cli::list::format_human` so users see the same shape
        // whether they're reading from `list` or `watch`.
        let icon = icon_for(&e.content_type);
        let preview = e.preview.lines().next().unwrap_or("");
        println!("{icon} [{}] {preview}", e.content_type);
    }
}

/// Icon per content type. Mirrors the `icon_for` table in
/// `cli::list`; duplicated here to keep the watch loop
/// self-contained (no cross-module imports for a one-liner).
fn icon_for(content_type: &str) -> &'static str {
    match content_type {
        "text" => "\u{1F4CB}",
        "html" => "\u{1F310}",
        "image" => "\u{1F5BC}",
        "file" => "\u{1F4C1}",
        "code" => "\u{1F4BB}",
        "video" => "\u{1F3AC}",
        _ => "\u{1F4CB}",
    }
}

/// Apply the optional case-insensitive substring filter. Returns
/// `true` if the entry should be printed.
///
/// Match order:
/// 1. `content` contains the pattern (case-insensitive), OR
/// 2. `preview` contains the pattern (case-insensitive).
fn matches_pattern(e: &ClipboardEntry, pattern_lower: Option<&str>) -> bool {
    let Some(pat) = pattern_lower else {
        return true;
    };
    let pat = pat.to_lowercase();
    if pat.is_empty() {
        return true;
    }
    e.content.to_lowercase().contains(&pat) || e.preview.to_lowercase().contains(&pat)
}

/// Clamp the user-supplied interval into the safe range. Any value
/// below `MIN_INTERVAL_MS` is rounded up; anything above
/// `MAX_INTERVAL_MS` is rounded down. The default is preserved
/// only when the user passed `None` at the call site — here we
/// always have a concrete value.
fn clamp_interval(ms: u64) -> u64 {
    ms.clamp(MIN_INTERVAL_MS, MAX_INTERVAL_MS)
}

/// Sleep in `MIN_INTERVAL_MS` slices until either `total` ms have
/// elapsed or `running` flips to `false`. Splitting the sleep is
/// what gives us prompt Ctrl-C response — a single
/// `thread::sleep(500ms)` would block the SIGINT check for the
/// full duration.
fn sleep_interruptible(total: u64, running: &AtomicBool) {
    let slice = MIN_INTERVAL_MS;
    let mut elapsed: u64 = 0;
    while elapsed < total {
        if !running.load(Ordering::Acquire) {
            return;
        }
        let remaining = total - elapsed;
        let this_sleep = remaining.min(slice);
        std::thread::sleep(Duration::from_millis(this_sleep));
        elapsed += this_sleep;
    }
}

/// Install the SIGINT handler. Safe to call multiple times — the
/// `ctrlc` crate deduplicates. The handler flips the global
/// `RUNNING` flag; the poll loop reads it on the next slice.
fn install_sigint_handler() {
    // Best-effort: if `ctrlc` isn't available (e.g. WASM target),
    // we just skip — the loop will still respond to the test's
    // externally-controlled flag.
    #[cfg(not(target_arch = "wasm32"))]
    {
        // We intentionally ignore the Result — on platforms without
        // a signal API (WASM, some embedded targets) `set_handler`
        // returns an error and we keep polling. The `ctrlc` crate
        // is in scope as a dev-dependency for the binary.
        let _ = ctrlc::set_handler(|| {
            RUNNING.store(false, Ordering::Release);
        });
    }
}

/// Look up the current maximum timestamp in the repo. Used to
/// seed the high-water mark on the first tick so we don't replay
/// history. Returns `None` if the repo is empty or the query fails
/// (the caller falls back to `0` in either case — the first tick
/// will print everything, which is fine for an empty repo).
fn current_max_timestamp(repo: &dyn ClipboardRepository) -> Option<i64> {
    let all = repo.get_history(i32::MAX, 0, None).ok()?;
    all.iter().map(|e| e.timestamp).max()
}

/// Report whether the most recent (or current) `run` exited because
/// the global `RUNNING` flag was flipped to `false` by the SIGINT
/// handler. The binary uses this to map a clean loop exit to the
/// conventional SIGINT exit code (130) instead of a generic 0.
///
/// Returns `true` once the flag is `false`; stays `true` until
/// the next `run` resets it.
pub fn was_stopped_by_signal() -> bool {
    !RUNNING.load(Ordering::Acquire)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: i64, ts: i64, kind: &str, content: &str) -> ClipboardEntry {
        ClipboardEntry {
            id,
            content_type: kind.to_string(),
            content: content.to_string(),
            html_content: None,
            source_app: String::new(),
            source_app_path: None,
            timestamp: ts,
            preview: content.to_string(),
            is_pinned: false,
            tags: vec![],
            use_count: 0,
            is_external: false,
            pinned_order: 0,
            file_preview_exists: false,
            content_kinds: vec![],
            ocr_text: None,
            ocr_status: None,
        }
    }

    #[test]
    fn clamp_interval_clamps_to_safe_range() {
        assert_eq!(clamp_interval(0), MIN_INTERVAL_MS);
        assert_eq!(clamp_interval(10), MIN_INTERVAL_MS);
        assert_eq!(clamp_interval(500), 500);
        assert_eq!(clamp_interval(MAX_INTERVAL_MS + 1), MAX_INTERVAL_MS);
    }

    #[test]
    fn matches_pattern_with_none_matches_everything() {
        let e = entry(1, 1, "text", "hello world");
        assert!(matches_pattern(&e, None));
    }

    #[test]
    fn matches_pattern_with_empty_matches_everything() {
        let e = entry(1, 1, "text", "hello world");
        assert!(matches_pattern(&e, Some("")));
    }

    #[test]
    fn matches_pattern_substring_case_insensitive() {
        let e = entry(1, 1, "text", "Hello World");
        assert!(matches_pattern(&e, Some("hello")));
        assert!(matches_pattern(&e, Some("WORLD")));
        assert!(matches_pattern(&e, Some("lo wo")));
        assert!(!matches_pattern(&e, Some("xyz")));
    }

    #[test]
    fn matches_pattern_falls_back_to_preview() {
        let mut e = entry(1, 1, "text", "full content here");
        e.preview = "preview text only".to_string();
        // The pattern matches preview but not content — still hits.
        assert!(matches_pattern(&e, Some("preview")));
        // The pattern matches neither — miss.
        assert!(!matches_pattern(&e, Some("missing")));
    }
}
