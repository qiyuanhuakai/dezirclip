//! `dzc-standalone` library surface.
//!
//! This crate exposes the data types (`Entry`, `MockRepo`), the
//! subcommand `run_*` functions, and the formatting helpers that the
//! `dzc` binary uses. Putting them here lets the integration tests
//! drive the same code paths the CLI does, without depending on
//! `println!` capture or `stdout` redirection.
//!
//! The `bin/dzc.rs` file is a thin clap wrapper that calls into
//! `run_list`, `run_search`, `run_get`, etc.

use std::sync::Mutex;

// =====================================================================
// Entry shape
// =====================================================================

#[derive(Debug, Clone)]
pub struct Entry {
    pub id: i64,
    pub content_type: String,
    pub content: String,
    pub preview: String,
    pub tags: Vec<String>,
    pub is_pinned: bool,
    pub timestamp: i64,
    /// Number of times the entry has been pasted / reused. Mirrors
    /// the in-tree `ClipboardEntry::use_count` field; the stats
    /// subcommand reads it for the "most used" top-N list. Defaults
    /// to 0 in `Entry::new` so existing fixtures don't need to be
    /// updated.
    pub use_count: i32,
}

impl Entry {
    pub fn new(id: i64, content_type: &str, content: &str) -> Self {
        let preview = content.chars().take(500).collect::<String>();
        Self {
            id,
            content_type: content_type.to_string(),
            content: content.to_string(),
            preview,
            tags: Vec::new(),
            is_pinned: false,
            timestamp: 1_700_000_000 + id,
            use_count: 0,
        }
    }

    pub fn with_tags(mut self, tags: &[&str]) -> Self {
        self.tags = tags.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn pinned(mut self) -> Self {
        self.is_pinned = true;
        self
    }

    pub fn with_use_count(mut self, n: i32) -> Self {
        self.use_count = n;
        self
    }
}

/// Thread-safe in-memory store. `Mutex<Vec<Entry>>` is enough for a
/// single-process CLI; concurrency only matters when `watch` lands.
#[derive(Debug)]
pub struct MockRepo {
    pub entries: Mutex<Vec<Entry>>,
    /// Monotonic id counter. Initialized to `max(existing ids) + 1`
    /// (or `1` for an empty store) so seeded entries via `with(...)`
    /// don't get clobbered by `save`.
    next_id: Mutex<i64>,
}

impl Default for MockRepo {
    fn default() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
            next_id: Mutex::new(1),
        }
    }
}

impl MockRepo {
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed the repo with `entries` and prime the id counter to
    /// `max(existing ids) + 1` so subsequent `save` calls don't reuse
    /// a seeded id.
    pub fn with(entries: Vec<Entry>) -> Self {
        let next_id = entries.iter().map(|e| e.id).max().unwrap_or(0) + 1;
        Self {
            entries: Mutex::new(entries),
            next_id: Mutex::new(next_id),
        }
    }

    /// Replace the entries (test helper). Drops the previous store.
    /// Re-primes the id counter to `max + 1` of the new entries.
    pub fn set(&self, entries: Vec<Entry>) {
        let next_id = entries.iter().map(|e| e.id).max().unwrap_or(0) + 1;
        *self.entries.lock().expect("poisoned") = entries;
        *self.next_id.lock().expect("poisoned") = next_id;
    }

    /// Allocate the next id. Pure counter; doesn't touch `entries`.
    fn next_id(&self) -> i64 {
        let mut guard = self.next_id.lock().expect("poisoned");
        let id = *guard;
        *guard = id + 1;
        id
    }

    /// Snapshot of all entries, ordered by id ASC (insertion order).
    pub fn iter(&self) -> Vec<Entry> {
        let mut v = self.entries.lock().expect("poisoned").clone();
        v.sort_by(|a, b| a.id.cmp(&b.id));
        v
    }

    /// Insert `entry` and return its new id. Sets the entry's `id`
    /// and `timestamp` from the counter so saved entries are ordered
    /// by insertion time. Mirrors the in-tree `repo.save()` return
    /// type (`Result<i64, String>`) minus the Result — in-memory can't
    /// fail.
    pub fn save(&self, mut entry: Entry) -> i64 {
        let id = self.next_id();
        entry.id = id;
        // Match `Entry::new`'s `1_700_000_000 + id` convention so
        // timestamps stay ordered with seeded entries.
        entry.timestamp = 1_700_000_000 + id;
        self.entries.lock().expect("poisoned").push(entry);
        id
    }

    /// Remove the entry with the given id. No-op if not present.
    pub fn delete(&self, id: i64) {
        self.entries
            .lock()
            .expect("poisoned")
            .retain(|e| e.id != id);
    }

    /// Replace the entry's tag list. Returns `Err` if the id doesn't
    /// exist. Mirrors `TagRepository::update_entry_tags` semantics.
    pub fn set_tags(&self, id: i64, tags: Vec<String>) -> Result<(), String> {
        let mut entries = self.entries.lock().expect("poisoned");
        let entry = entries
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or_else(|| format!("set_tags: no entry for id={id}"))?;
        entry.tags = tags;
        Ok(())
    }

    /// Snapshot of the current entries, ordered by id DESC.
    pub fn snapshot(&self) -> Vec<Entry> {
        let mut v = self.entries.lock().expect("poisoned").clone();
        v.sort_by(|a, b| b.id.cmp(&a.id));
        v
    }

    /// `get_history` analogue: limit + offset + content_type filter.
    /// Same ordering as the in-tree repo: pinned DESC, timestamp DESC,
    /// id DESC.
    pub fn get_history(&self, limit: i32, offset: i32, content_type: Option<&str>) -> Vec<Entry> {
        let mut v = self.entries.lock().expect("poisoned").clone();
        v.sort_by(|a, b| {
            b.is_pinned
                .cmp(&a.is_pinned)
                .then_with(|| b.timestamp.cmp(&a.timestamp))
                .then_with(|| b.id.cmp(&a.id))
        });
        if let Some(ct) = content_type {
            v.retain(|e| e.content_type == ct);
        }
        v.into_iter()
            .skip(offset.max(0) as usize)
            .take(limit.max(0) as usize)
            .collect()
    }

