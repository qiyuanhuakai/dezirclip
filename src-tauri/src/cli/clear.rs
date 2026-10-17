//! `dzc clear --yes` — drop the unprotected part of the history.
//!
//! This mirrors the app's own clear rather than truncating the table: the
//! repository removes only entries that are neither pinned nor tagged, so
//! anything the user protected on purpose survives. `--yes` is required for
//! the same reason `delete` requires it — this CLI is built for
//! non-interactive use, so a missing confirmation flag is a hard error rather
//! than a blocking prompt.
//!
//! `data_dir` is passed as `None`, matching `delete`: pinned files and the
//! blobs belonging to protected entries stay on disk, and the app keeps
//! owning them.
//!
//! Output: `Cleared <n> entries` unless `--quiet`, where n is the number
//! actually removed — the total before minus the total after.

use crate::infrastructure::repository::clipboard_repo::ClipboardRepository;

pub struct ClearArgs {
    /// When `false` (default), refuse to clear.
    pub yes: bool,
}

/// Validate the confirmation flag. Pure, so it is unit-tested without a
/// repository.
fn confirm(args: &ClearArgs) -> Result<(), String> {
    if args.yes {
        Ok(())
    } else {
        Err(
            "clear: refusing to clear history without --yes (use --yes to confirm)".to_string(),
        )
    }
}

/// Run the `clear` subcommand. Returns how many entries were removed.
pub fn run(args: &ClearArgs, repo: &dyn ClipboardRepository) -> Result<i64, String> {
    confirm(args)?;

    let before = repo
        .get_count()
        .map_err(|e| format!("clear: repo failed: {e}"))?;
    repo.clear(None).map_err(|e| format!("clear: repo failed: {e}"))?;
    let after = repo
        .get_count()
        .map_err(|e| format!("clear: repo failed: {e}"))?;

    Ok((before - after).max(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirm_accepts_yes() {
        assert!(confirm(&ClearArgs { yes: true }).is_ok());
    }

    #[test]
    fn confirm_refuses_without_yes() {
        let err = confirm(&ClearArgs { yes: false }).expect_err("must refuse");
        assert!(err.contains("--yes"), "error must mention --yes: {err}");
        assert!(err.starts_with("clear:"), "error must be prefixed: {err}");
    }

    #[test]
    fn removed_count_never_goes_negative() {
        // `clear` only deletes, but a repo that reported a larger count
        // afterwards must not produce a negative "Cleared -3 entries".
        let (before, after) = (10i64, 12i64);
        assert_eq!((before - after).max(0), 0);
    }
}
