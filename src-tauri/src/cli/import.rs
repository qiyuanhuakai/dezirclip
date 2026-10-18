//! `dzc import` — import clipboard history from a file.
//!
//! Two input formats, decided by file extension:
//! * `.json` (or any other extension): plaintext JSON produced by
//!   `dzc export` or by the in-app `File > Import` dialog. Parsed
//!   via `services::backup::entries_from_json`.
//! * `.dzc` / `.tiez`: AES-256-GCM encrypted binary blob. Decrypted via
//!   `services::backup::decrypt_to_json` (which transparently returns
//!   `BackupError::WrongPassphrase` on tag mismatch — the CLI surfaces
//!   that as "wrong passphrase").
//!
//! ## Args
//!
//! * `path` — input file path. Extension drives format detection.
//! * `--mode merge|replace` (default `merge`):
//!   * `merge` — keep existing rows; overwrite on `id` collision
//!     (the import routine preserves the existing `id` and rewrites
//!     content/tags).
//!   * `replace` — clear the table first, then insert every row from
//!     the file. Destructive.
//! * `--passphrase <PW>` — required for encrypted backup files. The CLI
//!   rejects encrypted imports without `--passphrase` early so the user
//!   doesn't waste time on a 19-MiB Argon2id derivation.
//!
//! ## Flow
//!
//! 1. Validate args (mode spelling, passphrase for encrypted backups).
//! 2. Read the file.
//! 3. For encrypted backups: decrypt with `--passphrase`. For `.json`: parse
//!    JSON directly.
//! 4. Resolve `ImportMode` from the `--mode` arg.
//! 5. Import all entries in one transaction, preserving IDs and metadata.
//!    Replace removes all existing rows; merge overwrites matching IDs.
//!
//! ## Output
//!
//! Human summary on success: `Imported <n>/<total> entries from <path>
//! (<json|encrypted>, mode <merge|replace>)`. Errors are propagated as
//! `Err(String)`.

use std::path::Path;

use crate::infrastructure::repository::clipboard_repo::SqliteClipboardRepository;
use crate::services::backup::{decrypt_to_json, entries_from_json, import_from_json, ImportMode};

/// Args for the `import` subcommand.
pub struct ImportArgs {
    pub path: String,
    /// Either the literal `"merge"` or `"replace"` (case-insensitive).
    /// Anything else returns an error at validate time.
    pub mode: String,
    /// Passphrase for encrypted backup files. Required when the file is
    /// encrypted; ignored for JSON.
    pub passphrase: Option<String>,
    /// Emit only an exit code; suppress the success summary.
    pub quiet: bool,
}

/// Run the `import` subcommand.
///
/// Returns `Ok(())` on success. Errors are returned as `Err(String)`
/// for the binary to surface via `eprintln!` + exit code 1.
pub fn run(args: &ImportArgs, repo: &SqliteClipboardRepository) -> Result<(), String> {
    // 1. Mode spelling.
    let mode = parse_mode(&args.mode)?;

    // 2. Detect format from extension.
    let path = Path::new(&args.path);
    let is_encrypted = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("dzc") || e.eq_ignore_ascii_case("tiez"))
        .unwrap_or(false);

    // 3. encrypted backups require a passphrase up-front. Fail fast before doing
    //    the (expensive) Argon2id KDF derivation.
    if is_encrypted
        && args
            .passphrase
            .as_deref()
            .map(str::is_empty)
            .unwrap_or(true)
    {
        return Err("import: passphrase required for encrypted backups".to_string());
    }

    // 4. Read the file. Surface the underlying IO error verbatim —
    //    a `No such file or directory` message is the most useful
    //    thing we can show the user here.
    let bytes = std::fs::read(path).map_err(|e| format!("import: read {path:?}: {e}"))?;

    // 5. Decode → JSON string.
    let json = if is_encrypted {
        let pw = args.passphrase.as_deref().unwrap();
        decrypt_to_json(&bytes, pw).map_err(|e| format!("import: decrypt failed: {e}"))?
    } else {
        // Plain JSON: convert bytes to string (lossy is fine — the
        // backup module's serde_json will reject anything that isn't
        // strict UTF-8 anyway).
        String::from_utf8(bytes).map_err(|e| format!("import: file is not valid UTF-8: {e}"))?
    };

    // 6. Parse → Vec<ExportEntry>. The backup module's
    //    `entries_from_json` does NOT validate the version field —
    //    it's a structural parse only. We re-parse via
    //    `import_from_json` below to surface version errors.
    import_from_json(&json, mode).map_err(|e| format!("import: invalid backup: {e}"))?;
    let export_entries =
        entries_from_json(&json).map_err(|e| format!("import: parse json: {e}"))?;
    let total = export_entries.len();

    // 7. Apply. Replace mode clears first; merge is upsert-on-id.
    crate::services::backup::apply_entries(repo, &export_entries, mode)?;
    let imported = export_entries.len();

    if !args.quiet {
        let label = if is_encrypted { "encrypted" } else { "json" };
        println!(
            "Imported {imported}/{total} entries from {} ({label}, mode {})",
            args.path,
            mode.as_str(),
        );
    }

    Ok(())
}

/// Parse `--mode` into a typed `ImportMode`. The string is matched
/// case-insensitively so `--mode Merge` works the same as
/// `--mode merge`. Empty input falls back to `Merge` (matches the
/// default-value behavior in the binary).
fn parse_mode(raw: &str) -> Result<ImportMode, String> {
    match raw.to_ascii_lowercase().as_str() {
        "" | "merge" => Ok(ImportMode::Merge),
        "replace" => Ok(ImportMode::Replace),
        other => Err(format!(
            "import: invalid mode '{other}' (expected 'merge' or 'replace')"
        )),
    }
}

/// Map a `services::backup::ExportEntry` to a `ClipboardEntry` for
/// persistence. The reverse of `export::to_export_entry` modulo
/// fields the runtime model needs but the export schema doesn't carry
/// (e.g. `is_external`, `file_preview_exists` — both default to
/// `false`; `pinned_order` rounds from `i32` back to `i64`).

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mode_accepts_both_cases() {
        assert_eq!(parse_mode("merge").unwrap(), ImportMode::Merge);
        assert_eq!(parse_mode("MERGE").unwrap(), ImportMode::Merge);
        assert_eq!(parse_mode("Replace").unwrap(), ImportMode::Replace);
        assert_eq!(parse_mode("").unwrap(), ImportMode::Merge);
    }

    #[test]
    fn parse_mode_rejects_unknown() {
        let err = parse_mode("delete").unwrap_err();
        assert!(err.contains("invalid mode"), "got: {err}");
        assert!(err.contains("delete"), "got: {err}");
    }
}