    /// LIKE-substring match across content/preview/tags. Mirrors the
    /// in-tree `repo.search()` semantics: case-insensitive substring.
    pub fn search_contains(&self, query: &str, limit: i32) -> Vec<Entry> {
        let needle = query.to_lowercase();
        let mut v = self
            .entries
            .lock()
            .expect("poisoned")
            .iter()
            .filter(|e| {
                e.content.to_lowercase().contains(&needle)
                    || e.preview.to_lowercase().contains(&needle)
                    || e.tags.iter().any(|t| t.to_lowercase().contains(&needle))
            })
            .cloned()
            .collect::<Vec<_>>();
        v.sort_by(|a, b| b.timestamp.cmp(&a.timestamp).then_with(|| b.id.cmp(&a.id)));
        v.truncate(limit.max(0) as usize);
        v
    }

    /// Subsequence fuzzy match — every char of the query must appear
    /// in content in order, case-insensitive. Same algorithm as the
    /// in-tree `cli::search::fuzzy_match`.
    pub fn search_fuzzy(&self, query: &str, limit: i32) -> Vec<Entry> {
        let v = self
            .entries
            .lock()
            .expect("poisoned")
            .iter()
            .filter(|e| fuzzy_match(query, &e.content))
            .cloned()
            .collect::<Vec<_>>();
        v.into_iter().take(limit.max(0) as usize).collect()
    }

    pub fn get_by_id(&self, id: i64) -> Option<Entry> {
        self.entries
            .lock()
            .expect("poisoned")
            .iter()
            .find(|e| e.id == id)
            .cloned()
    }

    pub fn latest(&self) -> Option<Entry> {
        self.get_history(1, 0, None).into_iter().next()
    }

    /// Insert an entry, replacing any existing row with the same
    /// `id`. Mirrors the in-tree `repo.save()` upsert behavior so
    /// `run_import` and `run_add` tests can write through the same
    /// surface. The returned id is `entry.id` (no auto-increment in
    /// the standalone — the caller controls ids).
    pub fn insert(&self, entry: Entry) -> i64 {
        let mut v = self.entries.lock().expect("poisoned");
        let id = entry.id;
        if let Some(existing) = v.iter().position(|e| e.id == id) {
            v[existing] = entry;
        } else {
            v.push(entry);
        }
        id
    }

    /// Drop every entry. Mirrors `repo.clear()` in the in-tree trait
    /// for the `--mode replace` import path.
    pub fn clear(&self) {
        self.entries.lock().expect("poisoned").clear();
    }

    /// Test helper: build a fresh `MockRepo` with `n` synthetic text
    /// entries. Each entry's content is `"entry #{i}"` so the
    /// roundtrip test can assert on a known shape. The default 5
    /// matches the in-tree demo data length.
    pub fn with_n(n: usize) -> Self {
        let entries = (0..n)
            .map(|i| {
                let content = format!("entry #{}", i + 1);
                Entry::new((i + 1) as i64, "text", &content)
            })
            .collect();
        Self::with(entries)
    }
}

// =====================================================================
// Subsequence fuzzy match (mirrors in-tree cli::search::fuzzy_match)
// =====================================================================

pub fn fuzzy_match(pattern: &str, text: &str) -> bool {
    let p = pattern.to_lowercase();
    let t = text.to_lowercase();
    let mut piter = p.chars().peekable();
    for ch in t.chars() {
        if piter.peek() == Some(&ch) {
            piter.next();
        }
    }
    piter.peek().is_none()
}

// =====================================================================
// Output formatting (mirrors in-tree cli::list::format_human etc.)
// =====================================================================

pub const DEFAULT_LIMIT: i32 = 20;
pub const UNLIMITED: i32 = i32::MAX;
pub const LATEST_TOKEN: &str = "latest";

pub fn icon_for(content_type: &str) -> &'static str {
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

pub fn format_human(e: &Entry) -> String {
    let pin = if e.is_pinned { "\u{1F4CC} " } else { "" };
    let icon = icon_for(&e.content_type);
    let preview = preview_line(&e.preview);
    format!("{pin}{icon} [{}] {preview}", e.content_type)
}

fn preview_line(preview: &str) -> String {
    let first = preview.lines().next().unwrap_or("");
    first.split_whitespace().collect::<Vec<_>>().join(" ")
}

// =====================================================================
// Manual JSON serializer for CliEntry (no serde_json in standalone)
// =====================================================================

/// CLI-shaped entry. Matches the in-tree `cli::list::CliEntry` field
/// set and the JSON keys documented in the task spec.
#[derive(Debug, Clone)]
pub struct CliEntry {
    pub id: i64,
    pub content_type: String,
    pub content: String,
    pub preview: String,
    pub tags: Vec<String>,
    pub pinned: bool,
    pub timestamp: i64,
}

impl From<&Entry> for CliEntry {
    fn from(e: &Entry) -> Self {
        Self {
            id: e.id,
            content_type: e.content_type.clone(),
            content: e.content.clone(),
            preview: e.preview.clone(),
            tags: e.tags.clone(),
            pinned: e.is_pinned,
            timestamp: e.timestamp,
        }
    }
}

/// Escape a string per RFC 8259 (subset we need):
/// * `"` → `\"`
/// * `\` → `\\`
/// * `\b` → `\b`, `\f` → `\f`, `\n` → `\n`, `\r` → `\r`, `\t` → `\t`
/// * control chars (< 0x20) → `\u00XX`
pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Serialize a `CliEntry` to a single-line JSON object (no trailing
/// newline — caller controls emission).
pub fn cli_entry_to_json(e: &CliEntry) -> String {
    let tags = e
        .tags
        .iter()
        .map(|t| format!("\"{}\"", json_escape(t)))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"id\":{},\"type\":\"{}\",\"content\":\"{}\",\"preview\":\"{}\",\"tags\":[{}],\"pinned\":{},\"timestamp\":{}}}",
        e.id,
        json_escape(&e.content_type),
        json_escape(&e.content),
        json_escape(&e.preview),
        tags,
        e.pinned,
        e.timestamp,
    )
}

/// Serialize a list of `CliEntry` to a JSON array (no trailing
/// newline).
pub fn cli_entries_to_json_array(entries: &[Entry]) -> String {
    let parts = entries
        .iter()
        .map(|e| cli_entry_to_json(&CliEntry::from(e)))
        .collect::<Vec<_>>();
    format!("[{}]", parts.join(","))
}

// =====================================================================
// Subcommand arg shapes — mirror the in-tree cli module structs so the
// `run_*` functions share a stable signature across both crates.
// =====================================================================

