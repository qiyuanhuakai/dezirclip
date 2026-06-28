//! Behavioral tests for `dzc` add/delete/tag subcommands.
//!
//! Three required tests + extras covering the `add` / `delete` / `tag`
//! paths end-to-end against the in-memory `MockRepo`.
//!
//! The tests call the public `run_*` functions directly so we don't
//! depend on stdout capture. The CLI binary path is also exercised
//! for `add` via the stdin/file modes (see the `_smoke_*` tests at
//! the bottom).
//!
//! Coverage:
//! 1. `add` text then `search` finds it
//! 2. `add` then `delete` then `search` returns empty
//! 3. `tag add` then `tag list` shows the tag
//! 4. extra: `add` rejects empty content
//! 5. extra: `add` honors `--type` override
//! 6. extra: `add` from file reads the file contents
//! 7. extra: `delete` without `--yes` returns error
//! 8. extra: `delete` with `--yes` on missing id is a no-op
//! 9. extra: `tag remove` drops the tag
//! 10. extra: `tag list` on missing id returns error
//! 11. extra: `tag add` is idempotent

use std::io::Write;
use std::process::{Command, Stdio};

use dzc_standalone::{
    run_add, run_delete, run_search, run_tag, AddArgs, DeleteArgs, MockRepo, Output, SearchArgs,
    SearchMode, TagArgs, TagCommand,
};

// =====================================================================
// Required tests (3)
// =====================================================================

#[test]
fn test_add_text_then_search_finds_it() {
    let mut repo = MockRepo::default();
    let args = AddArgs {
        text: Some("git clone https://...".into()),
        stdin: false,
        file: None,
        kind: None,
    };
    run_add(&args, &mut repo).unwrap();

    let search_args = SearchArgs {
        query: "git".into(),
        mode: SearchMode::Contains,
        limit: None,
        json: false,
        quiet: false,
    };
    let out = run_search(&search_args, &repo).unwrap();
    let s = match out {
        Output::Text(s) => s,
        other => panic!("expected Text, got: {other:?}"),
    };
    let lines: Vec<&str> = s.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 1, "expected 1 match, got {}: {s:?}", lines.len());
    assert!(
        lines[0].contains("git"),
        "result must contain 'git': {}",
        lines[0]
    );
}

#[test]
fn test_add_then_delete_then_search_empty() {
    let mut repo = MockRepo::default();
    let args = AddArgs {
        text: Some("temporary entry".into()),
        stdin: false,
        file: None,
        kind: None,
    };
    run_add(&args, &mut repo).unwrap();
    let new_id = repo.iter().first().map(|e| e.id).expect("entry present");
    assert_eq!(new_id, 1, "first saved id must be 1");

    let del_args = DeleteArgs { id: new_id, yes: true };
    run_delete(&del_args, &mut repo).unwrap();

    let search_args = SearchArgs {
        query: "temporary".into(),
        mode: SearchMode::Contains,
        limit: None,
        json: false,
        quiet: false,
    };
    let out = run_search(&search_args, &repo).unwrap();
    let s = match out {
        Output::Text(s) => s,
        other => panic!("expected Text, got: {other:?}"),
    };
    assert!(s.trim().is_empty(), "search must be empty, got: {s:?}");
}

#[test]
fn test_tag_add_then_list_shows_tag() {
    let mut repo = MockRepo::default();
    let add_args = AddArgs {
        text: Some("entry to tag".into()),
        stdin: false,
        file: None,
        kind: None,
    };
    run_add(&add_args, &mut repo).unwrap();
    let id = repo.iter().first().map(|e| e.id).expect("entry present");

    let tag_args = TagArgs {
        command: TagCommand::Add(id, "work".to_string()),
    };
    run_tag(&tag_args, &mut repo).unwrap();

    let list_args = TagArgs {
        command: TagCommand::List(id),
    };
    let out = run_tag(&list_args, &mut repo).unwrap();
    let s = match out {
        Output::Text(s) => s,
        other => panic!("expected Text, got: {other:?}"),
    };
    assert!(
        s.contains("work"),
        "list output must contain 'work', got: {s:?}"
    );
}

// =====================================================================
// Extra behavioral tests
// =====================================================================

