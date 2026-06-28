//! Behavioral tests for `dzc` export/import/stats/watch subcommands.
//!
//! Four required tests + extras:
//! 1. `export → import` round-trip preserves entry count.
//! 2. `stats` reports correct total in human-readable output.
//! 3. `stats --json` includes the `total` and `by_type` keys.
//! 4. `watch` outputs new entries as they appear and exits on signal.
//! 5-7. Extra coverage: encrypted export errors without passphrase,
//!    import --mode replace clears before inserting, watch --pattern
//!    filters output, watch default interval is 500ms (G32).
//!
//! The test repo is `Arc<Mutex<MockRepo>>` everywhere — the watch
//! path locks-and-snapshots on every tick, so the same handle has to
//! be sharable from the test thread.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use dzc_standalone::{
    run_add, run_export, run_import, run_stats, run_watch, AddArgs, Entry, ExportArgs,
    ImportArgs, MockRepo, StatsArgs, WatchArgs,
};

// =====================================================================
// Helpers
// =====================================================================

/// Build a fresh `MockRepo` with 5 text entries. Mirrors the in-tree
/// demo data length so the roundtrip test matches the in-tree
/// verification harness shape.
fn setup_repo_with_5_entries() -> MockRepo {
    MockRepo::with_n(5)
}

// =====================================================================
// Required tests (4)
// =====================================================================

#[test]
fn test_export_import_roundtrip() {
    let repo = setup_repo_with_5_entries();
    let tmp = std::env::temp_dir().join("dezirclip-test-export.json");
    let path = tmp.to_string_lossy().to_string();

    run_export(
        &ExportArgs {
            path: path.clone(),
            encrypted: false,
            passphrase: None,
            quiet: true,
        },
        &repo,
    )
    .expect("export must succeed");

    let mut new_repo = MockRepo::default();
    run_import(
        &ImportArgs {
            path: path.clone(),
            mode: "merge".to_string(),
            passphrase: None,
            quiet: true,
        },
        &mut new_repo,
    )
    .expect("import must succeed");

    assert_eq!(
        new_repo.iter().len(),
        5,
        "roundtrip should preserve all 5 entries (path={path})"
    );

    // Cleanup — the tmp file isn't auto-removed. The next test run
    // would overwrite, so the leak is bounded to one file.
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn test_stats_reports_total() {
    let repo = setup_repo_with_5_entries();
    let stats = run_stats(
        &StatsArgs {
            top: Some(3),
            json: false,
        },
        &repo,
    );
    assert!(
        stats.contains("Total: 5") || stats.contains("total: 5"),
        "stats human output should report total=5; got: {stats:?}"
    );
}

#[test]
fn test_stats_json_has_required_fields() {
    let repo = setup_repo_with_5_entries();
    let stats = run_stats(
        &StatsArgs {
            top: Some(3),
            json: true,
        },
        &repo,
    );
    assert!(
        stats.contains("\"total\":5"),
        "stats JSON must contain \"total\":5; got: {stats}"
    );
    assert!(
        stats.contains("\"by_type\""),
        "stats JSON must contain \"by_type\"; got: {stats}"
    );
    assert!(
        stats.contains("\"most_used\""),
        "stats JSON must contain \"most_used\"; got: {stats}"
    );
}

#[test]
fn test_watch_outputs_new_entries() {
    let repo = Arc::new(Mutex::new(MockRepo::default()));
    let last_ts = Arc::new(Mutex::new(0i64));
    let running = Arc::new(AtomicBool::new(true));

    // Add an entry via the lib's run_add.
    {
        let mut r = repo.lock().expect("poisoned");
        run_add(
            &AddArgs {
                text: Some("watched entry".into()),
                stdin: false,
                file: None,
                kind: None,
            },
            &mut r,
        )
        .expect("run_add must succeed");
    }

    // Spawn a thread that flips `running` after 100ms — the watch
    // loop should observe it on the next tick and exit.
    let running_clone = running.clone();
    let _ = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(100));
        running_clone.store(false, Ordering::Release);
    });

    let output = run_watch(
        &WatchArgs {
            pattern: None,
            interval_ms: Some(50),
            json: false,
            quiet: true,
        },
        &repo,
        last_ts.clone(),
        running,
    );

    assert!(
        output.contains("watched entry"),
        "watch output should contain the new entry; got: {output:?}"
    );
}

// =====================================================================
// Extras (not required, fill obvious gaps)
// =====================================================================

