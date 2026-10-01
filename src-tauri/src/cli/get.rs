//! `dzc get` — fetch a single entry by ID or `latest`.
//!
//! Two ID forms:
//! * numeric (e.g. `42`): looked up via `repo.get_entry_by_id`.
//! * `latest` (special token): resolved via `repo.get_history(1, 0, None)`
//!   and taking the first row, which is ordered `is_pinned DESC,
//!   pinned_order DESC, timestamp DESC, id DESC` — i.e. newest.
//!
//! Output:
//! * default: print `content` followed by a newline (pipe-friendly).
//! * `--preview`: print `preview` instead.
//! * `--json`: print the full `CliEntry` JSON object.
//! * `--quiet`: silent on success.
//!
//! Errors:
//! * non-numeric, non-`latest` ID → `Err("get: invalid id '...': ...")`

use crate::domain::models::ClipboardEntry;
use crate::infrastructure::repository::clipboard_repo::ClipboardRepository;

use super::list::CliEntry;

/// Sentinel ID token that resolves to the most-recent entry.
pub const LATEST_TOKEN: &str = "latest";

/// Args for the `get` subcommand. `id` is a borrowed string so we can
/// surface the original input in error messages.
pub struct GetArgs {
    pub id: String,
    pub preview: bool,
    pub json: bool,
    pub quiet: bool,
}

/// Run the `get` subcommand.
pub fn run(args: &GetArgs, repo: &dyn ClipboardRepository) -> Result<(), String> {
    let entry = resolve(args, repo)?;
    let Some(entry) = entry else {
        return Err(format!("get: no entry for id '{}'", args.id));
    };

    if args.quiet {
        return Ok(());
    }
    if args.json {
        let view = CliEntry {
            id: entry.id,
            content_type: entry.content_type,
            content: entry.content,
            preview: entry.preview.clone(),
            tags: entry.tags,
            pinned: entry.is_pinned,
            timestamp: entry.timestamp,
        };
        let s = serde_json::to_string(&view)
            .map_err(|e| format!("get: serialize json: {e}"))?;
        println!("{s}");
        return Ok(());
    }
    if args.preview {
        println!("{}", entry.preview);
    } else {
        println!("{}", entry.content);
    }
    Ok(())
}

/// Resolve the requested ID against the repo. Returns `Ok(None)` when
/// the repo confirms no row exists (so the caller can decide the error
/// wording without the resolver baking in "no such id").
fn resolve(args: &GetArgs, repo: &dyn ClipboardRepository) -> Result<Option<ClipboardEntry>, String> {
    if args.id == LATEST_TOKEN {
        let mut rows = repo
            .get_history(1, 0, None)
            .map_err(|e| format!("get: latest lookup failed: {e}"))?;
        Ok(rows.pop())
    } else {
        let id: i64 = args
            .id
            .parse()
            .map_err(|e| format!("get: invalid id '{}': {e}", args.id))?;
        repo.get_entry_by_id(id)
            .map_err(|e| format!("get: lookup failed: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_token_is_lowercase() {
        assert_eq!(LATEST_TOKEN, "latest");
    }
}
