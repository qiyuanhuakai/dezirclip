//! `dzc` subcommand implementations.
//!
//! Each submodule owns a single subcommand's `run` function that takes
//! a typed args struct (defined in the binary) and a `&dyn
//! ClipboardRepository` for DB access. No SQL lives in the cli layer —
//! all persistence goes through the repository trait; the only
//! post-fetch work done here is in-memory filtering for `--tag` and
//! `--pinned` (the repository's `get_history` does not yet accept those
//! filters) plus case-insensitive substring matching for the default
//! search mode.
//!
//! Output modes (set by flag):
//! * human (default): one entry per line, type-icon + preview
//! * `--json`:        valid JSON array of `CliEntry` objects
//! * `--ids`:         one numeric ID per line, no other text
//! * `--quiet`:       no stdout on success (still returns Ok/Err)

pub mod add;
pub mod delete;
pub mod export;
pub mod get;
pub mod import;
pub mod list;
pub mod search;
pub mod stats;
pub mod tag;
pub mod watch;