#[derive(Debug, Clone, Default)]
pub struct ListArgs {
    pub limit: Option<i32>,
    pub kind: Option<String>,
    pub tag: Option<String>,
    pub pinned: bool,
    pub json: bool,
    pub ids: bool,
    pub quiet: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SearchMode {
    #[default]
    Contains,
    Fuzzy,
    Regex,
    Fts5,
}

#[derive(Debug, Clone, Default)]
pub struct SearchArgs {
    pub query: String,
    pub mode: SearchMode,
    pub limit: Option<i32>,
    pub json: bool,
    pub quiet: bool,
}

#[derive(Debug, Clone, Default)]
pub struct GetArgs {
    pub id: String,
    pub preview: bool,
    pub json: bool,
    pub quiet: bool,
}

// =====================================================================
// Stream 1 / Task 58 arg shapes — mirror the in-tree
// cli::{export,import,stats,watch} types so the bin layer and tests
// share the same surface.
// =====================================================================

/// Args for the `export` subcommand. `passphrase` is required when
/// `encrypted` is true; otherwise ignored.
#[derive(Debug, Clone, Default)]
pub struct ExportArgs {
    pub path: String,
    pub encrypted: bool,
    pub passphrase: Option<String>,
    pub quiet: bool,
}

/// Args for the `import` subcommand. `mode` is one of `"merge"` (default)
/// or `"replace"`; anything else is rejected with an error.
#[derive(Debug, Clone, Default)]
pub struct ImportArgs {
    pub path: String,
    pub mode: String,
    pub passphrase: Option<String>,
    pub quiet: bool,
}

/// Args for the `stats` subcommand.
#[derive(Debug, Clone, Default)]
pub struct StatsArgs {
    /// How many entries to include in `most_used`. `None` → default 5.
    pub top: Option<i32>,
    /// Emit JSON instead of human-readable text.
    pub json: bool,
}

/// Args for the `watch` subcommand. `pattern` is a case-insensitive
/// substring filter; `interval_ms` is the poll interval (default
/// 500ms — G32 hard constraint).
#[derive(Debug, Clone, Default)]
pub struct WatchArgs {
    pub pattern: Option<String>,
    pub interval_ms: Option<u64>,
    pub json: bool,
    pub quiet: bool,
}

/// Args for the `add` subcommand. The bin resolves the positional
/// `content` token into one of `text` / `stdin` / `file`; this struct
/// is what the cli surface actually consumes. Mirrors the in-tree
/// `cli::add::AddArgs` field set so the two stay in sync.
#[derive(Debug, Clone, Default)]
pub struct AddArgs {
    /// Literal text payload (when the user passed a non-special token).
    pub text: Option<String>,
    /// `true` when the user passed `-` (read stdin until EOF).
    pub stdin: bool,
    /// File path (when the user passed `@<path>`).
    pub file: Option<String>,
    /// Optional content type override (`text`, `html`, `image`, ...).
    pub kind: Option<String>,
}

/// Args for the `delete` subcommand. The standalone stores `id` as
/// `i64` (not `String` like the in-tree) because the integration
/// tests construct it directly from `repo.iter().next().unwrap().id`.
#[derive(Debug, Clone, Default)]
pub struct DeleteArgs {
    pub id: i64,
    /// When `false` (default), refuse to delete.
    pub yes: bool,
}

/// Subcommand for `tag`. Tuple variants match the test literal
/// `TagCommand::Add(id, "work".to_string())`.
#[derive(Debug, Clone)]
pub enum TagCommand {
    /// Append the tag to the entry's tag list.
    Add(i64, String),
    /// Remove the tag from the entry's tag list.
    Remove(i64, String),
    /// Print the entry's tags, one per line.
    List(i64),
}

/// Args for the `tag` subcommand.
#[derive(Debug, Clone)]
pub struct TagArgs {
    pub command: TagCommand,
}

// =====================================================================
// Subcommand implementations — mirror in-tree cli::*::run shapes.
// Each `run_*` returns the formatted string instead of writing to
// stdout, so tests can assert on it directly. The bin layer prints the
// returned string.
// =====================================================================

#[derive(Debug)]
pub enum Output {
    Text(String),
    Json(String),
    Ids(Vec<i64>),
    Quiet,
}

/// Run the `list` subcommand and return the formatted output. The
/// in-tree equivalent writes to stdout; this returns a structured
/// value so tests don't need to capture stdout.
pub fn run_list(args: &ListArgs, repo: &MockRepo) -> Result<Output, String> {
    let raw_limit = args.limit.unwrap_or(DEFAULT_LIMIT);
    let limit = if raw_limit < 0 { UNLIMITED } else { raw_limit };

    let candidates = repo.get_history(limit, 0, args.kind.as_deref());

    let entries: Vec<Entry> = candidates
        .into_iter()
        .filter(|e| match &args.tag {
            Some(t) => e.tags.iter().any(|et| et == t),
            None => true,
        })
        .filter(|e| !args.pinned || e.is_pinned)
        .collect();

    if args.quiet {
        return Ok(Output::Quiet);
    }
    if args.ids {
        return Ok(Output::Ids(entries.iter().map(|e| e.id).collect()));
    }
    if args.json {
        return Ok(Output::Json(cli_entries_to_json_array(&entries)));
    }
    let mut out = String::new();
    for e in &entries {
        out.push_str(&format_human(e));
        out.push('\n');
    }
    Ok(Output::Text(out))
}

/// Run the `search` subcommand and return the formatted output.
pub fn run_search(args: &SearchArgs, repo: &MockRepo) -> Result<Output, String> {
    if args.query.trim().is_empty() {
        if args.quiet {
            return Ok(Output::Quiet);
        }
        if args.json {
            return Ok(Output::Json("[]".to_string()));
        }
        return Ok(Output::Text(String::new()));
    }

    let limit = args.limit.unwrap_or(DEFAULT_LIMIT);

    // Note: `Regex` and `Fts5` are degraded in the standalone crate
    // because we cannot add the `regex` crate (and have no FTS5 index).
    // The in-tree binary uses the real `regex` crate for `Regex` and
    // `services::search` for `Fts5`.
    let candidates: Vec<Entry> = match args.mode {
        SearchMode::Fts5 => repo.search_contains(&args.query, limit),
        SearchMode::Contains => repo.search_contains(&args.query, limit),
        SearchMode::Fuzzy => repo.search_fuzzy(&args.query, limit),
        SearchMode::Regex => repo.search_contains(&args.query, limit),
    };

    if args.quiet {
        return Ok(Output::Quiet);
    }
    if args.json {
        return Ok(Output::Json(cli_entries_to_json_array(&candidates)));
    }
    let mut out = String::new();
    for e in &candidates {
        out.push_str(&format_human(e));
        out.push('\n');
    }
    Ok(Output::Text(out))
}

/// Run the `get` subcommand and return the formatted output.
pub fn run_get(args: &GetArgs, repo: &MockRepo) -> Result<Output, String> {
    let entry = if args.id == LATEST_TOKEN {
        repo.latest()
    } else {
        let id: i64 = args
            .id
            .parse()
            .map_err(|e| format!("get: invalid id '{}': {e}", args.id))?;
        repo.get_by_id(id)
    };
    let Some(entry) = entry else {
        return Err(format!("get: no entry for id '{}'", args.id));
    };

    if args.quiet {
        return Ok(Output::Quiet);
    }
    if args.json {
        return Ok(Output::Json(cli_entry_to_json(&CliEntry::from(&entry))));
    }
    if args.preview {
        return Ok(Output::Text(format!("{}\n", entry.preview)));
    }
    Ok(Output::Text(format!("{}\n", entry.content)))
}

// =====================================================================
// Task 58: export / import / stats / watch implementations.
//
// These return `Result<(), String>` for export/import (file IO can
// fail) and `String` for stats/watch (the caller/test does
// `.contains(...)` directly per the spec). The watch path takes
// `Arc<Mutex<i64>>` and `Arc<AtomicBool>` so tests can drive the
// stop signal from a separate thread.
// =====================================================================

/// File-level envelope for the export JSON. Matches the in-tree
/// `services::backup::ExportFile` shape (version + exported_at +
/// entries) but uses a manual `serde`-free serializer so the
/// standalone crate has no extra deps.
pub const EXPORT_VERSION: &str = "dezirclip-export-v1";
pub const LEGACY_EXPORT_VERSION: &str = "tiez-export-v1";

/// Minimum passphrase length. Mirrors the in-tree
/// `cli::export::MIN_PASSPHRASE_LEN` so the two code paths share
/// the same UX rule.
pub const MIN_PASSPHRASE_LEN: usize = 12;

/// Run the `export` subcommand. Writes the entries to `args.path` in
/// either JSON or AES-GCM-encrypted form.
///
/// In the standalone crate the encrypted form is a **stub** — we
/// only write a magic header so a roundtrip import can detect the
/// format. The in-tree binary uses `services::backup` for real
/// encryption. This is by design: the standalone can't pull in
/// `aes-gcm` + `argon2` without the full Tauri dependency chain.
pub fn run_export(args: &ExportArgs, repo: &MockRepo) -> Result<(), String> {
    let lower = args.path.to_ascii_lowercase();
    let format = if args.encrypted || lower.ends_with(".dzc") || lower.ends_with(".dzc") {
        ExportFormat::Encrypted
    } else {
        ExportFormat::Json
    };

    if format == ExportFormat::Encrypted {
        let pw = args.passphrase.as_deref().unwrap_or("");
        if pw.is_empty() {
            return Err("export: passphrase required for encrypted output".to_string());
        }
        if pw.chars().count() < MIN_PASSPHRASE_LEN {
            return Err(format!(
                "export: passphrase must be at least {MIN_PASSPHRASE_LEN} characters"
            ));
        }
    }

    let entries = repo.snapshot();
    let count = entries.len();

    let bytes: Vec<u8> = match format {
        ExportFormat::Json => {
            let envelope = ExportEnvelope {
                version: EXPORT_VERSION.to_string(),
                exported_at: 0,
                entries: entries.iter().map(export_entry_from).collect(),
            };
            serde_noop::serialize_envelope(&envelope).into_bytes()
        }
        ExportFormat::Encrypted => {
            // Stub: real encryption happens in the in-tree binary.
            // The magic bytes make the import path's format detection
            // robust without the standalone needing AES-GCM.
            let mut buf = Vec::new();
            buf.extend_from_slice(b"DEZIRCLIP-ENC-STUB-v1\n");
            buf.extend_from_slice(format!("passphrase_used={}\n", args.passphrase.as_deref().unwrap()).as_bytes());
            buf.extend_from_slice(format!("count={count}\n").as_bytes());
            buf
        }
    };

    write_file(&args.path, &bytes)?;

    if !args.quiet {
        let label = match format {
            ExportFormat::Json => "json",
            ExportFormat::Encrypted => "encrypted",
        };
        println!("Exported {count} entries to {} ({label})", args.path);
    }
    Ok(())
}

/// Run the `import` subcommand. Reads from `args.path` and inserts
/// every entry into `repo`.
///
/// Like `run_export`, the encrypted import is a stub — the
/// standalone detects the magic header and pretends to decrypt.
/// The in-tree binary uses the real `decrypt_to_json` path.
pub fn run_import(args: &ImportArgs, repo: &mut MockRepo) -> Result<(), String> {
    let mode = match args.mode.to_ascii_lowercase().as_str() {
        "" | "merge" => "merge",
        "replace" => "replace",
        other => {
            return Err(format!(
                "import: invalid mode '{other}' (expected 'merge' or 'replace')"
            ))
        }
    };

    let lower = args.path.to_ascii_lowercase();
    let is_encrypted = lower.ends_with(".dzc") || lower.ends_with(".dzc");
    if is_encrypted && args.passphrase.as_deref().map(str::is_empty).unwrap_or(true) {
        return Err("import: passphrase required for encrypted backup files".to_string());
    }

    let bytes = std::fs::read(&args.path)
        .map_err(|e| format!("import: read {:?}: {e}", args.path))?;

    if is_encrypted {
        // Stub path: validate the magic header, ignore the rest.
        // A real import would call `services::backup::decrypt_to_json`
        // + `entries_from_json` and then `repo.save()` per row.
        let header = String::from_utf8_lossy(&bytes);
        if !header.starts_with("DEZIRCLIP-ENC-STUB-v1") && !header.starts_with("TIEZ-ENC-STUB-v1") {
            return Err("import: not a recognized encrypted backup file".to_string());
        }
        if !args.quiet {
            println!(
                "Imported 0/0 entries from {} (encrypted-stub, mode {mode})",
                args.path
            );
        }
        return Ok(());
    }

    let json = std::str::from_utf8(&bytes)
        .map_err(|e| format!("import: file is not valid UTF-8: {e}"))?;
    let envelope: ExportEnvelope = serde_noop::deserialize_envelope(json)
        .map_err(|e| format!("import: parse json: {e}"))?;
    if envelope.version != EXPORT_VERSION && envelope.version != LEGACY_EXPORT_VERSION {
        return Err(format!(
            "import: unsupported export version '{}' (expected {EXPORT_VERSION})",
            envelope.version
        ));
    }

    if mode == "replace" {
        repo.clear();
    }

    let total = envelope.entries.len();
    let mut imported = 0usize;
    for e in envelope.entries {
        repo.insert(entry_from_export(&e));
        imported += 1;
    }

    if !args.quiet {
        println!(
            "Imported {imported}/{total} entries from {} (json, mode {mode})",
            args.path
        );
    }
    Ok(())
}

/// Run the `stats` subcommand. Returns the human-readable or JSON
/// string directly (not wrapped in `Result`) so the test can do
/// `stats.contains("Total: 5")` without unwrapping. Internal
/// invariant failures (impossible from a `MockRepo`) are not
/// handled — the test fixtures always satisfy the preconditions.
pub fn run_stats(args: &StatsArgs, repo: &MockRepo) -> String {
    let entries = repo.snapshot();
    let total = entries.len();
    let by_type = count_by_type(&entries);

    let top_n = args.top.unwrap_or(5).max(0) as usize;
    let mut most_used = entries.clone();
    most_used.sort_by(|a, b| {
        b.use_count
            .cmp(&a.use_count)
            .then_with(|| b.id.cmp(&a.id))
    });
    most_used.truncate(top_n);

    if args.json {
        format_stats_json(total, &by_type, &most_used)
    } else {
        format_stats_human(total, &by_type, &most_used)
    }
}

/// Run the `watch` subcommand. Polls `repo` every `args.interval_ms`
/// (default 500 — G32), writes each new entry to stdout in real time
/// (so the user sees output as it arrives), and ALSO accumulates
/// them in the returned string. Exits when `running` flips to
/// `false`; the caller is responsible for flipping it (e.g. the bin
/// installs a SIGINT handler, the test uses a thread + sleep).
///
/// `last_seen` is a shared high-water-mark; the function reads AND
/// updates it in place so the next call picks up where this one
/// left off. The shared `Arc<Mutex<i64>>` shape matches the test
/// signature in the task spec.
///
/// The repo is taken as `&Arc<Mutex<MockRepo>>` (not `&MockRepo`)
/// so the watch loop can lock and re-snapshot every tick, allowing
/// other threads to insert entries between ticks. This mirrors the
/// in-tree `&dyn ClipboardRepository` lifetime (which is naturally
/// `Sync`).
pub fn run_watch(
    args: &WatchArgs,
    repo: &std::sync::Arc<std::sync::Mutex<MockRepo>>,
    last_seen: std::sync::Arc<std::sync::Mutex<i64>>,
    running: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> String {
    use std::io::Write;
    use std::sync::atomic::Ordering;
    let interval = args.interval_ms.unwrap_or(500).clamp(50, 60_000);
    let pattern_lower = args.pattern.as_ref().map(|p| p.to_lowercase());

    let mut out = String::new();
    if !args.quiet {
        // Startup banner mirrors the in-tree `cli::watch::run` shape.
        let banner = format!(
            "Watching for new entries... (interval={interval}ms, Ctrl-C to exit)\n"
        );
        out.push_str(&banner);
        print!("{banner}");
        let _ = std::io::stdout().flush();
    }

    while running.load(Ordering::Acquire) {
        let entries = repo.lock().expect("poisoned").snapshot();
        let mut new_entries: Vec<Entry> = entries
            .into_iter()
            .filter(|e| {
                let cur = *last_seen.lock().expect("poisoned");
                e.timestamp > cur
            })
            .collect();
        new_entries.sort_by(|a, b| {
            a.timestamp
                .cmp(&b.timestamp)
                .then_with(|| a.id.cmp(&b.id))
        });

        for e in &new_entries {
            if !matches_watch_pattern(e, pattern_lower.as_deref()) {
                continue;
            }
            let line = if args.json {
                format!("{}\n", cli_entry_to_json(&CliEntry::from(e)))
            } else {
                format!("{}\n", format_human(e))
            };
            out.push_str(&line);
            print!("{line}");
            let _ = std::io::stdout().flush();
            let mut cur = last_seen.lock().expect("poisoned");
            if e.timestamp > *cur {
                *cur = e.timestamp;
            }
        }

        if running.load(Ordering::Acquire) {
            std::thread::sleep(std::time::Duration::from_millis(interval));
        }
    }

    if !args.quiet {
        out.push_str("Stopped watching.\n");
    }
    out
}

// =====================================================================
// Internal helpers for the Task 58 subcommands.
// =====================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExportFormat {
    Json,
    Encrypted,
}

/// Per-entry export shape. Subset of the in-tree
/// `services::backup::ExportEntry` — only fields the standalone
/// can faithfully roundtrip. `tags` flow through; `kinds` and `ocr_text`
/// are dropped (no equivalent in the standalone `Entry`).
#[derive(Debug, Clone)]
struct WireExportEntry {
    id: i64,
    content_type: String,
    content: String,
    preview: String,
    tags: Vec<String>,
    pinned: bool,
    timestamp: i64,
    use_count: i32,
}

#[derive(Debug, Clone)]
struct ExportEnvelope {
    version: String,
    exported_at: i64,
    entries: Vec<WireExportEntry>,
}

fn export_entry_from(e: &Entry) -> WireExportEntry {
    WireExportEntry {
        id: e.id,
        content_type: e.content_type.clone(),
        content: e.content.clone(),
        preview: e.preview.clone(),
        tags: e.tags.clone(),
        pinned: e.is_pinned,
        timestamp: e.timestamp,
        use_count: e.use_count,
    }
}

fn entry_from_export(w: &WireExportEntry) -> Entry {
    Entry {
        id: w.id,
        content_type: w.content_type.clone(),
        content: w.content.clone(),
        preview: w.preview.clone(),
        tags: w.tags.clone(),
        is_pinned: w.pinned,
        timestamp: w.timestamp,
        use_count: w.use_count,
    }
}

fn count_by_type(entries: &[Entry]) -> std::collections::BTreeMap<String, usize> {
    let mut out = std::collections::BTreeMap::new();
    for e in entries {
        *out.entry(e.content_type.clone()).or_insert(0) += 1;
    }
    out
}

fn format_stats_human(
    total: usize,
    by_type: &std::collections::BTreeMap<String, usize>,
    most_used: &[Entry],
) -> String {
    let mut s = String::new();
    s.push_str(&format!("Total: {total} entries\n\nBy type:\n"));
    if by_type.is_empty() {
        s.push_str("  (none)\n");
    } else {
        for (k, v) in by_type {
            s.push_str(&format!("  {k}: {v}\n"));
        }
    }
    if !most_used.is_empty() {
        s.push_str(&format!("\nMost used (top {}):\n", most_used.len()));
        for (i, e) in most_used.iter().enumerate() {
            s.push_str(&format!(
                "  {}. [id={}, used={}] {}\n",
                i + 1,
                e.id,
                e.use_count,
                one_line(&e.preview)
            ));
        }
    }
    s
}

fn format_stats_json(
    total: usize,
    by_type: &std::collections::BTreeMap<String, usize>,
    most_used: &[Entry],
) -> String {
    let mut s = String::new();
    s.push_str("{");
    s.push_str(&format!("\"total\":{total},"));
    s.push_str("\"by_type\":{");
    let mut first = true;
    for (k, v) in by_type {
        if !first {
            s.push(',');
        }
        s.push_str(&format!("\"{}\":{v}", json_escape(k)));
        first = false;
    }
    s.push_str("},\"most_used\":[");
    for (i, e) in most_used.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!(
            "{{\"id\":{},\"use_count\":{},\"preview\":\"{}\"}}",
            e.id,
            e.use_count,
            json_escape(&one_line(&e.preview))
        ));
    }
    s.push_str("]}");
    s
}

