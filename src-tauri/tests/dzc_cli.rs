use std::path::Path;
use std::process::Command;

/// A per-test database path, unique per process and clock reading so the
/// parallel test runner cannot hand two tests the same file.
fn scratch_db(label: &str) -> std::path::PathBuf {
    let stamp = format!(
        "{}-{}-{}",
        label,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    );
    std::env::temp_dir().join(format!("dezirclip-dzc-{stamp}.db"))
}

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

/// Run `dzc` expecting failure, returning (exit code, stdout + stderr).
fn run_dzc_expecting_failure(db_path: &Path, args: &[&str]) -> (i32, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_dzc"))
        .env("DEZIRCLIP_DB_PATH", db_path)
        .args(args)
        .output()
        .expect("run dzc");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.code().unwrap_or(-1), combined)
}

#[test]
fn theme_validation_rejects_resources_without_opening_history_or_applying_settings() {
    let db = scratch_db("theme-validation");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../skills/dezirclip-theme/assets/forest.theme.json");
    let result = run_dzc(
        &db,
        &["theme", "validate", fixture.to_str().unwrap(), "--json"],
    );
    let result: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(result["valid"], true);
    assert_eq!(result["id"], "forest");
    assert!(!db.exists());

    let source = db.with_extension("theme.json");
    let mut package: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture).unwrap()).unwrap();
    package["css"] = serde_json::json!(".theme-custom { background: url(https://example.invalid/resource); }");
    std::fs::write(&source, serde_json::to_vec(&package).unwrap()).unwrap();
    let (code, _) = run_dzc_expecting_failure(
        &db,
        &["theme", "validate", source.to_str().unwrap()],
    );
    assert_eq!(code, 1);
    assert!(!db.exists());
    std::fs::remove_file(source).unwrap();
}

/// Pin then unpin has to actually move the row's flag, and `list --pinned`
/// is the only way to observe it from outside.
#[test]
fn dzc_pin_and_unpin_move_the_entry_in_and_out_of_the_pinned_list() {
    let db = scratch_db("pin");
    run_dzc(&db, &["add", "pin round trip", "--type", "text"]);

    let all = run_dzc(&db, &["list", "10", "--json"]);
    let id = extract_first_id(&all);

    assert!(
        !run_dzc(&db, &["list", "10", "--pinned", "--json"]).contains("pin round trip"),
        "entry must not start pinned"
    );

    let pinned = run_dzc(&db, &["pin", &id]);
    assert_eq!(pinned.trim(), format!("pin: id={id}"));
    assert!(
        run_dzc(&db, &["list", "10", "--pinned", "--json"]).contains("pin round trip"),
        "after pin the entry must appear in the pinned list: {pinned}"
    );

    let unpinned = run_dzc(&db, &["unpin", &id]);
    assert_eq!(unpinned.trim(), format!("unpin: id={id}"));
    assert!(
        !run_dzc(&db, &["list", "10", "--pinned", "--json"]).contains("pin round trip"),
        "after unpin the entry must leave the pinned list"
    );

    let _ = std::fs::remove_file(db);
}

/// The pin SQL is `UPDATE ... WHERE id = ?`, which reports success for an id
/// that does not exist. The CLI has to refuse instead, or a typo in a script
/// silently leaves the entry unpinned.
#[test]
fn dzc_pin_refuses_an_id_that_does_not_exist() {
    let db = scratch_db("pin-missing");
    run_dzc(&db, &["add", "only entry", "--type", "text"]);

    let (code, out) = run_dzc_expecting_failure(&db, &["pin", "987654"]);
    assert_ne!(code, 0, "pinning a missing id must fail: {out}");
    assert!(
        out.contains("no entry with id=987654"),
        "error must name the missing id: {out}"
    );

    let _ = std::fs::remove_file(db);
}

#[test]
fn dzc_pin_rejects_a_non_numeric_id() {
    let db = scratch_db("pin-nan");
    let (code, out) = run_dzc_expecting_failure(&db, &["pin", "abc"]);
    assert_ne!(code, 0, "must fail: {out}");
    assert!(out.contains("invalid id"), "error must name the problem: {out}");
    let _ = std::fs::remove_file(db);
}

#[test]
fn dzc_pin_quiet_prints_nothing() {
    let db = scratch_db("pin-quiet");
    run_dzc(&db, &["add", "quiet pin", "--type", "text"]);
    let id = extract_first_id(&run_dzc(&db, &["list", "10", "--json"]));

    let out = run_dzc(&db, &["pin", &id, "--quiet"]);
    assert!(out.is_empty(), "--quiet must print nothing, got: {out:?}");
    // Still has to have taken effect.
    assert!(run_dzc(&db, &["list", "10", "--pinned", "--json"]).contains("quiet pin"));

    let _ = std::fs::remove_file(db);
}

#[test]
fn dzc_edit_replaces_content_and_keeps_the_entry() {
    let db = scratch_db("edit");
    run_dzc(&db, &["add", "before text", "--type", "text"]);
    let id = extract_first_id(&run_dzc(&db, &["list", "10", "--json"]));

    let out = run_dzc(&db, &["edit", &id, "after text"]);
    assert_eq!(out.trim(), format!("Edited: id={id}"));

    let fetched = run_dzc(&db, &["get", &id]);
    assert_eq!(fetched.trim(), "after text", "get must return the new content");
    assert!(
        !fetched.contains("before text"),
        "old content must be gone: {fetched}"
    );
    // Editing in place must not create a second row.
    assert_eq!(run_dzc(&db, &["count"]).trim(), "1");

    let _ = std::fs::remove_file(db);
}

