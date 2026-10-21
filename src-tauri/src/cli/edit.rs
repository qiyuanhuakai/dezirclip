//! `dzc edit <ID> <CONTENT>` — replace an entry's content.
//!
//! Content sources are `add`'s, resolved by the bin into one of plain text,
//! `-` (stdin until EOF) or `@<path>`, and then read through `add`'s own
//! resolver and preview helper. Sharing both is deliberate: `add` and `edit`
//! must not drift apart on what `-` and `@` mean, or on how long a stored
//! preview is.
//!
//! The repository keeps everything the caller did not ask to change — content
//! type, tags, pin state — and recomputes the text hash used by
//! deduplication, so an edit to identical text is a no-op. Entries carrying a
//! sensitive tag are re-encrypted, which makes this safe to run against an
//! encrypted history.
//!
//! The id is resolved before writing so a typo reports "no entry with id=..."
//! rather than surfacing the repository's raw "Query returned no rows".
//!
//! Output: `Edited: id=<n>` unless `--quiet`.

use crate::cli::add::{preview_of, read_content, AddArgs};
use crate::api::HistoryService;

pub struct EditArgs {
    pub id: String,
    /// The content source, already resolved from the positional token by the
    /// bin using the same rules as `add`.
    pub source: AddArgs,
}

/// Parse the positional id. Pure, so the error wording is unit-tested without
/// a repository.
fn parse_id(raw: &str) -> Result<i64, String> {
    raw.parse()
        .map_err(|e| format!("edit: invalid id '{raw}': {e}"))
}

/// Run the `edit` subcommand. Returns the id that was rewritten.
pub fn run(args: &EditArgs, repo: &HistoryService) -> Result<i64, String> {
    let id = parse_id(&args.id)?;
    let ticket = repo.privacy().capture_ticket(super::access::now()).map_err(|error| error.to_string())?;
    let exists = super::access::query(repo, &crate::api::HistoryQuery {limit:1,id:Some(id),..Default::default()})?.pop().is_some();
    if !exists {
        return Err(format!("edit: no entry with id={id}"));
    }

    let content = read_content(&args.source)?;
    if content.is_empty() {
        return Err("edit: empty content (refusing to write)".to_string());
    }

    repo.update_content_authorized(id, &content, &preview_of(&content), &ticket, super::access::now())
        .map_err(|e| format!("edit: repo failed: {e}"))?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_id_accepts_a_number() {
        assert_eq!(parse_id("7").unwrap(), 7);
    }

    #[test]
    fn parse_id_rejects_non_numeric() {
        let err = parse_id("nope").expect_err("non-numeric must error");
        assert!(err.contains("invalid id"), "error must name the problem: {err}");
        assert!(err.contains("nope"), "error must echo the input: {err}");
    }

    #[test]
    fn an_edit_writes_the_same_preview_length_as_add() {
        // `add` truncates the stored preview at PREVIEW_CHARS; an edit that used
        // a different length would leave the two commands writing inconsistent
        // previews for identical content.
        let long: String = "y".repeat(crate::cli::add::PREVIEW_CHARS * 3);
        assert_eq!(preview_of(&long).chars().count(), crate::cli::add::PREVIEW_CHARS);
    }
}