#[test]
fn export_without_passphrase_errors_for_encrypted() {
    let repo = setup_repo_with_5_entries();
    let tmp = std::env::temp_dir().join("dezirclip-test-no-pass.dzc");
    let res = run_export(
        &ExportArgs {
            path: tmp.to_string_lossy().to_string(),
            encrypted: true,
            passphrase: None,
            quiet: true,
        },
        &repo,
    );
    let err = res.expect_err("export --encrypted without passphrase must error");
    assert!(
        err.contains("passphrase required"),
        "error should mention passphrase required; got: {err}"
    );
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn export_with_short_passphrase_errors() {
    let repo = setup_repo_with_5_entries();
    let tmp = std::env::temp_dir().join("dezirclip-test-short-pw.dzc");
    let res = run_export(
        &ExportArgs {
            path: tmp.to_string_lossy().to_string(),
            encrypted: true,
            passphrase: Some("short".to_string()),
            quiet: true,
        },
        &repo,
    );
    let err = res.expect_err("export with 5-char passphrase must error");
    assert!(
        err.contains("at least 12"),
        "error should mention 12-char minimum; got: {err}"
    );
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn export_with_long_passphrase_succeeds() {
    let repo = setup_repo_with_5_entries();
    let tmp = std::env::temp_dir().join("dezirclip-test-long-pw.dzc");
    run_export(
        &ExportArgs {
            path: tmp.to_string_lossy().to_string(),
            encrypted: true,
            passphrase: Some("long-passphrase-12".to_string()),
            quiet: true,
        },
        &repo,
    )
    .expect("export with 12-char passphrase must succeed");
    assert!(tmp.exists(), "encrypted file should exist on disk");
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn import_replace_mode_clears_existing_entries() {
    // Pre-populate a repo with 3 entries.
    let mut repo = MockRepo::with_n(3);
    assert_eq!(repo.iter().len(), 3);

    // Export 5 from a fresh repo, then import with --mode replace.
    let src = setup_repo_with_5_entries();
    let tmp = std::env::temp_dir().join("dezirclip-test-replace.json");
    let path = tmp.to_string_lossy().to_string();
    run_export(
        &ExportArgs {
            path: path.clone(),
            encrypted: false,
            passphrase: None,
            quiet: true,
        },
        &src,
    )
    .expect("export must succeed");

    run_import(
        &ImportArgs {
            path,
            mode: "replace".to_string(),
            passphrase: None,
            quiet: true,
        },
        &mut repo,
    )
    .expect("import must succeed");

    // After replace, the repo should have 5 entries (the original 3
    // cleared, the 5 from the file inserted).
    assert_eq!(
        repo.iter().len(),
        5,
        "replace mode should leave exactly 5 entries (3 cleared + 5 inserted)"
    );
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn import_invalid_mode_errors() {
    let mut repo = MockRepo::default();
    let err = run_import(
        &ImportArgs {
            path: "/tmp/nonexistent.json".to_string(),
            mode: "delete".to_string(),
            passphrase: None,
            quiet: true,
        },
        &mut repo,
    )
    .expect_err("invalid mode must error");
    assert!(
        err.contains("invalid mode"),
        "error should mention invalid mode; got: {err}"
    );
    assert!(err.contains("delete"), "error should mention the bad value; got: {err}");
}

#[test]
fn watch_default_interval_is_500ms() {
    // G32 hard constraint: the default interval for `dzc watch`
    // is 500ms. Verify by passing `interval_ms: None` and confirming
    // the startup banner mentions 500ms.
    let repo = Arc::new(Mutex::new(MockRepo::default()));
    let last_ts = Arc::new(Mutex::new(0i64));
    let running = Arc::new(AtomicBool::new(true));

    let running_clone = running.clone();
    let _ = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(50));
        running_clone.store(false, Ordering::Release);
    });

    let output = run_watch(
        &WatchArgs {
            pattern: None,
            interval_ms: None, // ← default → 500
            json: false,
            quiet: false, // we want the banner
        },
        &repo,
        last_ts,
        running,
    );
    assert!(
        output.contains("interval=500ms"),
        "default interval should be 500ms (G32); got: {output:?}"
    );
}

#[test]
fn watch_pattern_filters_output() {
    // Pre-seed: 2 entries, one containing "needle", one without.
    let repo = Arc::new(Mutex::new(MockRepo::with(vec![
        Entry::new(1, "text", "this is a needle match"),
        Entry::new(2, "text", "this is just hay"),
    ])));
    let last_ts = Arc::new(Mutex::new(0i64));
    let running = Arc::new(AtomicBool::new(true));

    let running_clone = running.clone();
    let _ = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(80));
        running_clone.store(false, Ordering::Release);
    });

    let output = run_watch(
        &WatchArgs {
            pattern: Some("needle".to_string()),
            interval_ms: Some(20),
            json: false,
            quiet: true,
        },
        &repo,
        last_ts,
        running,
    );

    assert!(
        output.contains("needle"),
        "filtered output should include 'needle' entry; got: {output:?}"
    );
    assert!(
        !output.contains("hay"),
        "filtered output should exclude 'hay' entry; got: {output:?}"
    );
}

#[test]
fn watch_quiet_suppresses_banner_but_prints_entries() {
    let repo = Arc::new(Mutex::new(MockRepo::with(vec![Entry::new(
        1, "text", "visible entry",
    )])));
    let last_ts = Arc::new(Mutex::new(0i64));
    let running = Arc::new(AtomicBool::new(true));

    let running_clone = running.clone();
    let _ = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(60));
        running_clone.store(false, Ordering::Release);
    });

    let output = run_watch(
        &WatchArgs {
            pattern: None,
            interval_ms: Some(20),
            json: false,
            quiet: true,
        },
        &repo,
        last_ts,
        running,
    );

    assert!(
        !output.contains("Watching for new entries"),
        "quiet mode should suppress startup banner; got: {output:?}"
    );
    assert!(
        output.contains("visible entry"),
        "quiet mode should still print the entry; got: {output:?}"
    );
    assert!(
        !output.contains("Stopped watching"),
        "quiet mode should suppress exit banner too; got: {output:?}"
    );
}

#[test]
fn watch_json_emits_ndjson_per_entry() {
    let repo = Arc::new(Mutex::new(MockRepo::with(vec![Entry::new(
        42, "text", "json me",
    )])));
    let last_ts = Arc::new(Mutex::new(0i64));
    let running = Arc::new(AtomicBool::new(true));

    let running_clone = running.clone();
    let _ = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(60));
        running_clone.store(false, Ordering::Release);
    });

    let output = run_watch(
        &WatchArgs {
            pattern: None,
            interval_ms: Some(20),
            json: true,
            quiet: true,
        },
        &repo,
        last_ts,
        running,
    );

    assert!(
        output.contains("\"id\":42"),
        "json mode should include id=42; got: {output:?}"
    );
    assert!(
        output.contains("\"content\":\"json me\""),
        "json mode should include escaped content; got: {output:?}"
    );
}
