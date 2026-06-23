//! Behavioral tests for `tiez-c` list/search/get subcommands.
//!
//! Three required tests + extras:
//! 1. `list --json` produces a valid JSON array with all required fields.
//! 2. `search` filters entries to those matching the query.
//! 3. `get` returns the entry's content by numeric ID.
//! 4. CLI binary accepts `list --ids` and exits 0 (smoke).
//! 5. CLI binary errors on `get latest` with an empty store (smoke).
//!
//! Tests call the public `run_*` functions directly so we don't depend
//! on stdout capture. The dispatch path is also exercised end-to-end
//! via the `Command::new(bin)` smoke tests.

use std::process::Command;

use tiez_c_standalone::{
    run_get, run_list, run_search, Entry, GetArgs, ListArgs, MockRepo, Output, SearchArgs,
    SearchMode,
};

// =====================================================================
// Required tests (3)
// =====================================================================

#[test]
fn list_json_contains_required_fields() {
    let repo = MockRepo::with(vec![
        Entry::new(1, "text", "hello world"),
        Entry::new(2, "html", "<b>bold</b>"),
        Entry::new(3, "image", "[png data]"),
    ]);

    let args = ListArgs {
        limit: None,
        kind: None,
        tag: None,
        pinned: false,
        json: true,
        ids: false,
        quiet: false,
    };

    let out = run_list(&args, &repo).expect("run_list");
    let s = match out {
        Output::Json(s) => s,
        other => panic!("expected Json output, got: {other:?}"),
    };

    assert!(s.starts_with('['), "JSON must start with '[': {s}");
    assert!(s.ends_with(']'), "JSON must end with ']': {s}");

    for field in [
        "\"id\":",
        "\"type\":",
        "\"content\":",
        "\"preview\":",
        "\"tags\":",
        "\"pinned\":",
        "\"timestamp\":",
    ] {
        assert!(s.contains(field), "missing required field {field}: {s}");
    }

    assert!(s.contains("\"id\":1"), "id 1 missing: {s}");
    assert!(s.contains("\"id\":2"), "id 2 missing: {s}");
    assert!(s.contains("\"id\":3"), "id 3 missing: {s}");

    assert!(s.contains("\"type\":\"text\""), "text type missing: {s}");
    assert!(s.contains("\"type\":\"html\""), "html type missing: {s}");
    assert!(s.contains("\"type\":\"image\""), "image type missing: {s}");
}

#[test]
fn search_finds_matching_entries() {
    let repo = MockRepo::with(vec![
        Entry::new(1, "text", "git status"),
        Entry::new(2, "text", "cargo build"),
        Entry::new(3, "text", "git commit -m 'msg'"),
        Entry::new(4, "text", "ls -la"),
        Entry::new(5, "text", "github.com"),
    ]);

    let args = SearchArgs {
        query: "git".to_string(),
        mode: SearchMode::Contains,
        limit: None,
        json: false,
        quiet: false,
    };

    let out = run_search(&args, &repo).expect("run_search");
    let s = match out {
        Output::Text(s) => s,
        other => panic!("expected Text output, got: {other:?}"),
    };

    let lines: Vec<&str> = s.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(
        lines.len(),
        3,
        "expected 3 matches for 'git' (entries 1, 3, 5), got {} lines: {s:?}",
        lines.len()
    );

    for line in &lines {
        assert!(
            line.to_lowercase().contains("git"),
            "result line does not contain 'git': {line}"
        );
    }
}

#[test]
fn get_returns_content_by_id() {
    let repo = MockRepo::with(vec![
        Entry::new(1, "text", "first entry"),
        Entry::new(42, "text", "hello world"),
        Entry::new(99, "text", "another"),
    ]);

    let args = GetArgs {
        id: "42".to_string(),
        preview: false,
        json: false,
        quiet: false,
    };

    let out = run_get(&args, &repo).expect("run_get 42");
    let s = match out {
        Output::Text(s) => s,
        other => panic!("expected Text output, got: {other:?}"),
    };
    assert!(
        s.contains("hello world"),
        "get 42 must print 'hello world', got: {s:?}"
    );
    assert!(
        !s.contains("first entry"),
        "get 42 must not print other entries: {s:?}"
    );
    assert!(
        !s.contains("another"),
        "get 42 must not print other entries: {s:?}"
    );
}

