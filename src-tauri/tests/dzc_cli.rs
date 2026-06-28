use std::path::Path;
use std::process::Command;

fn run_dzc(db_path: &Path, args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_dzc"))
        .env("DEZIRCLIP_DB_PATH", db_path)
        .args(args)
        .output()
        .expect("run dzc");
    assert!(
        output.status.success(),
        "dzc {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("utf8 stdout")
}

#[test]
fn dzc_import_into_new_database_persists_entries() {
    let stamp = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    );
    let source_db = std::env::temp_dir().join(format!("dezirclip-dzc-source-{stamp}.db"));
    let target_db = std::env::temp_dir().join(format!("dezirclip-dzc-target-{stamp}.db"));
    let backup = std::env::temp_dir().join(format!("dezirclip-dzc-{stamp}.dzc"));
    let backup_arg = backup.to_string_lossy().to_string();
    let content = "DezirClip CLI import persistence";

    run_dzc(&source_db, &["add", content, "--type", "text"]);
    run_dzc(
        &source_db,
        &[
            "export",
            &backup_arg,
            "--encrypted",
            "--passphrase",
            "dezirclip-passphrase",
        ],
    );
    run_dzc(
        &target_db,
        &[
            "import",
            &backup_arg,
            "--mode",
            "merge",
            "--passphrase",
            "dezirclip-passphrase",
        ],
    );
    let imported = run_dzc(&target_db, &["list", "5", "--json"]);

    assert!(imported.contains(content), "imported list: {imported}");

    let _ = std::fs::remove_file(source_db);
    let _ = std::fs::remove_file(target_db);
    let _ = std::fs::remove_file(backup);
}
