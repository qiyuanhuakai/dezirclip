//! `dzc pin` / `dzc unpin` — set an entry's pinned flag.
//!
//! The repository method is named `toggle_pin` but takes an explicit
//! `is_pinned`, so it *sets* the flag rather than flipping it. Pinning also
//! stamps `pinned_order = max + 1` so the entry lands on top of the pinned
//! block, which means re-pinning an already-pinned entry moves it back up
//! rather than being a no-op.
//!
//! The SQL behind it is `UPDATE ... WHERE id = ?`, which touches zero rows for
//! an id that does not exist and still reports success. In a scripted context a
//! typo would look like it worked and the entry would silently stay unpinned,
//! so both subcommands resolve the id first and refuse to report success for a
//! row that is not there.
//!
//! Output: `pin: id=<n>` / `unpin: id=<n>` unless `--quiet`.

use crate::infrastructure::repository::clipboard_repo::ClipboardRepository;

/// Args shared by `pin` and `unpin`; `pinned` is what selects between them.
pub struct PinArgs {
    pub id: String,
    /// `true` for `pin`, `false` for `unpin`.
    pub pinned: bool,
}

/// The verb used in error messages and output lines.
pub const fn op_name(pinned: bool) -> &'static str {
    if pinned { "pin" } else { "unpin" }
}

/// Parse the positional id. Pure, so the error wording is unit-tested without
/// a repository.
fn parse_id(raw: &str, pinned: bool) -> Result<i64, String> {
    raw.parse()
        .map_err(|e| format!("{}: invalid id '{raw}': {e}", op_name(pinned)))
}

/// Resolve the id and confirm the row exists.
fn resolve_id(args: &PinArgs, repo: &dyn ClipboardRepository) -> Result<i64, String> {
    let op = op_name(args.pinned);
    let id = parse_id(&args.id, args.pinned)?;
    let exists = repo
        .get_entry_by_id(id)
        .map_err(|e| format!("{op}: repo failed: {e}"))?
        .is_some();
    if !exists {
        return Err(format!("{op}: no entry with id={id}"));
    }
    Ok(id)
}

/// Run the `pin` / `unpin` subcommand. Returns the id that was written.
pub fn run(args: &PinArgs, repo: &dyn ClipboardRepository) -> Result<i64, String> {
    let id = resolve_id(args, repo)?;
    let op = op_name(args.pinned);
    repo.toggle_pin(id, args.pinned)
        .map_err(|e| format!("{op}: repo failed: {e}"))?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn op_name_follows_the_flag() {
        assert_eq!(op_name(true), "pin");
        assert_eq!(op_name(false), "unpin");
    }

    #[test]
    fn parse_id_accepts_a_number() {
        assert_eq!(parse_id("42", true).unwrap(), 42);
        assert_eq!(parse_id("42", false).unwrap(), 42);
    }

    #[test]
    fn parse_id_rejects_non_numeric() {
        let err = parse_id("abc", true).expect_err("non-numeric must error");
        assert!(err.contains("invalid id"), "error must name the problem: {err}");
        // The verb has to be the one the user actually typed.
        assert!(err.starts_with("pin:"), "error must be prefixed by the verb: {err}");
        assert!(err.contains("abc"), "error must echo the input: {err}");
    }

    #[test]
    fn parse_id_names_unpin_for_the_unpin_verb() {
        let err = parse_id("abc", false).expect_err("non-numeric must error");
        assert!(err.starts_with("unpin:"), "error must use the right verb: {err}");
    }

    #[test]
    fn parse_id_rejects_an_empty_id() {
        assert!(parse_id("", true).is_err());
    }
}
