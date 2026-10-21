//! `dzc` subcommand implementations.
//!
//! History commands and terminal frontends use the shared repositories.
//! Filtering and pagination run in the repository before content is decoded.
//! Shell initialization needs no database; MCP opens it only for valid queries.
//!
//! Output modes (set by flag):
//! * human (default): one entry per line, type-icon + preview
//! * `--json`:        valid JSON array of `CliEntry` objects
//! * `--ids`:         one numeric ID per line, no other text
//! * `--quiet`:       no stdout on success (still returns Ok/Err)

pub mod add;
mod access;
mod host_access;
pub mod clear;
pub mod count;
pub mod delete;
pub mod edit;
pub mod export;
pub mod frontend;
pub mod get;
pub mod import;
pub mod list;
pub mod mcp;
pub mod payload;
pub mod pick;
pub mod pin;
pub mod search;
pub mod shell;
pub mod service;
pub mod stats;
pub mod tag;
pub mod tui;
pub mod watch;
