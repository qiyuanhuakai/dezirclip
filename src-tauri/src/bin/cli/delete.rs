//! `tiez-c delete` — remove an entry by id.
//!
//! Safety: requires `--yes` to confirm. Without it, `run` returns
//! `Err(...)` rather than prompting. The CLI is designed for
//! non-interactive use (shell scripts, CI, hotkey hooks), so a missing
//! confirmation flag is a hard error, not a blocking stdin prompt.
//!
//! A future task may add a real `Confirm::new().interact()` prompt for
//! human-driven flows, gated by a separate `--interactive` flag; for
//! now the safety is just "no `--yes` → refuse".
//!
//! Output: silent on success. Errors propagate with the operation name
//! prefix (`delete: ...`) so they show up in the user's terminal.

use crate::infrastructure::repository::clipboard_repo::ClipboardRepository;

/// Args for the `delete` subcommand.
pub struct DeleteArgs {
    /// Entry id (parsed as `i64`; non-numeric is a hard error).
    pub id: String,
    /// When `false` (default), refuse to delete.
    pub yes: bool,
}

/// Validate the args and parse the id. Pure function, no repo access,
/// so it can be unit-tested without a `ClipboardRepository` impl.
fn parse_id(args: &DeleteArgs) -> Result<i64, String> {
    if !args.yes {
        return Err(format!(
            "delete: refusing to delete id={} without --yes (use --yes to confirm)",
            args.id
        ));
    }
    args.id
        .parse()
        .map_err(|e| format!("delete: invalid id '{}': {e}", args.id))
}

/// Run the `delete` subcommand. Returns `Ok(())` on successful delete.
pub fn run(args: &DeleteArgs, repo: &dyn ClipboardRepository) -> Result<(), String> {
    let id = parse_id(args)?;
    repo.delete(id, None)
        .map_err(|e| format!("delete: repo failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_id_refuses_without_yes() {
        let args = DeleteArgs {
            id: "1".into(),
            yes: false,
        };
        let err = parse_id(&args).expect_err("must refuse");
        assert!(err.contains("--yes"), "error must mention --yes: {err}");
        assert!(err.contains("1"), "error must echo the id: {err}");
    }

    #[test]
    fn parse_id_accepts_with_yes() {
        let args = DeleteArgs {
            id: "42".into(),
            yes: true,
        };
        let id = parse_id(&args).expect("ok");
        assert_eq!(id, 42);
    }

    #[test]
    fn parse_id_rejects_non_numeric() {
        let args = DeleteArgs {
            id: "abc".into(),
            yes: true,
        };
        let err = parse_id(&args).expect_err("non-numeric must error");
        assert!(
            err.contains("invalid id"),
            "error must mention invalid id: {err}"
        );
    }
}
