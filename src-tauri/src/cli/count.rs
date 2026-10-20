//! `dzc count` — the number of stored entries.
//!
//! The cheapest possible read for shell conditions and scripts: one
//! `SELECT COUNT(*)`, no row materialisation, no preview projection.
//!
//! Output: just the integer, so `if [ "$(dzc count)" -gt 500 ]; then ...` works
//! without parsing. `--json` emits `{"count": <n>}` instead.

use crate::api::HistoryService;

/// Run the `count` subcommand. Returns the entry total; rendering is the
/// binary's job, since only it knows the `--json` flag.
pub fn run(repo: &HistoryService) -> Result<i64, String> {
    repo.get_count()
        .map_err(|e| format!("count: repo failed: {e}"))
}

/// Render the count in the shape the caller's flags asked for.
pub fn render(count: i64, json: bool) -> String {
    if json {
        format!("{{\"count\":{count}}}")
    } else {
        count.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_integer_is_parseable_by_shell_arithmetic() {
        assert_eq!(render(42, false), "42");
        assert_eq!(render(0, false), "0");
    }

    #[test]
    fn json_shape_is_a_single_flat_object() {
        assert_eq!(render(7, true), "{\"count\":7}");
        assert_eq!(render(0, true), "{\"count\":0}");
    }
}
