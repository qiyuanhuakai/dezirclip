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
//! 2. Fetch all entries via `repo.query_cli(&CliQuery {limit:i32::MAX,..Default::default()})`.
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

use base64::{engine::general_purpose::STANDARD, Engine};

use crate::domain::models::ClipboardEntry;
use crate::infrastructure::repository::{
    cli_query::CliQuery, clipboard_repo::SqliteClipboardRepository,
};
use crate::services::backup::{export_to_encrypted, export_to_json, ExportEntry};

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
pub fn run(args: &ExportArgs, repo: &SqliteClipboardRepository) -> Result<(), String> {
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

    // Read complete payloads rather than the desktop history preview projection.
    let raw_entries = repo
        .query_cli(&CliQuery {
            limit: i32::MAX,
            ..Default::default()
        })
        .map_err(|e| format!("export: history lookup failed: {e}"))?;

    // Resolve image attachments before opening the destination, so a missing
    // source cannot overwrite a usable backup with dangling image paths.
    let export_entries: Vec<ExportEntry> = raw_entries
        .iter()
        .map(to_export_entry)
        .collect::<Result<_, _>>()?;
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
    let mut file =
        std::fs::File::create(path).map_err(|e| format!("export: create {path:?}: {e}"))?;
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

/// Image records carry their original encoded bytes across machines; file
/// lists and resources linked from HTML remain references to their sources.
fn to_export_entry(e: &ClipboardEntry) -> Result<ExportEntry, String> {
    let content = if e.content_type == "image" && !e.content.starts_with("data:image/") {
        let bytes = super::payload::image_bytes(&e.content)
            .map_err(|error| format!("export: image attachment id={}: {error}", e.id))?;
        let format = image::guess_format(&bytes)
            .map_err(|error| format!("export: invalid image attachment id={}: {error}", e.id))?;
        image::load_from_memory(&bytes)
            .map_err(|error| format!("export: invalid image attachment id={}: {error}", e.id))?;
        format!(
            "data:{};base64,{}",
            format.to_mime_type(),
            STANDARD.encode(bytes)
        )
    } else {
        e.content.clone()
    };

    Ok(ExportEntry {
        id: e.id,
        content_type: e.content_type.clone(),
        content,
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
    })
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
        let exp = to_export_entry(&e).unwrap();
        assert_eq!(exp.id, 42);
        assert_eq!(exp.content_type, "text");
        assert_eq!(exp.content, "hello");
        assert_eq!(exp.preview.as_deref(), Some("hello"));
        assert_eq!(exp.source_app.as_deref(), Some("test-app"));
        assert!(exp.is_pinned);
        assert_eq!(exp.use_count, 7);
        assert_eq!(exp.tags, vec!["work".to_string()]);
    }

    fn image_entry(content: String) -> ClipboardEntry {
        ClipboardEntry {
            id: 51,
            content_type: "image".to_string(),
            content,
            html_content: None,
            source_app: "test-app".to_string(),
            source_app_path: None,
            timestamp: 1234,
            preview: "image".to_string(),
            is_pinned: false,
            tags: Vec::new(),
            use_count: 0,
            is_external: true,
            pinned_order: 0,
            file_preview_exists: true,
            content_kinds: vec!["image".to_string()],
            ocr_text: None,
            ocr_status: None,
        }
    }

    fn temporary_path(label: &str) -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("dzc-export-{label}-{}-{nonce}", std::process::id()))
    }

    #[test]
    fn image_attachment_export_survives_source_removal_with_original_bytes() {
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::new(1, 1)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let bytes = png.into_inner();
        let path = temporary_path("image%original.png");
        std::fs::write(&path, &bytes).unwrap();
        let entry = image_entry(path.to_string_lossy().into_owned());
        let exported = to_export_entry(&entry);
        std::fs::remove_file(path).unwrap();

        let json = export_to_json(vec![exported.unwrap()]).unwrap();
        let imported = crate::services::backup::entries_from_json(&json).unwrap();
        let (header, payload) = imported[0].content.split_once(',').unwrap();
        assert_eq!(header, "data:image/png;base64");
        assert_eq!(STANDARD.decode(payload).unwrap(), bytes);
        assert_eq!(imported[0].id, entry.id);
    }

    #[test]
    fn image_data_urls_are_exported_unchanged() {
        let content = "data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///w==";
        let exported = to_export_entry(&image_entry(content.to_string())).unwrap();
        assert_eq!(exported.content, content);
    }

    #[test]
    fn missing_image_attachment_does_not_overwrite_existing_backup() {
        use crate::infrastructure::repository::clipboard_repo::ClipboardRepository;
        use std::sync::{Arc, Mutex};

        let path = temporary_path("missing.png");
        let output = temporary_path("backup.json");
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        crate::infrastructure::repository::migrations::run_migrations(&mut connection).unwrap();
        let repo = SqliteClipboardRepository::new(Arc::new(Mutex::new(connection)));
        let mut entry = image_entry(path.to_string_lossy().into_owned());
        entry.id = 0;
        let id = repo.save(&entry, None).unwrap();
        std::fs::write(&output, b"existing backup").unwrap();
        let result = run(
            &ExportArgs {
                path: output.to_string_lossy().into_owned(),
                encrypted: false,
                passphrase: None,
                quiet: true,
            },
            &repo,
        );
        let existing = std::fs::read(&output).unwrap();
        std::fs::remove_file(output).unwrap();

        let error = result.unwrap_err();
        assert!(error.contains(&format!("image attachment id={id}")));
        assert_eq!(existing, b"existing backup");
    }
}