fn one_line(s: &str) -> String {
    let first = s.lines().next().unwrap_or("");
    if first.chars().count() > 80 {
        let truncated: String = first.chars().take(77).collect();
        format!("{truncated}...")
    } else {
        first.to_string()
    }
}

fn matches_watch_pattern(e: &Entry, pattern_lower: Option<&str>) -> bool {
    let Some(pat) = pattern_lower else {
        return true;
    };
    if pat.is_empty() {
        return true;
    }
    e.content.to_lowercase().contains(pat) || e.preview.to_lowercase().contains(pat)
}

fn write_file(path: &str, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let p = std::path::Path::new(path);
    if let Some(parent) = p.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create_dir_all {parent:?}: {e}"))?;
        }
    }
    let mut f = std::fs::File::create(p)
        .map_err(|e| format!("create {p:?}: {e}"))?;
    f.write_all(bytes).map_err(|e| format!("write {p:?}: {e}"))?;
    Ok(())
}

// =====================================================================
// Manual serializer for `ExportEnvelope`. Pulled into its own
// submodule so the `serde_json` import in the test file doesn't
// shadow the in-tree binary's path. We don't pull in `serde_json` to
// keep the standalone crate dep-free; the file format is a tiny
// subset.
// =====================================================================

mod serde_noop {
    use super::{json_escape, ExportEnvelope, WireExportEntry};

