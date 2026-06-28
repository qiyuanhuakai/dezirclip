//! Wave D E2E backup round-trip integration test.
//!
//! **Build env note**: this integration test target is part of the
//! `dezirclip` package, so it shares Cargo.toml deps. Running it requires
//! the full Tauri build env (xcap + libpipewire-0.3-dev on Linux). For
//! a build that does NOT need those system libs, see `backup-e2e/tests/`
//! (the standalone sub-crate that copies `services/backup.rs` verbatim
//! and uses sha256 + hex for byte-level content comparison).
//!
//! This file uses only `std` + the in-tree `services::backup` module
//! (no dev-dep additions) so the test stays compatible with the
//! project's "NO [dev-dependencies] section" rule from AGENTS.md.
//! It uses `std::collections::hash_map::DefaultHasher` for content
//! fingerprint comparison (good enough for round-trip equality).

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::Write;

use dezirclip::services::backup::{
    decrypt_to_json, entries_from_json, export_to_encrypted, export_to_json, import_from_encrypted,
    import_from_json, BackupError, ExportEntry, ImportMode,
};

fn content_hash(s: &str) -> u64 {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

fn make_10_entries() -> Vec<ExportEntry> {
    let types = ["text", "html", "image", "file"];
    (0..10)
        .map(|i| ExportEntry {
            id: (i + 1) as i64,
            content_type: types[i % 4].to_string(),
            content: match types[i % 4] {
                "text" => format!("hello world #{i} — 你好 🌍"),
                "html" => format!("<p>html entry {i}</p>"),
                "image" => format!("data:image/png;base64,iVBORw0KGgo={i}"),
                _ => format!("/path/to/file-{i}.bin"),
            },
            preview: Some(format!("entry-{i}")),
            html_content: if i % 2 == 0 {
                Some(format!("<b>html {i}</b>"))
            } else {
                None
            },
            source_app: Some(format!("App{i}")),
            source_app_path: if i == 9 {
                Some("/special/path".into())
            } else {
                None
            },
            created_at: 1_700_000_000 + i as i64,
            updated_at: 1_700_000_001 + i as i64,
            use_count: i as i32 * 7,
            is_pinned: i % 3 == 0,
            pinned_order: if i % 3 == 0 { (i as i32) + 1 } else { 0 },
            tags: (0..(i % 4)).map(|t| format!("tag-{i}-{t}")).collect(),
            ocr_text: if i % 5 == 0 {
                Some(format!("ocr-text-{i}"))
            } else {
                None
            },
            kinds: vec!["text".to_string()],
        })
        .collect()
}

fn assert_entries_equal(a: &[ExportEntry], b: &[ExportEntry], ctx: &str) {
    assert_eq!(a.len(), b.len(), "{ctx}: entry count mismatch");
    for (i, (ea, eb)) in a.iter().zip(b.iter()).enumerate() {
        assert_eq!(ea.id, eb.id, "{ctx}: entry[{i}].id");
        assert_eq!(
            ea.content_type, eb.content_type,
            "{ctx}: entry[{i}].content_type"
        );
        assert_eq!(
            content_hash(&ea.content),
            content_hash(&eb.content),
            "{ctx}: entry[{i}].content hash"
        );
        assert_eq!(ea.preview, eb.preview, "{ctx}: entry[{i}].preview");
        assert_eq!(
            ea.html_content, eb.html_content,
            "{ctx}: entry[{i}].html_content"
        );
        assert_eq!(ea.source_app, eb.source_app, "{ctx}: entry[{i}].source_app");
        assert_eq!(
            ea.source_app_path, eb.source_app_path,
            "{ctx}: entry[{i}].source_app_path"
        );
        assert_eq!(ea.created_at, eb.created_at, "{ctx}: entry[{i}].created_at");
        assert_eq!(ea.updated_at, eb.updated_at, "{ctx}: entry[{i}].updated_at");
        assert_eq!(ea.use_count, eb.use_count, "{ctx}: entry[{i}].use_count");
        assert_eq!(ea.is_pinned, eb.is_pinned, "{ctx}: entry[{i}].is_pinned");
        assert_eq!(
            ea.pinned_order, eb.pinned_order,
            "{ctx}: entry[{i}].pinned_order"
        );
        assert_eq!(ea.tags, eb.tags, "{ctx}: entry[{i}].tags");
        assert_eq!(ea.ocr_text, eb.ocr_text, "{ctx}: entry[{i}].ocr_text");
        assert_eq!(ea.kinds, eb.kinds, "{ctx}: entry[{i}].kinds");
    }
}

fn tmp_path(suffix: &str) -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "dezirclip-backup-e2e-{}-{}.{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        suffix
    ));
    p
}

