//! Standalone inspect tool for .dzc backup files (plain JSON or AES-GCM encrypted).
//!
//! Companion to `services/backup.rs`. Use for cross-platform verification:
//! decrypt a file produced on any OS without needing the Tauri runtime.
//!
//! Build: `cargo build --no-default-features --example backup-inspect`
//! Run:   `cargo run --no-default-features --example backup-inspect -- <file> [passphrase]`
//!        `cargo run --no-default-features --example backup-inspect -- --schema <file>`
//!
//! Exit codes:
//!     0 — success
//!     1 — bad args / I/O error
//!     2 — decryption failed (wrong passphrase / corrupted blob)
//!     3 — JSON parse / version mismatch
//!
//! This binary uses the library's shared backup format and cryptography.
//!
//! ## Build env note
//!
//! This example shares the CLI library. `--no-default-features` excludes
//! the desktop dependencies and their graphical build requirements.

use std::env;
use std::fs;
use std::io::Write;
use std::process::ExitCode;

use dezirclip::services::backup::{decrypt_to_json, entries_from_json, ExportEntry};
use sha2::{Digest, Sha256};

const HEADER_LEN: usize = 12 + 16;

fn looks_encrypted(data: &[u8]) -> bool {
    if data.len() < HEADER_LEN {
        return false;
    }
    if let Ok(s) = std::str::from_utf8(data) {
        if s.trim_start().starts_with('{') {
            return false;
        }
    }
    true
}

fn sha256_hex(b: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(b);
    hex::encode(h.finalize())
}

fn inspect_json(json_str: &str) -> Result<(), String> {
    let entries: Vec<ExportEntry> = entries_from_json(json_str).map_err(|e| e.to_string())?;
    println!("format: json");
    println!("entries: {}", entries.len());
    for (i, e) in entries.iter().enumerate() {
        println!(
            "  [{}] id={} content_sha256={}",
            i,
            e.id,
            sha256_hex(e.content.as_bytes())
        );
    }
    Ok(())
}

fn inspect_encrypted(data: &[u8], passphrase: &str) -> Result<(), String> {
    let json = decrypt_to_json(data, passphrase).map_err(|e| e.to_string())?;
    inspect_json(&json)
}

fn cmd_schema(data: &[u8]) -> Result<(), String> {
    println!("file_size: {} bytes", data.len());
    println!("looks_encrypted: {}", looks_encrypted(data));
    if looks_encrypted(data) {
        let nonce = &data[..12];
        let salt = &data[12..28];
        let ct = &data[28..];
        println!("nonce_hex: {}", hex::encode(nonce));
        println!("salt_hex: {}", hex::encode(salt));
        println!("ciphertext_tag_len: {} bytes", ct.len());
    }
    Ok(())
}

fn run() -> Result<u8, String> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: {} <file> [passphrase] | --schema <file>", args[0]);
        return Ok(1);
    }
    match args[1].as_str() {
        "--schema" => {
            let path = args.get(2).ok_or("--schema needs <file>")?;
            let data = fs::read(path).map_err(|e| format!("read: {e}"))?;
            cmd_schema(&data)?;
            Ok(0)
        }
        path => {
            let data = fs::read(path).map_err(|e| format!("read: {e}"))?;
            if data.is_empty() {
                return Err("file is empty".into());
            }
            if looks_encrypted(&data) {
                let pw = args
                    .get(2)
                    .ok_or_else(|| "encrypted file requires passphrase".to_string())?;
                match inspect_encrypted(&data, pw) {
                    Ok(()) => Ok(0),
                    Err(e) => {
                        eprintln!("WRONG_PASSPHRASE_OR_DECRYPT_ERROR: {e}");
                        Ok(2)
                    }
                }
            } else {
                let s = std::str::from_utf8(&data).map_err(|e| format!("utf8: {e}"))?;
                match inspect_json(s) {
                    Ok(()) => Ok(0),
                    Err(e) => {
                        eprintln!("PARSE_ERROR: {e}");
                        Ok(3)
                    }
                }
            }
        }
    }
}

fn main() -> ExitCode {
    let stdout = std::io::stdout();
    let mut h = stdout.lock();
    match run() {
        Ok(0) => ExitCode::SUCCESS,
        Ok(code) => {
            let _ = writeln!(h, "exit_code: {code}");
            ExitCode::from(code)
        }
        Err(e) => {
            let _ = writeln!(h, "error: {e}");
            ExitCode::from(1)
        }
    }
}