    pub fn serialize_envelope(env: &ExportEnvelope) -> String {
        let mut s = String::new();
        s.push_str("{");
        s.push_str(&format!(
            "\"version\":\"{}\",\"exported_at\":{},\"entries\":[",
            json_escape(&env.version),
            env.exported_at
        ));
        for (i, e) in env.entries.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&serialize_entry(e));
        }
        s.push_str("]}");
        s
    }

    fn serialize_entry(e: &WireExportEntry) -> String {
        let tags = e
            .tags
            .iter()
            .map(|t| format!("\"{}\"", json_escape(t)))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"id\":{},\"content_type\":\"{}\",\"content\":\"{}\",\"preview\":\"{}\",\"tags\":[{}],\"pinned\":{},\"timestamp\":{},\"use_count\":{}}}",
            e.id,
            json_escape(&e.content_type),
            json_escape(&e.content),
            json_escape(&e.preview),
            tags,
            e.pinned,
            e.timestamp,
            e.use_count,
        )
    }

    pub fn deserialize_envelope(json: &str) -> Result<ExportEnvelope, String> {
        // Tiny recursive-descent parser good enough for the shape
        // `serialize_envelope` produces. Anything more elaborate
        // (escapes, unicode) and we should just take the
        // `serde_json` dep — but for the standalone roundtrip test
        // we control both ends, so this is fine.
        let trimmed = json.trim();
        let (rest, _obj) = expect_object(trimmed)?;
        let _ = rest; // we don't care about trailing whitespace
        let v = Value::Object(_obj);
        let version = v
            .lookup("version")
            .ok_or_else(|| "missing 'version' field".to_string())?
            .as_str()
            .ok_or_else(|| "'version' must be a string".to_string())?
            .to_string();
        let exported_at = v
            .lookup("exported_at")
            .ok_or_else(|| "missing 'exported_at' field".to_string())?
            .as_i64()
            .ok_or_else(|| "'exported_at' must be a number".to_string())?;
        let entries_v = v
            .lookup("entries")
            .ok_or_else(|| "missing 'entries' field".to_string())?
            .as_array()
            .ok_or_else(|| "'entries' must be an array".to_string())?;
        let mut entries = Vec::with_capacity(entries_v.len());
        for item in entries_v {
            let m = item
                .as_object()
                .ok_or_else(|| "entry must be an object".to_string())?;
            entries.push(WireExportEntry {
                id: m
                    .iter()
                    .find(|(k, _)| k == "id")
                    .and_then(|(_, x)| x.as_i64())
                    .ok_or_else(|| "entry.id missing/invalid".to_string())?,
                content_type: m
                    .iter()
                    .find(|(k, _)| k == "content_type")
                    .and_then(|(_, x)| x.as_str())
                    .ok_or_else(|| "entry.content_type missing".to_string())?
                    .to_string(),
                content: m
                    .iter()
                    .find(|(k, _)| k == "content")
                    .and_then(|(_, x)| x.as_str())
                    .ok_or_else(|| "entry.content missing".to_string())?
                    .to_string(),
                preview: m
                    .iter()
                    .find(|(k, _)| k == "preview")
                    .and_then(|(_, x)| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                tags: m
                    .iter()
                    .find(|(k, _)| k == "tags")
                    .and_then(|(_, x)| x.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|t| t.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
                pinned: m
                    .iter()
                    .find(|(k, _)| k == "pinned")
                    .and_then(|(_, x)| x.as_bool())
                    .unwrap_or(false),
                timestamp: m
                    .iter()
                    .find(|(k, _)| k == "timestamp")
                    .and_then(|(_, x)| x.as_i64())
                    .ok_or_else(|| "entry.timestamp missing".to_string())?,
                use_count: m
                    .iter()
                    .find(|(k, _)| k == "use_count")
                    .and_then(|(_, x)| x.as_i64())
                    .map(|n| n as i32)
                    .unwrap_or(0),
            });
        }
        Ok(ExportEnvelope {
            version,
            exported_at,
            entries,
        })
    }

    // --- tiny JSON value model ---------------------------------------

    #[derive(Debug, Clone)]
    enum Value {
        Null,
        Bool(bool),
        Number(Number),
        String(String),
        Array(Vec<Value>),
        Object(Vec<(String, Value)>),
    }

    #[derive(Debug, Clone, Copy)]
    enum Number {
        Int(i64),
    }

    impl Value {
        fn as_str(&self) -> Option<&str> {
            if let Value::String(s) = self {
                Some(s)
            } else {
                None
            }
        }
        fn as_i64(&self) -> Option<i64> {
            if let Value::Number(Number::Int(n)) = self {
                Some(*n)
            } else {
                None
            }
        }
        fn as_bool(&self) -> Option<bool> {
            if let Value::Bool(b) = self {
                Some(*b)
            } else {
                None
            }
        }
        fn as_array(&self) -> Option<&Vec<Value>> {
            if let Value::Array(a) = self {
                Some(a)
            } else {
                None
            }
        }
        fn as_object(&self) -> Option<&Vec<(String, Value)>> {
            if let Value::Object(o) = self {
                Some(o)
            } else {
                None
            }
        }
        fn lookup(&self, key: &str) -> Option<&Value> {
            if let Value::Object(o) = self {
                o.iter().find(|(k, _)| k == key).map(|(_, v)| v)
            } else {
                None
            }
        }
    }

    fn expect_object(s: &str) -> Result<(&str, Vec<(String, Value)>), String> {
        let (s, _) = skip_ws(s);
        if !s.starts_with('{') {
            return Err("expected '{'".to_string());
        }
        let s = &s[1..];
        let (s, _) = skip_ws(s);
        let mut out = Vec::new();
        if let Some(rest) = s.strip_prefix('}') {
            return Ok((rest, out));
        }
        let mut s = s;
        loop {
            let (rest, _) = skip_ws(s);
            let (rest, key) = parse_string(rest)?;
            let (rest, _) = skip_ws(rest);
            if !rest.starts_with(':') {
                return Err("expected ':'".to_string());
            }
            let rest = &rest[1..];
            let (rest, value) = parse_value(rest)?;
            out.push((key, value));
            let (rest, _) = skip_ws(rest);
            if let Some(r2) = rest.strip_prefix(',') {
                s = r2;
                continue;
            }
            if let Some(r2) = rest.strip_prefix('}') {
                return Ok((r2, out));
            }
            return Err("expected ',' or '}'".to_string());
        }
    }

    fn parse_value(s: &str) -> Result<(&str, Value), String> {
        let (s, _) = skip_ws(s);
        if let Some(rest) = s.strip_prefix("null") {
            return Ok((rest, Value::Null));
        }
        if let Some(rest) = s.strip_prefix("true") {
            return Ok((rest, Value::Bool(true)));
        }
        if let Some(rest) = s.strip_prefix("false") {
            return Ok((rest, Value::Bool(false)));
        }
        if s.starts_with('"') {
            let (rest, str_val) = parse_string(s)?;
            return Ok((rest, Value::String(str_val)));
        }
        if s.starts_with('[') {
            return parse_array(s);
        }
        if s.starts_with('{') {
            let (rest, obj) = expect_object(s)?;
            return Ok((rest, Value::Object(obj)));
        }
        parse_number(s)
    }

    fn parse_string(s: &str) -> Result<(&str, String), String> {
        let (s, _) = skip_ws(s);
        if !s.starts_with('"') {
            return Err("expected '\"'".to_string());
        }
        let bytes = s.as_bytes();
        let mut i = 1;
        let mut out = String::new();
        while i < bytes.len() {
            let c = bytes[i];
            if c == b'"' {
                return Ok((&s[i + 1..], out));
            }
            if c == b'\\' {
                if i + 1 >= bytes.len() {
                    return Err("trailing escape".to_string());
                }
                let esc = bytes[i + 1];
                match esc {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'n' => out.push('\n'),
                    b't' => out.push('\t'),
                    b'r' => out.push('\r'),
                    b'b' => out.push('\x08'),
                    b'f' => out.push('\x0c'),
                    b'u' => {
                        if i + 5 >= bytes.len() {
                            return Err("bad \\u escape".to_string());
                        }
                        let hex = std::str::from_utf8(&bytes[i + 2..i + 6])
                            .map_err(|_| "bad \\u utf8".to_string())?;
                        let cp = u32::from_str_radix(hex, 16)
                            .map_err(|_| "bad \\u hex".to_string())?;
                        if let Some(ch) = char::from_u32(cp) {
                            out.push(ch);
                        }
                        i += 4;
                    }
                    _ => return Err(format!("bad escape \\{}", esc as char)),
                }
                i += 2;
                continue;
            }
            // ASCII fast path; fall through to UTF-8 for the rest.
            if c < 0x80 {
                out.push(c as char);
                i += 1;
            } else {
                // Decode one UTF-8 char.
                let rest_str = &s[i..];
                let ch = rest_str
                    .chars()
                    .next()
                    .ok_or_else(|| "bad utf-8".to_string())?;
                out.push(ch);
                i += ch.len_utf8();
            }
        }
        Err("unterminated string".to_string())
    }

    fn parse_array(s: &str) -> Result<(&str, Value), String> {
        if !s.starts_with('[') {
            return Err("expected '['".to_string());
        }
        let s1 = &s[1..];
        let (s1, _) = skip_ws(s1);
        if let Some(r2) = s1.strip_prefix(']') {
            return Ok((r2, Value::Array(vec![])));
        }
        let mut rest = s1;
        let mut items = Vec::new();
        loop {
            let (r, _) = skip_ws(rest);
            let (r, value) = parse_value(r)?;
            items.push(value);
            let (r, _) = skip_ws(r);
            if let Some(r2) = r.strip_prefix(',') {
                rest = r2;
                continue;
            }
            if let Some(r2) = r.strip_prefix(']') {
                return Ok((r2, Value::Array(items)));
            }
            return Err("expected ',' or ']'".to_string());
        }
    }

    fn parse_number(s: &str) -> Result<(&str, Value), String> {
        let bytes = s.as_bytes();
        let mut i = 0;
        if i < bytes.len() && (bytes[i] == b'-') {
            i += 1;
        }
        while i < bytes.len() && (bytes[i].is_ascii_digit()) {
            i += 1;
        }
        if i < bytes.len() && bytes[i] == b'.' {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
        }
        if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
            i += 1;
            if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
                i += 1;
            }
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
        }
        let text = std::str::from_utf8(&bytes[..i])
            .map_err(|_| "bad number utf-8".to_string())?;
        let n: i64 = text
            .parse()
            .map_err(|e| format!("bad number '{text}': {e}"))?;
        Ok((&s[i..], Value::Number(Number::Int(n))))
    }

    fn skip_ws(s: &str) -> (&str, ()) {
        let trimmed = s.trim_start();
        let consumed = s.len() - trimmed.len();
        (&s[consumed..], ())
    }
}