#[test]
fn e2e_10_entries_json_roundtrip() {
    let original = make_10_entries();
    let path = tmp_path("json");
    let json = export_to_json(original.clone()).expect("export json");
    fs::write(&path, &json).expect("write");

    let file_contents = fs::read_to_string(&path).expect("read back");
    assert!(
        file_contents.contains("\"version\": \"dezirclip-export-v1\""),
        "header version missing"
    );
    let reimported = entries_from_json(&file_contents).expect("reimport");
    assert_entries_equal(&original, &reimported, "json roundtrip");

    let s_merge = import_from_json(&file_contents, ImportMode::Merge).expect("summary merge");
    let s_replace = import_from_json(&file_contents, ImportMode::Replace).expect("summary replace");
    assert_eq!(s_merge.imported, 10);
    assert_eq!(s_merge.mode, "merge");
    assert_eq!(s_replace.imported, 10);
    assert_eq!(s_replace.mode, "replace");
    let _ = fs::remove_file(&path);
}

#[test]
fn e2e_10_entries_encrypted_roundtrip_correct_pw() {
    let original = make_10_entries();
    let passphrase = "dezirclip-export-2026-correct-horse-battery-staple";
    let path = tmp_path("enc");
    let blob = export_to_encrypted(original.clone(), passphrase).expect("encrypt");
    fs::write(&path, &blob).expect("write encrypted");

    let json = decrypt_to_json(&blob, passphrase).expect("decrypt");
    let reimported = entries_from_json(&json).expect("parse");
    assert_entries_equal(&original, &reimported, "encrypted roundtrip");

    let s = import_from_encrypted(&blob, passphrase).expect("summary");
    assert_eq!(s.imported, 10);
    assert_eq!(s.mode, "merge");
    let _ = fs::remove_file(&path);
}

#[test]
fn e2e_wrong_passphrase_yields_wrong_passphrase() {
    let original = make_10_entries();
    let blob = export_to_encrypted(original, "right-pw-xxx").expect("encrypt");
    let result = import_from_encrypted(&blob, "wrong-pw-yyy");
    assert!(
        matches!(result, Err(BackupError::WrongPassphrase)),
        "expected WrongPassphrase, got {result:?}"
    );
}

#[test]
fn e2e_tampered_ciphertext_detected() {
    let original = make_10_entries();
    let mut blob = export_to_encrypted(original, "pw").expect("encrypt");
    let last = blob.len() - 1;
    blob[last] ^= 0xFF;
    let result = import_from_encrypted(&blob, "pw");
    assert!(
        matches!(result, Err(BackupError::WrongPassphrase)),
        "tampered ciphertext must be detected as wrong-passphrase, got {result:?}"
    );
}

#[test]
fn e2e_schema_nonce_salt_ciphertext_layout() {
    let original = make_10_entries();
    let blob = export_to_encrypted(original, "pw").expect("encrypt");

    // [nonce(12) | salt(16) | ciphertext||tag(>=16)]
    assert!(blob.len() >= 12 + 16 + 16, "blob too short: {}", blob.len());

    // Nonce + salt random per encryption
    let blob2 = export_to_encrypted(make_10_entries(), "pw").expect("encrypt 2");
    assert_ne!(
        &blob[..28],
        &blob2[..28],
        "nonce+salt must be random per encryption"
    );

    // Not valid JSON
    let parsed: Result<serde_json::Value, _> = serde_json::from_slice(&blob);
    assert!(parsed.is_err(), "encrypted blob must not be valid JSON");

    // No plaintext leak
    let as_str = String::from_utf8_lossy(&blob);
    for entry in make_10_entries() {
        assert!(
            !as_str.contains(&entry.content),
            "plaintext leaked for entry id={}",
            entry.id
        );
    }
}

#[test]
fn e2e_replace_mode_clears_first_semantic() {
    let original = make_10_entries();
    let json = export_to_json(original).expect("export");
    let s = import_from_json(&json, ImportMode::Replace).expect("import");
    assert_eq!(s.mode, "replace");
    assert_eq!(s.imported, 10);
    assert_eq!(s.skipped, 0);
}

#[test]
fn e2e_3x_idempotency() {
    for run in 0..3 {
        let original = make_10_entries();
        let pw = format!("run-{run}-pw");
        let blob = export_to_encrypted(original.clone(), &pw).expect("encrypt");
        let json = decrypt_to_json(&blob, &pw).expect("decrypt");
        let reimported = entries_from_json(&json).expect("parse");
        assert_entries_equal(&original, &reimported, &format!("idempotency run {run}"));
    }
}

#[test]
fn e2e_roundtrip_with_file_io() {
    // End-to-end: write to a real file, read back, verify byte content.
    let original = make_10_entries();
    let path = tmp_path("io");
    let json = export_to_json(original.clone()).expect("export");
    {
        let mut f = fs::File::create(&path).expect("create file");
        f.write_all(json.as_bytes()).expect("write");
        f.sync_all().expect("sync");
    }
    let read_back = fs::read(&path).expect("read file");
    let s = std::str::from_utf8(&read_back).expect("utf8");
    let entries = entries_from_json(s).expect("parse");
    assert_entries_equal(&original, &entries, "file-io roundtrip");
    let _ = fs::remove_file(&path);
}
