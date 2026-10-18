//! Fetch complete stored content; raw and MIME output never append a newline.
use crate::domain::models::ClipboardEntry;
use crate::infrastructure::repository::{
    cli_query::CliQuery, clipboard_repo::SqliteClipboardRepository,
};
use std::io::Write;

pub const LATEST_TOKEN: &str = "latest";
pub struct GetArgs {
    pub id: String,
    pub preview: bool,
    pub json: bool,
    pub quiet: bool,
    pub raw: bool,
    pub mime: Option<String>,
    pub exclude_sensitive: bool,
}

pub fn resolve(
    id: &str,
    exclude_sensitive: bool,
    repo: &SqliteClipboardRepository,
) -> Result<ClipboardEntry, String> {
    let latest = id == LATEST_TOKEN;
    let numeric = if latest {
        None
    } else {
        Some(
            id.parse::<i64>()
                .map_err(|e| format!("get: invalid id '{id}': {e}"))?,
        )
    };
    repo.query_cli(&CliQuery {
        limit: 1,
        id: numeric,
        latest,
        exclude_sensitive,
        ..Default::default()
    })?
    .pop()
    .ok_or_else(|| format!("get: no entry for id '{id}'"))
}

pub fn run(args: &GetArgs, repo: &SqliteClipboardRepository) -> Result<(), String> {
    let entry = resolve(&args.id, args.exclude_sensitive, repo)?;
    if args.quiet {
        return Ok(());
    }
    if args.json {
        println!(
            "{}",
            serde_json::to_string(&super::list::json_view(&entry, false)?)
                .map_err(|e| e.to_string())?
        );
    } else if let Some(mime) = &args.mime {
        std::io::stdout()
            .lock()
            .write_all(&super::payload::bytes_for(&entry, mime)?)
            .map_err(|e| e.to_string())?;
    } else {
        let value = if args.preview {
            &entry.preview
        } else {
            &entry.content
        };
        let mut out = std::io::stdout().lock();
        out.write_all(value.as_bytes()).map_err(|e| e.to_string())?;
        if !args.raw {
            out.write_all(b"\n").map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