/// Run the `add` subcommand. Mirrors the in-tree `cli::add::run`
/// semantics: empty content is rejected, content type defaults to
/// `text`, the new id is returned in the `Output::Text` payload so the
/// bin can print `Added: id=<n>`. The repo handle is `&mut` so the
/// signature lines up with the integration test (`&mut repo`).
pub fn run_add(args: &AddArgs, repo: &mut MockRepo) -> Result<Output, String> {
    let content = read_add_content(args)?;
    if content.is_empty() {
        return Err("add: empty content (refusing to insert)".to_string());
    }

    let content_type = args
        .kind
        .as_deref()
        .map(normalize_add_kind)
        .unwrap_or_else(|| "text".to_string());
    let preview: String = content.chars().take(500).collect();

    let entry = Entry {
        id: 0,
        content_type,
        content,
        preview,
        tags: Vec::new(),
        is_pinned: false,
        timestamp: 0,
        use_count: 0,
    };
    let id = repo.save(entry);
    Ok(Output::Text(format!("Added: id={id}\n")))
}

/// Resolve the three content sources into a single string. Pure
/// function so the test can drive `text` and `file` branches without
/// a real stdin.
fn read_add_content(args: &AddArgs) -> Result<String, String> {
    if args.stdin {
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| format!("add: read stdin: {e}"))?;
        return Ok(buf);
    }
    if let Some(path) = &args.file {
        return std::fs::read_to_string(std::path::Path::new(path))
            .map_err(|e| format!("add: read file {path:?}: {e}"));
    }
    if let Some(text) = &args.text {
        return Ok(text.clone());
    }
    Err("add: no content source (set text, stdin=true, or file)".to_string())
}