#[test]
fn test_add_rejects_empty_content() {
    let mut repo = MockRepo::default();
    let args = AddArgs {
        text: Some(String::new()),
        stdin: false,
        file: None,
        kind: None,
    };
    let err = run_add(&args, &mut repo).expect_err("empty must error");
    assert!(
        err.contains("empty"),
        "error must mention 'empty': {err}"
    );
}

#[test]
fn test_add_kind_override() {
    let mut repo = MockRepo::default();
    let args = AddArgs {
        text: Some("<b>hi</b>".into()),
        stdin: false,
        file: None,
        kind: Some("html".into()),
    };
    run_add(&args, &mut repo).unwrap();
    let entries = repo.iter();
    let e = entries.first().expect("entry present");
    assert_eq!(e.content_type, "html", "--type html must override");
}

#[test]
fn test_add_kind_alias_normalization() {
    let mut repo = MockRepo::default();
    // `PNG` is an alias for `image` per `normalize_add_kind`.
    let args = AddArgs {
        text: Some("data:image/png;base64,AAAA".into()),
        stdin: false,
        file: None,
        kind: Some("PNG".into()),
    };
    run_add(&args, &mut repo).unwrap();
    let entries = repo.iter();
    let e = entries.first().expect("entry present");
    assert_eq!(e.content_type, "image", "PNG alias must map to image");
}

#[test]
fn test_add_from_file() {
    let mut repo = MockRepo::default();
    let dir = std::env::temp_dir();
    let path = dir.join("dezirclip_c_add_delete_tag_test.txt");
    {
        let mut f = std::fs::File::create(&path).expect("create file");
        f.write_all(b"contents from file").expect("write");
    }
    let args = AddArgs {
        text: None,
        stdin: false,
        file: Some(path.to_string_lossy().to_string()),
        kind: None,
    };
    run_add(&args, &mut repo).unwrap();
    let entries = repo.iter();
    let e = entries.first().expect("entry present");
    assert_eq!(e.content, "contents from file");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn test_delete_without_yes_is_error() {
    let mut repo = MockRepo::default();
    let add_args = AddArgs {
        text: Some("x".into()),
        stdin: false,
        file: None,
        kind: None,
    };
    run_add(&add_args, &mut repo).unwrap();
    let id = repo.iter().first().map(|e| e.id).expect("entry");
    let del_args = DeleteArgs { id, yes: false };
    let err = run_delete(&del_args, &mut repo).expect_err("must refuse");
    assert!(err.contains("--yes"), "error must mention --yes: {err}");
    // Entry should still be present
    assert_eq!(repo.iter().len(), 1, "entry must survive refused delete");
}

#[test]
fn test_delete_yes_on_missing_id_is_noop() {
    let mut repo = MockRepo::default();
    let del_args = DeleteArgs { id: 999, yes: true };
    run_delete(&del_args, &mut repo).expect("missing id delete is a no-op");
    assert!(repo.iter().is_empty());
}

#[test]
fn test_tag_remove_drops_tag() {
    let mut repo = MockRepo::default();
    let add_args = AddArgs {
        text: Some("x".into()),
        stdin: false,
        file: None,
        kind: None,
    };
    run_add(&add_args, &mut repo).unwrap();
    let id = repo.iter().first().map(|e| e.id).expect("entry");

    run_tag(
        &TagArgs {
            command: TagCommand::Add(id, "alpha".into()),
        },
        &mut repo,
    )
    .unwrap();
    run_tag(
        &TagArgs {
            command: TagCommand::Add(id, "beta".into()),
        },
        &mut repo,
    )
    .unwrap();
    run_tag(
        &TagArgs {
            command: TagCommand::Remove(id, "alpha".into()),
        },
        &mut repo,
    )
    .unwrap();

    let out = run_tag(
        &TagArgs {
            command: TagCommand::List(id),
        },
        &mut repo,
    )
    .unwrap();
    let s = match out {
        Output::Text(s) => s,
        other => panic!("expected Text, got: {other:?}"),
    };
    assert!(!s.contains("alpha"), "alpha must be gone: {s}");
    assert!(s.contains("beta"), "beta must remain: {s}");
}

#[test]
fn test_tag_list_on_missing_id_errors() {
    let mut repo = MockRepo::default();
    let err = run_tag(
        &TagArgs {
            command: TagCommand::List(999),
        },
        &mut repo,
    )
    .expect_err("missing id must error");
    assert!(
        err.contains("no entry") || err.contains("999"),
        "error must mention missing id: {err}"
    );
}

#[test]
fn test_tag_add_is_idempotent() {
    let mut repo = MockRepo::default();
    let add_args = AddArgs {
        text: Some("x".into()),
        stdin: false,
        file: None,
        kind: None,
    };
    run_add(&add_args, &mut repo).unwrap();
    let id = repo.iter().first().map(|e| e.id).expect("entry");

    run_tag(
        &TagArgs {
            command: TagCommand::Add(id, "dup".into()),
        },
        &mut repo,
    )
    .unwrap();
    run_tag(
        &TagArgs {
            command: TagCommand::Add(id, "dup".into()),
        },
        &mut repo,
    )
    .unwrap();

    let out = run_tag(
        &TagArgs {
            command: TagCommand::List(id),
        },
        &mut repo,
    )
    .unwrap();
    let s = match out {
        Output::Text(s) => s,
        other => panic!("expected Text, got: {other:?}"),
    };
    let count = s.matches("dup").count();
    assert_eq!(count, 1, "dup tag must appear once, got: {s}");
}

// =====================================================================
// CLI binary smoke tests (run the actual binary as a subprocess)
// =====================================================================

#[test]
fn cli_add_text_succeeds() {
    let bin = env!("CARGO_BIN_EXE_dzc");
    let out = Command::new(bin)
        .args(["add", "hello from cli"])
        .output()
        .expect("spawn dzc add");
    assert!(out.status.success(), "exit non-zero: {:?}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Added: id="),
        "stdout should contain 'Added: id=', got: {stdout:?}"
    );
}

