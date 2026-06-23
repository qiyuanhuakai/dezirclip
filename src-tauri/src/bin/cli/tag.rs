//! `tiez-c tag` — manage tags on an entry.
//!
//! Subcommands:
//! * `add <ID> <TAG>`    — append `<TAG>` to the entry's tag list
//! * `remove <ID> <TAG>` — remove `<TAG>` from the entry's tag list
//! * `list <ID>`         — print the entry's tags, one per line
//!
//! All mutations go through `TagRepository::update_entry_tags`, which
//! the bin wires via its `tag_repo` handle. `list` reads tags off the
//! `ClipboardEntry` returned by `clip_repo.get_entry_by_id` — it does
//! NOT use the `tag_repo`, so the listing reflects what's stored on
//! the entry, not what's in the `entry_tags` join table. (The two are
//! kept in sync by `update_entry_tags` so this is safe.)
//!
//! Tag semantics:
//! * whitespace is trimmed; empty tags are rejected with an error
//! * duplicates are silently de-duplicated (no error if you add a tag
//!   that's already present — this matches the GUI's behavior)
//! * `remove` of a non-present tag is a no-op (still `Ok(())`)

use crate::infrastructure::repository::clipboard_repo::ClipboardRepository;
use crate::infrastructure::repository::tag_repo::TagRepository;

/// Subcommand for the `tag` command. The bin parses this from clap's
/// subcommand block; the cli module just dispatches on the variant.
#[derive(Debug, Clone)]
pub enum TagCommand {
    Add { id: i64, tag: String },
    Remove { id: i64, tag: String },
    List { id: i64 },
}

/// Args for the `tag` subcommand.
pub struct TagArgs {
    pub command: TagCommand,
}

/// Run the `tag` subcommand. Returns `Ok(())` on success; the output
/// (`"Added: <tag> to id=<n>"`, `"Removed: <tag> from id=<n>"`, or the
/// list of tags) is written to stdout by `run`.
pub fn run(
    args: &TagArgs,
    clip_repo: &dyn ClipboardRepository,
    tag_repo: &dyn TagRepository,
) -> Result<(), String> {
    match &args.command {
        TagCommand::Add { id, tag } => {
            let cleaned = clean_tag(tag).ok_or_else(|| "tag: tag cannot be empty".to_string())?;
            let mut current = fetch_tags(*id, clip_repo)?;
            if !current.iter().any(|t| t == &cleaned) {
                current.push(cleaned.clone());
            }
            tag_repo
                .update_entry_tags(*id, current)
                .map_err(|e| format!("tag add: {e}"))?;
            println!("tag: added {cleaned:?} to id={id}");
            Ok(())
        }
        TagCommand::Remove { id, tag } => {
            let cleaned = clean_tag(tag).ok_or_else(|| "tag: tag cannot be empty".to_string())?;
            let mut current = fetch_tags(*id, clip_repo)?;
            let before = current.len();
            current.retain(|t| t != &cleaned);
            // If nothing changed, the tag wasn't there — still a no-op
            // success because the post-state is what the user wanted.
            if current.len() != before {
                tag_repo
                    .update_entry_tags(*id, current)
                    .map_err(|e| format!("tag remove: {e}"))?;
            }
            println!("tag: removed {cleaned:?} from id={id}");
            Ok(())
        }
        TagCommand::List { id } => {
            let current = fetch_tags(*id, clip_repo)?;
            if current.is_empty() {
                println!("(no tags)");
            } else {
                for t in &current {
                    println!("{t}");
                }
            }
            Ok(())
        }
    }
}

/// Fetch the entry's current tags. Returns `Err` if the entry does
/// not exist; an entry with no tags is a valid `Ok(vec![])`.
fn fetch_tags(id: i64, clip_repo: &dyn ClipboardRepository) -> Result<Vec<String>, String> {
    let entry = clip_repo
        .get_entry_by_id(id)
        .map_err(|e| format!("tag: lookup failed: {e}"))?;
    entry
        .map(|e| e.tags)
        .ok_or_else(|| format!("tag: no entry for id={id}"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_tag_trims_whitespace() {
        assert_eq!(clean_tag("  work  "), Some("work".to_string()));
        assert_eq!(clean_tag("\twork\n"), Some("work".to_string()));
    }

    #[test]
    fn clean_tag_rejects_empty() {
        assert_eq!(clean_tag(""), None);
        assert_eq!(clean_tag("   "), None);
        assert_eq!(clean_tag("\t\n"), None);
    }
}