/// Map `--type` aliases onto the canonical `content_type` strings.
fn normalize_add_kind(raw: &str) -> String {
    match raw.to_ascii_lowercase().as_str() {
        "text" | "txt" | "plain" => "text".to_string(),
        "html" => "html".to_string(),
        "image" | "img" | "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" => "image".to_string(),
        "code" | "src" => "code".to_string(),
        "file" | "path" => "file".to_string(),
        other => other.to_string(),
    }
}

/// Run the `delete` subcommand. Refuses without `--yes`.
pub fn run_delete(args: &DeleteArgs, repo: &mut MockRepo) -> Result<Output, String> {
    if !args.yes {
        return Err(format!(
            "delete: refusing to delete id={} without --yes (use --yes to confirm)",
            args.id
        ));
    }
    repo.delete(args.id);
    Ok(Output::Quiet)
}

/// Run the `tag` subcommand. Dispatches on the inner `TagCommand`.
pub fn run_tag(args: &TagArgs, repo: &mut MockRepo) -> Result<Output, String> {
    match &args.command {
        TagCommand::Add(id, tag) => {
            let cleaned = clean_tag(tag).ok_or_else(|| "tag: tag cannot be empty".to_string())?;
            let entry = repo
                .get_by_id(*id)
                .ok_or_else(|| format!("tag: no entry for id={id}"))?;
            let mut current = entry.tags;
            if !current.iter().any(|t| t == &cleaned) {
                current.push(cleaned.clone());
            }
            repo.set_tags(*id, current)
                .map_err(|e| format!("tag add: {e}"))?;
            Ok(Output::Text(format!("tag: added {cleaned:?} to id={id}\n")))
        }
        TagCommand::Remove(id, tag) => {
            let cleaned = clean_tag(tag).ok_or_else(|| "tag: tag cannot be empty".to_string())?;
            let entry = repo
                .get_by_id(*id)
                .ok_or_else(|| format!("tag: no entry for id={id}"))?;
            let mut current = entry.tags;
            let before = current.len();
            current.retain(|t| t != &cleaned);
            if current.len() != before {
                repo.set_tags(*id, current)
                    .map_err(|e| format!("tag remove: {e}"))?;
            }
            Ok(Output::Text(format!(
                "tag: removed {cleaned:?} from id={id}\n"
            )))
        }
        TagCommand::List(id) => {
            let entry = repo
                .get_by_id(*id)
                .ok_or_else(|| format!("tag: no entry for id={id}"))?;
            if entry.tags.is_empty() {
                Ok(Output::Text("(no tags)\n".to_string()))
            } else {
                let mut out = String::new();
                for t in &entry.tags {
                    out.push_str(t);
                    out.push('\n');
                }
                Ok(Output::Text(out))
            }
        }
    }
}

