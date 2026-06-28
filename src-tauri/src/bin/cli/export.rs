//! `dzc export` — export clipboard history to a file.
//!
//! Two output formats:
//! * `JSON` (default, `.json`): plaintext pretty-printed JSON via
//!   `services::backup::export_to_json`. Stable schema, version-tagged.
//! * `ENCRYPTED` (`.dzc`, opt-in via `--encrypted`): AES-256-GCM
//!   binary blob via `services::backup::export_to_encrypted`. Argon2id
//!   KDF with the user's passphrase.
//!
//! ## Args
//!
//! * `path` — output file path. Extension drives default format
//!   detection: `.dzc` / `.tiez` → encrypted, anything else → JSON.
//! * `--encrypted` — force encrypted output. Auto-set when `path` ends
//!   in `.dzc` / `.tiez`. Requires `--passphrase`.
//! * `--passphrase <PW>` — passphrase for `--encrypted`. Must be at
//!   least 12 characters (G32 hard constraint, matches the in-app
//!   backup dialog).
//!
//! ## Flow
//!
//! 1. Validate args (passphrase length, encrypted+no-passphrase).
//! 2. Fetch all entries via `repo.get_history(i32::MAX, 0, None)`.
//! 3. Map `ClipboardEntry` → `services::backup::ExportEntry`.
//! 4. Serialize via `export_to_json` or `export_to_encrypted`.
//! 5. Write to `path` (creates parent dirs if missing).
//!
//! ## Output
//!
//! On success: human-friendly summary on stdout
//! (`Exported N entries to <path> (json|encrypted)`). On `--quiet`:
//! silent success. Errors are propagated as `Err(String)` with
//! actionable messages (e.g. "passphrase required", "passphrase must
//! be at least 12 characters", "io error: ...").

use std::io::Write;
use std::path::Path;

use crate::domain::models::ClipboardEntry;
use crate::infrastructure::repository::clipboard_repo::ClipboardRepository;
use crate::services::backup::{
    export_to_encrypted, export_to_json, ExportEntry,
};

/// Minimum acceptable passphrase length. Matches the
/// `BackupDialog` validation in the frontend and the G32 hard
/// constraint. 12 chars is the OWASP baseline for user-chosen
/// passphrases; shorter values are rejected outright.
pub const MIN_PASSPHRASE_LEN: usize = 12;

/// Args for the `export` subcommand. Borrowed so the binary can pass
/// the parsed clap struct straight through.
pub struct ExportArgs {
    pub path: String,
    /// Force encrypted output. Auto-detected when `path` ends in
    /// `.dzc`, but the flag lets the user write to any extension.
    pub encrypted: bool,
    /// Passphrase for encrypted output. Required when `encrypted` is
    /// `true`; ignored otherwise (so the user can use the same flag
    /// shape on both code paths).
    pub passphrase: Option<String>,
    /// Emit only an exit code; suppress the success summary.
    pub quiet: bool,
}

/// Format actually selected for the output file. Drives both the
/// human summary line and the dispatch to the right
/// `services::backup` function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExportFormat {
    Json,
    Encrypted,
}

/// Run the `export` subcommand.
///
/// Returns `Ok(())` on success. Errors are returned as `Err(String)`
/// for the binary to surface via `eprintln!` + exit code 1.
pub fn run(args: &ExportArgs, repo: &dyn ClipboardRepository) -> Result<(), String> {
    // 1. Determine format from args + path extension.
    let format = resolve_format(&args.path, args.encrypted)?;

    // 2. Passphrase validation. Only enforced for encrypted output.
    if format == ExportFormat::Encrypted {
        let pw = args.passphrase.as_deref().unwrap_or("");
        if pw.is_empty() {
            return Err("export: passphrase required for encrypted output".to_string());
        }
        if pw.chars().count() < MIN_PASSPHRASE_LEN {
            return Err(format!(
                "export: passphrase must be at least {MIN_PASSPHRASE_LEN} characters"
            ));
        }
    }

    // 3. Fetch every entry (unlimited). The repo's `get_history` is the
    //    only enumeration surface on the trait; it accepts a positive
    //    i32 limit, so we use `i32::MAX` for "all".
    let raw_entries = repo
        .get_history(i32::MAX, 0, None)
        .map_err(|e| format!("export: get_history failed: {e}"))?;

    // 4. Map to the export shape that the backup module knows how to
    //    serialize. Only fields the backup schema actually carries are
    //    populated; anything else (e.g. OCR text) stays as `None` for
    //    now — the backup schema has a forward-compatible `#[serde(default)]`.
    let export_entries: Vec<ExportEntry> = raw_entries.iter().map(to_export_entry).collect();
    let count = export_entries.len();

    // 5. Serialize.
    let (bytes, format_label): (Vec<u8>, &'static str) = match format {
        ExportFormat::Json => {
            let json = export_to_json(export_entries)
                .map_err(|e| format!("export: json serialize failed: {e}"))?;
            (json.into_bytes(), "json")
        }
        ExportFormat::Encrypted => {
            let blob = export_to_encrypted(export_entries, args.passphrase.as_deref().unwrap())
                .map_err(|e| format!("export: encrypt failed: {e}"))?;
            (blob, "encrypted")
        }
    };

    // 6. Write. Create parent dirs so the user can point at
    //    `/tmp/backup/2026/dezirclip.dzc` without mkdir first.
    let path = Path::new(&args.path);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("export: create_dir_all {parent:?}: {e}"))?;
        }
    }
    let mut file = std::fs::File::create(path)
        .map_err(|e| format!("export: create {path:?}: {e}"))?;
    file.write_all(&bytes)
        .map_err(|e| format!("export: write {path:?}: {e}"))?;

    // 7. Summary. Skipped under --quiet so the user can pipe a fresh
    //    file into another command without `head -1` ceremony.
    if !args.quiet {
        println!("Exported {count} entries to {} ({format_label})", args.path);
    }

    Ok(())
}