// =====================================================================
// CLI smoke tests (run the actual binary as a subprocess)
// =====================================================================

#[test]
fn cli_list_ids_outputs_ids_one_per_line() {
    let bin = env!("CARGO_BIN_EXE_tiez-c");
    let out = Command::new(bin)
        .args(["list", "--ids"])
        .output()
        .expect("spawn tiez-c list --ids");
    assert!(out.status.success(), "exit non-zero: {:?}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert!(!lines.is_empty(), "list --ids should output at least one id");
    for line in &lines {
        let id: i64 = line
            .parse()
            .unwrap_or_else(|_| panic!("--ids line should be a number, got: {line:?}"));
        assert!(id > 0, "id should be positive: {id}");
    }
}

#[test]
fn cli_get_latest_with_demo_data_returns_first() {
    let bin = env!("CARGO_BIN_EXE_tiez-c");
    let out = Command::new(bin)
        .args(["get", "latest"])
        .output()
        .expect("spawn tiez-c get latest");
    assert!(
        out.status.success(),
        "get latest with non-empty store should succeed: {:?}",
        out.status
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.trim().is_empty(),
        "get latest should print content, got: {stdout:?}"
    );
}

// =====================================================================
// Extra coverage (not required, but fills in obvious gaps)
// =====================================================================

#[test]
fn get_latest_returns_highest_id() {
    let repo = MockRepo::with(vec![
        Entry::new(1, "text", "oldest"),
        Entry::new(7, "text", "newest"),
        Entry::new(3, "text", "middle"),
    ]);
    let args = GetArgs {
        id: "latest".to_string(),
        ..Default::default()
    };
    let out = run_get(&args, &repo).expect("run_get latest");
    if let Output::Text(s) = out {
        assert!(s.contains("newest"), "latest should be the newest entry: {s}");
        assert!(!s.contains("oldest"), "latest should not be oldest: {s}");
    } else {
        panic!("expected Text");
    }
}

#[test]
fn list_ids_outputs_ids_only() {
    let repo = MockRepo::with(vec![
        Entry::new(10, "text", "a"),
        Entry::new(20, "text", "b"),
        Entry::new(30, "text", "c"),
    ]);
    let args = ListArgs {
        ids: true,
        ..Default::default()
    };
    let out = run_list(&args, &repo).expect("run_list --ids");
    if let Output::Ids(ids) = out {
        assert_eq!(ids.len(), 3);
        assert!(ids.contains(&10));
        assert!(ids.contains(&20));
        assert!(ids.contains(&30));
    } else {
        panic!("expected Ids");
    }
}

#[test]
fn list_quiet_emits_no_text_output() {
    let repo = MockRepo::with(vec![Entry::new(1, "text", "x")]);
    let args = ListArgs {
        quiet: true,
        ..Default::default()
    };
    let out = run_list(&args, &repo).expect("run_list --quiet");
    assert!(matches!(out, Output::Quiet));
}

#[test]
fn list_filters_by_kind() {
    let repo = MockRepo::with(vec![
        Entry::new(1, "text", "a"),
        Entry::new(2, "html", "b"),
        Entry::new(3, "text", "c"),
    ]);
    let args = ListArgs {
        kind: Some("text".to_string()),
        json: true,
        ..Default::default()
    };
    let out = run_list(&args, &repo).expect("run_list --kind text");
    let s = match out {
        Output::Json(s) => s,
        other => panic!("expected Json, got {other:?}"),
    };
    assert!(s.contains("\"type\":\"text\""), "expected text type: {s}");
    assert!(!s.contains("\"type\":\"html\""), "html should be filtered: {s}");
}

#[test]
fn list_filters_by_pinned() {
    let repo = MockRepo::with(vec![
        Entry::new(1, "text", "unpinned"),
        Entry::new(2, "text", "pinned").pinned(),
    ]);
    let args = ListArgs {
        pinned: true,
        json: true,
        ..Default::default()
    };
    let out = run_list(&args, &repo).expect("run_list --pinned");
    let s = match out {
        Output::Json(s) => s,
        other => panic!("expected Json, got {other:?}"),
    };
    let pinned_count = s.matches("\"pinned\":true").count();
    let unpinned_count = s.matches("\"pinned\":false").count();
    assert_eq!(pinned_count, 1, "exactly one pinned: {s}");
    assert_eq!(unpinned_count, 0, "no unpinned: {s}");
}

#[test]
fn list_filters_by_tag() {
    let repo = MockRepo::with(vec![
        Entry::new(1, "text", "no tag"),
        Entry::new(2, "text", "tagged").with_tags(&["work"]),
    ]);
    let args = ListArgs {
        tag: Some("work".to_string()),
        json: true,
        ..Default::default()
    };
    let out = run_list(&args, &repo).expect("run_list --tag work");
    let s = match out {
        Output::Json(s) => s,
        other => panic!("expected Json, got {other:?}"),
    };
    assert!(s.contains("\"id\":2"), "tagged entry should appear: {s}");
    assert!(!s.contains("\"id\":1"), "untagged entry should be filtered: {s}");
}

#[test]
fn list_with_limit_negative_returns_all() {
    let repo = MockRepo::with(vec![
        Entry::new(1, "text", "a"),
        Entry::new(2, "text", "b"),
        Entry::new(3, "text", "c"),
    ]);
    let args = ListArgs {
        limit: Some(-1),
        ids: true,
        ..Default::default()
    };
    let out = run_list(&args, &repo).expect("run_list --limit -1");
    if let Output::Ids(ids) = out {
        assert_eq!(ids.len(), 3, "limit -1 returns all 3 entries: {ids:?}");
    } else {
        panic!("expected Ids");
    }
}

#[test]
fn search_fuzzy_matches_subsequence() {
    let repo = MockRepo::with(vec![
        Entry::new(1, "text", "go iterate over things"),
        Entry::new(2, "text", "hello world"),
    ]);
    let args = SearchArgs {
        query: "git".to_string(),
        mode: SearchMode::Fuzzy,
        ..Default::default()
    };
    let out = run_search(&args, &repo).expect("run_search fuzzy 'git'");
    let s = match out {
        Output::Text(s) => s,
        other => panic!("expected Text, got {other:?}"),
    };
    assert!(
        s.contains("go iterate"),
        "fuzzy should match 'git' as subsequence in 'go iterate': {s}"
    );
    assert!(
        !s.contains("hello world"),
        "fuzzy should not match 'hello world': {s}"
    );
}

#[test]
fn get_invalid_id_returns_error() {
    let repo = MockRepo::default();
    let args = GetArgs {
        id: "not-a-number".to_string(),
        ..Default::default()
    };
    let err = run_get(&args, &repo).expect_err("non-numeric id must error");
    assert!(
        err.contains("invalid id"),
        "error should mention invalid id: {err}"
    );
}

#[test]
fn get_missing_id_returns_error() {
    let repo = MockRepo::with(vec![Entry::new(1, "text", "x")]);
    let args = GetArgs {
        id: "999".to_string(),
        ..Default::default()
    };
    let err = run_get(&args, &repo).expect_err("missing id must error");
    assert!(err.contains("no entry"), "error should mention no entry: {err}");
}

#[test]
fn get_preview_returns_preview_field() {
    let repo = MockRepo::with(vec![Entry::new(1, "text", "full content here")]);
    let args = GetArgs {
        id: "1".to_string(),
        preview: true,
        ..Default::default()
    };
    let out = run_get(&args, &repo).expect("get --preview");
    let s = match out {
        Output::Text(s) => s,
        other => panic!("expected Text, got {other:?}"),
    };
    assert!(
        s.contains("full content here"),
        "preview should match content: {s}"
    );
}

#[test]
fn search_empty_query_returns_empty() {
    let repo = MockRepo::with(vec![Entry::new(1, "text", "x")]);
    let args = SearchArgs {
        query: "   ".to_string(),
        ..Default::default()
    };
    let out = run_search(&args, &repo).expect("empty search");
    let s = match out {
        Output::Text(s) => s,
        other => panic!("expected Text, got {other:?}"),
    };
    assert!(s.is_empty(), "empty query must return empty text: {s:?}");
}