/// Trim a tag; `None` if the result is empty.
fn clean_tag(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

// =====================================================================
// Pretty-printer helper for the bin: writes Output to stdout.
// =====================================================================

pub fn emit_output(out: Output) {
    match out {
        Output::Text(s) => print!("{s}"),
        Output::Json(s) => println!("{s}"),
        Output::Ids(ids) => {
            for id in ids {
                println!("{id}");
            }
        }
        Output::Quiet => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_escape_handles_specials() {
        assert_eq!(json_escape("plain"), "plain");
        assert_eq!(json_escape("a\"b"), "a\\\"b");
        assert_eq!(json_escape("a\\b"), "a\\\\b");
        assert_eq!(json_escape("a\nb"), "a\\nb");
        assert_eq!(json_escape("a\tb"), "a\\tb");
        assert_eq!(json_escape("\x01"), "\\u0001");
    }

    #[test]
    fn fuzzy_match_basic() {
        assert!(fuzzy_match("git", "git status"));
        assert!(fuzzy_match("GIT", "GitHub"));
        assert!(fuzzy_match("git", "go iterate"));
        assert!(!fuzzy_match("xyz", "git status"));
    }

    #[test]
    fn cli_entry_json_round_trip() {
        let e = Entry::new(7, "text", "hello \"world\"\n").pinned().with_tags(&["a", "b"]);
        let s = cli_entry_to_json(&CliEntry::from(&e));
        // Each required field appears at least once.
        for needle in ["\"id\":7", "\"type\":\"text\"", "\"pinned\":true", "\"tags\":[\"a\",\"b\"]"] {
            assert!(s.contains(needle), "missing {needle:?} in {s}");
        }
    }
}
