pub mod database;
pub mod domain;

// `infrastructure::encryption` reports why it could not reach the master key
// through `crate::warn!`. The macro is `#[macro_export]`ed from the logger, and
// without the module here the standalone `dzc` build cannot resolve it -- which
// only shows up when the Linux keyring path is compiled, since that code is
// `#[cfg(not(windows))]`. The CLI never opens a log file, so the logger falls
// back to printing to the console there, which is what a CLI should do anyway.
pub mod logger;

#[path = "cli_infrastructure.rs"]
pub mod infrastructure;

#[path = "cli_services.rs"]
pub mod services;

pub mod cli;