/// Pick the output format. Rules:
/// 1. `--encrypted` → `Encrypted`.
/// 2. `path` ends in `.dzc` or `.tiez` → `Encrypted` (auto-detect).
/// 3. Otherwise → `Json`.
///
/// encrypted extension auto-detection is what makes `dzc export foo.dzc`
/// "just work" without an extra flag, matching the in-app
/// `File > Export` dialog behavior.
fn resolve_format(path: &str, encrypted: bool) -> Result<ExportFormat, String> {
    if encrypted {
        return Ok(ExportFormat::Encrypted);
    }
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".dzc") || lower.ends_with(".tiez") {
        return Ok(ExportFormat::Encrypted);
    }
    Ok(ExportFormat::Json)
}

/// Map a `ClipboardEntry` to a `services::backup::ExportEntry`.
/// Only the fields the backup schema carries are populated; optional
/// fields with no source default to `None` so the JSON stays
/// minimal. `tags` flow through because the backup schema supports
/// them; `use_count` is intentionally not exported — the backup
/// format is for migration, not analytics.
fn to_export_entry(e: &ClipboardEntry) -> ExportEntry {
    ExportEntry {
        id: e.id,
        content_type: e.content_type.clone(),
        content: e.content.clone(),
        preview: Some(e.preview.clone()),
        html_content: e.html_content.clone(),
        source_app: if e.source_app.is_empty() {
            None
        } else {
            Some(e.source_app.clone())
        },
        source_app_path: e.source_app_path.clone(),
        // Backup schema splits `timestamp` into created_at / updated_at.
        // We don't currently track them separately, so both default to
        // the existing `timestamp`. Future work: derive `updated_at`
        // from the `updated_at` column when it lands.
        created_at: e.timestamp,
        updated_at: e.timestamp,
        use_count: e.use_count,
        is_pinned: e.is_pinned,
        pinned_order: e.pinned_order as i32,
        tags: e.tags.clone(),
        ocr_text: e.ocr_text.clone(),
        kinds: e.content_kinds.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_format_defaults_to_json() {
        assert_eq!(
            resolve_format("/tmp/out.json", false).unwrap(),
            ExportFormat::Json
        );
        assert_eq!(
            resolve_format("/tmp/out.txt", false).unwrap(),
            ExportFormat::Json
        );
        assert_eq!(
            resolve_format("/tmp/out", false).unwrap(),
            ExportFormat::Json
        );
    }

    #[test]
    fn resolve_format_dezirclip_extension_is_encrypted() {
        assert_eq!(
            resolve_format("/tmp/out.dzc", false).unwrap(),
            ExportFormat::Encrypted
        );
        // Case-insensitive.
        assert_eq!(
            resolve_format("/tmp/out.DZC", false).unwrap(),
            ExportFormat::Encrypted
        );
    }

    #[test]
    fn resolve_format_encrypted_flag_wins() {
        // Even with a .json path, --encrypted forces encrypted output.
        assert_eq!(
            resolve_format("/tmp/out.json", true).unwrap(),
            ExportFormat::Encrypted
        );
    }

    #[test]
    fn to_export_entry_preserves_required_fields() {
        let e = ClipboardEntry {
            id: 42,
            content_type: "text".to_string(),
            content: "hello".to_string(),
            html_content: None,
            source_app: "test-app".to_string(),
            source_app_path: None,
            timestamp: 1_700_000_000,
            preview: "hello".to_string(),
            is_pinned: true,
            tags: vec!["work".to_string()],
            use_count: 7,
            is_external: false,
            pinned_order: 0,
            file_preview_exists: false,
            content_kinds: vec!["text".to_string()],
            ocr_text: None,
            ocr_status: None,
        };
        let exp = to_export_entry(&e);
        assert_eq!(exp.id, 42);
        assert_eq!(exp.content_type, "text");
        assert_eq!(exp.content, "hello");
        assert_eq!(exp.preview.as_deref(), Some("hello"));
        assert_eq!(exp.source_app.as_deref(), Some("test-app"));
        assert!(exp.is_pinned);
        assert_eq!(exp.use_count, 7);
        assert_eq!(exp.tags, vec!["work".to_string()]);
    }
}