#[test]
fn dzc_edit_reads_stdin_like_add() {
    let db = scratch_db("edit-stdin");
    run_dzc(&db, &["add", "original", "--type", "text"]);
    let id = extract_first_id(&run_dzc(&db, &["list", "10", "--json"]));

    let mut child = Command::new(env!("CARGO_BIN_EXE_dzc"))
        .env("DEZIRCLIP_DB_PATH", &db)
        .args(["edit", &id, "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn dzc edit -");
    {
        use std::io::Write;
        child
            .stdin
            .as_mut()
            .expect("stdin must be piped")
            .write_all(b"piped replacement")
            .expect("write stdin");
    }
    let output = child.wait_with_output().expect("wait for dzc edit -");

    assert!(output.status.success(), "edit from stdin must succeed");
    assert_eq!(
        run_dzc(&db, &["get", &id]).trim(),
        "piped replacement",
        "`-` must mean stdin for edit exactly as it does for add"
    );

    let _ = std::fs::remove_file(db);
}

#[test]
fn dzc_edit_refuses_a_missing_id_and_empty_content() {
    let db = scratch_db("edit-errors");
    run_dzc(&db, &["add", "keeper", "--type", "text"]);

    let (code, out) = run_dzc_expecting_failure(&db, &["edit", "424242", "x"]);
    assert_ne!(code, 0, "missing id must fail: {out}");
    assert!(out.contains("no entry with id=424242"), "error must name it: {out}");

    let id = extract_first_id(&run_dzc(&db, &["list", "10", "--json"]));
    let (code, out) = run_dzc_expecting_failure(&db, &["edit", &id, "@/definitely/not/here"]);
    assert_ne!(code, 0, "unreadable file source must fail: {out}");
    assert_eq!(
        run_dzc(&db, &["get", &id]).trim(),
        "keeper",
        "a failed edit must leave the entry untouched"
    );

    let _ = std::fs::remove_file(db);
}

#[test]
fn dzc_count_reports_the_entry_total() {
    let db = scratch_db("count");
    assert_eq!(run_dzc(&db, &["count"]).trim(), "0");

    run_dzc(&db, &["add", "one", "--type", "text"]);
    run_dzc(&db, &["add", "two", "--type", "text"]);
    assert_eq!(run_dzc(&db, &["count"]).trim(), "2");
    assert_eq!(
        run_dzc(&db, &["count", "--json"]).trim(),
        "{\"count\":2}"
    );

    let _ = std::fs::remove_file(db);
}

#[test]
fn dzc_clear_requires_yes_and_keeps_protected_entries() {
    let db = scratch_db("clear");

    run_dzc(&db, &["add", "throwaway", "--type", "text"]);
    run_dzc(&db, &["add", "keeper", "--type", "text"]);
    let keeper = extract_id_after(&run_dzc(&db, &["list", "10", "--json"]), "keeper");
    run_dzc(&db, &["pin", &keeper]);
    let (code, out) = run_dzc_expecting_failure(&db, &["clear"]);
    assert_ne!(code, 0, "clear without --yes must fail: {out}");
    assert!(out.contains("--yes"), "error must mention --yes: {out}");
    assert_eq!(run_dzc(&db, &["count"]).trim(), "2", "refused clear must change nothing");

    let out = run_dzc(&db, &["clear", "--yes"]);
    assert_eq!(out.trim(), "Cleared 1 entries", "only the unpinned entry goes");
    assert_eq!(
        run_dzc(&db, &["count"]).trim(),
        "1",
        "the pinned entry must survive"
    );
    assert!(run_dzc(&db, &["get", &keeper]).contains("keeper"));

    let _ = std::fs::remove_file(db);
}

/// Pull the first `"id": N` out of a `--json` listing.
fn extract_first_id(json: &str) -> String {
    read_id_at(json, json.find("\"id\":").expect("listing has no id"))
}

/// Pull the id of the entry whose content contains `needle`.
///
/// `id` precedes `content` in each `CliEntry` object, so this is the *last*
/// id before the needle — scanning forward from it would return the next
/// entry's id instead and quietly assert against the wrong row.
fn extract_id_after(json: &str, needle: &str) -> String {
    let end = json
        .find(needle)
        .unwrap_or_else(|| panic!("no {needle:?} in listing: {json}"));
    let head = &json[..end];
    let key = head
        .rfind("\"id\":")
        .unwrap_or_else(|| panic!("no id before {needle:?} in: {head}"));
    read_id_at(json, key)
}

/// Read the integer that begins at `key`, which must point at `"id":`.
fn read_id_at(json: &str, key: usize) -> String {
    let rest = &json[key + 5..];
    let digits: String = rest
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '-')
        .collect();
    assert!(!digits.is_empty(), "no numeric id in: {rest}");
    digits
}

/// The help text promises `list -1` returns every row, but clap reads a bare
/// `-1` as a flag unless the subcommand opts into negative numbers.
#[test]
fn dzc_list_minus_one_returns_every_row() {
    let db = scratch_db("list-all");
    for i in 1..=25 {
        run_dzc(&db, &["add", &format!("row {i}"), "--type", "text"]);
    }
    let defaulted = run_dzc(&db, &["list", "--ids"]);
    assert_eq!(
        defaulted.lines().count(),
        20,
        "default limit is 20 rows, which is what makes the -1 case meaningful"
    );

    let all = run_dzc(&db, &["list", "-1", "--ids"]);
    assert_eq!(
        all.lines().count(),
        25,
        "-1 must return every row, got {}",
        all.lines().count()
    );

    let _ = std::fs::remove_file(db);
}