#[test]
fn cli_add_from_stdin_succeeds() {
    let bin = env!("CARGO_BIN_EXE_dzc");
    let mut child = Command::new(bin)
        .args(["add", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn dzc add -");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"hello from stdin\n")
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait");
    assert!(out.status.success(), "exit non-zero: {:?}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Added: id="),
        "stdout should contain 'Added: id=', got: {stdout:?}"
    );
}

#[test]
fn cli_add_from_file_succeeds() {
    let dir = std::env::temp_dir();
    let path = dir.join("dzc_cli_add_file.txt");
    std::fs::write(&path, "file payload\n").expect("write");
    let bin = env!("CARGO_BIN_EXE_dzc");
    let out = Command::new(bin)
        .args([
            "add",
            &format!("@{}", path.to_string_lossy()),
        ])
        .output()
        .expect("spawn dzc add @file");
    let _ = std::fs::remove_file(&path);
    assert!(out.status.success(), "exit non-zero: {:?}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Added: id="),
        "stdout should contain 'Added: id=', got: {stdout:?}"
    );
}

#[test]
fn cli_delete_without_yes_fails() {
    let bin = env!("CARGO_BIN_EXE_dzc");
    let out = Command::new(bin)
        .args(["delete", "999"])
        .output()
        .expect("spawn dzc delete");
    assert!(!out.status.success(), "exit must be non-zero: {:?}", out.status);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--yes"),
        "stderr should mention --yes, got: {stderr:?}"
    );
}

#[test]
fn cli_tag_add_and_list_smoke() {
    // The bin starts a fresh `MockRepo::with(demo_entries())` on every
    // invocation, so each command below runs against a clean slate.
    // Cross-invocation state would need an on-disk store; the
    // standalone crate is in-memory only by design. The lib-level
    // tests above (`test_tag_add_then_list_shows_tag` etc.) cover the
    // state-mutation paths end-to-end; these CLI smoke tests just
    // confirm each subcommand parses and dispatches without error
    // on the demo seed (id=1 is the first demo entry).
    let bin = env!("CARGO_BIN_EXE_dzc");

    // tag add 1 work
    let out = Command::new(bin)
        .args(["tag", "add", "1", "work"])
        .output()
        .expect("spawn tag add");
    assert!(out.status.success(), "tag add failed: {:?}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("added") && stdout.contains("work"),
        "tag add stdout should confirm 'added work': {stdout:?}"
    );

    // tag list 1
    let out = Command::new(bin)
        .args(["tag", "list", "1"])
        .output()
        .expect("spawn tag list");
    assert!(out.status.success(), "tag list failed: {:?}", out.status);

    // tag remove 1 work
    let out = Command::new(bin)
        .args(["tag", "remove", "1", "work"])
        .output()
        .expect("spawn tag remove");
    assert!(out.status.success(), "tag remove failed: {:?}", out.status);
}
