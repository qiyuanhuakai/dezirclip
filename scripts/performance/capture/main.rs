#[cfg(not(feature = "legacy-layout"))]
use app_backend as app;
#[cfg(feature = "legacy-layout")]
use app_backend::legacy as app;

use app::database::{ClipboardEntry, DbState};
use app::infrastructure::repository::{
    clipboard_repo::SqliteClipboardRepository, settings_repo::SqliteSettingsRepository,
    tag_repo::SqliteTagRepository,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[cfg(feature = "engine")]
mod measured_engine;

fn state(path: &Path) -> DbState {
    let conn = Arc::new(Mutex::new(
        app::database::init_db(path.to_str().unwrap()).unwrap(),
    ));
    DbState {
        repo: SqliteClipboardRepository::new(conn.clone()),
        settings_repo: SqliteSettingsRepository::new(conn.clone()),
        tag_repo: SqliteTagRepository::new(conn.clone()),
        conn,
    }
}

fn entry(body: &str, sensitive: bool, rich: bool) -> ClipboardEntry {
    let normalized = app::database::normalize_text(body);
    let preview = if normalized.chars().count() > 500 {
        format!(
            "{}...",
            normalized
                .chars()
                .take(497)
                .collect::<String>()
                .replace('\n', " ")
        )
    } else {
        normalized.replace('\n', " ")
    };
    ClipboardEntry {
        id: 0,
        content_type: if rich { "rich_text" } else { "text" }.into(),
        content: body.into(),
        html_content: rich.then(|| format!("<p>{body}</p>")),
        source_app: "Synthetic performance source".into(),
        source_app_path: None,
        timestamp: 100_000,
        preview,
        is_pinned: false,
        tags: if sensitive {
            vec!["sensitive".into()]
        } else {
            Vec::new()
        },
        use_count: 0,
        is_external: false,
        pinned_order: 0,
        file_preview_exists: true,
        content_kinds: Vec::new(),
        ocr_text: None,
        ocr_status: None,
    }
}

fn seed(path: &Path, rows: usize) {
    let db = state(path);
    let conn = db.conn.lock().unwrap();
    conn.execute("INSERT INTO settings(key,value) VALUES('app.persistent_limit_enabled','false') ON CONFLICT(key) DO UPDATE SET value='false'", []).unwrap();
    conn.execute_batch("BEGIN IMMEDIATE").unwrap();
    for index in 1..=rows {
        let body = format!(
            "synthetic neutral history row {index} {}",
            "sample ".repeat(40)
        );
        conn.execute("INSERT INTO clipboard_history(id,content_type,content,source_app,timestamp,preview,tags) VALUES(?1,'text',?2,'fixture',?1,?2,'[]')", rusqlite::params![index as i64, body]).unwrap();
    }
    conn.execute_batch("COMMIT; PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
}

fn milliseconds(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

fn repository_capture(
    db: &DbState,
    input: &ClipboardEntry,
    directory: &Path,
    duplicate: bool,
    conversion: bool,
) -> (i64, BTreeMap<String, f64>) {
    let mut metrics = BTreeMap::new();
    let mut input = input.clone();
    let total = Instant::now();
    if conversion {
        let start = Instant::now();
        let conn = db.conn.lock().unwrap();
        db.repo.encrypt_entry_with_conn(&conn, input.id).unwrap();
        metrics.insert("conversion_commit_ms".into(), milliseconds(start));
        metrics.insert("repository_total_ms".into(), milliseconds(total));
        return (input.id, metrics);
    }
    input.id = 0;
    {
        let start = Instant::now();
        let conn = db.conn.lock().unwrap();
        let kinds: &[&str] = if input.content_type == "rich_text" {
            &["rich_text", "text", "code", "url"]
        } else {
            &["text"]
        };
        for kind in kinds {
            if let Some(id) = db
                .repo
                .find_by_content_with_hash(&conn, &input.content, Some(kind), None)
                .unwrap()
            {
                let _ = db
                    .repo
                    .get_entry_content_with_html_with_conn(&conn, id)
                    .unwrap()
                    .unwrap();
                let tags: String = conn
                    .query_row(
                        "SELECT tags FROM clipboard_history WHERE id=?1",
                        [id],
                        |row| row.get(0),
                    )
                    .unwrap();
                input.tags = serde_json::from_str(&tags).unwrap();
                input.id = id;
                break;
            }
        }
        assert_eq!(input.id > 0, duplicate);
        metrics.insert("deduplicate_ms".into(), milliseconds(start));
    }
    let start = Instant::now();
    let conn = db.conn.lock().unwrap();
    let id = db
        .repo
        .save_with_conn(&conn, &input, Some(directory))
        .unwrap();
    db.repo
        .enforce_limit_with_conn(&conn, Some(directory))
        .unwrap();
    metrics.insert("commit_ms".into(), milliseconds(start));
    metrics.insert(
        "common_store_total_ms".into(),
        metrics["commit_ms"] + metrics["deduplicate_ms"],
    );
    metrics.insert("repository_total_ms".into(), milliseconds(total));
    (id, metrics)
}

fn verify(db: &DbState, id: i64, input: &ClipboardEntry, rows: usize, protected: bool) -> Value {
    let conn = db.conn.lock().unwrap();
    let (content, preview, html): (String, String, Option<String>) = conn
        .query_row(
            "SELECT content,preview,html_content FROM clipboard_history WHERE id=?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    let decode = |value: &str| {
        app::infrastructure::encryption::decrypt_value(value).unwrap_or_else(|| value.to_owned())
    };
    assert_eq!(decode(&content), input.content);
    assert_eq!(decode(&preview), input.preview);
    assert_eq!(html.as_deref().map(&decode), input.html_content);
    if protected {
        assert!(app::infrastructure::encryption::is_encrypted_value(
            &content
        ));
        assert!(app::infrastructure::encryption::is_encrypted_value(
            &preview
        ));
        assert!(html
            .as_deref()
            .is_none_or(app::infrastructure::encryption::is_encrypted_value));
    }
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM clipboard_history", [], |row| row
            .get::<_, usize>(0))
            .unwrap(),
        rows + 1
    );
    let visible: bool = conn.query_row("SELECT COUNT(*) FROM clipboard_fts WHERE rowid=?1 AND clipboard_fts MATCH 'benchmarker'", [id], |row| Ok(row.get::<_,usize>(0)? > 0)).unwrap();
    assert_eq!(visible, !protected);
    json!({"body_roundtrip":true,"html_roundtrip":true,"row_count":rows+1,"id":id,"protected":protected,"fts_visible":visible})
}

fn summary(values: &mut [f64]) -> Value {
    let mut ordered = values.to_vec();
    ordered.sort_by(f64::total_cmp);
    let n = values.len();
    let median = if n % 2 == 0 {
        (ordered[n / 2 - 1] + ordered[n / 2]) / 2.0
    } else {
        ordered[n / 2]
    };
    json!({"p50_ms":median,"p95_ms":ordered[(n as f64*0.95).ceil() as usize-1],"samples_ms":values})
}

fn main() {
    assert!(
        cfg!(target_os = "windows"),
        "This harness requires real Windows DPAPI"
    );
    #[cfg(feature = "native-provider")]
    dzc_platform::install_default_key_provider();
    let args: Vec<String> = std::env::args().collect();
    assert!(
        args.len() == 5 || args.len() == 6,
        "usage: benchmark <case-root> <rows> <iterations> <output-json> [--verify-only]"
    );
    let verification = args.get(5).is_some_and(|flag| flag == "--verify-only");
    let root = PathBuf::from(&args[1]);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let rows: usize = args[2].parse().unwrap();
    let iterations: usize = args[3].parse().unwrap();
    assert!([1000, 10000].contains(&rows) && (verification || iterations >= 30));
    let template = root.join(format!("template-{}.db", std::process::id()));
    assert!(!template.exists());
    seed(&template, rows);
    let mut results = BTreeMap::new();
    for (name, sensitive, rich, duplicate, conversion) in [
        ("ordinary_new", false, false, false, false),
        ("ordinary_duplicate", false, false, true, false),
        ("rich_new", false, true, false, false),
        ("sensitive_new", true, true, false, false),
        ("sensitive_duplicate", true, false, true, false),
        ("sensitive_conversion", true, true, false, true),
    ] {
        let body = format!(
            "benchmarker synthetic copied payload 中文🙂\r\n{}",
            "sample ".repeat(100)
        );
        let input = entry(&body, sensitive, rich);
        let mut metrics: BTreeMap<String, Vec<f64>> = BTreeMap::new();
        let mut checked = Value::Null;
        for index in 0..if verification { 1 } else { iterations + 5 } {
            let directory = root.join(format!("{}-{name}-{index}", std::process::id()));
            std::fs::create_dir(&directory).unwrap();
            std::fs::copy(&template, directory.join("clipboard.db")).unwrap();
            let db = state(&directory.join("clipboard.db"));
            let mut input = input.clone();
            let expected_id = if duplicate || conversion {
                let mut initial = input.clone();
                if conversion {
                    initial.tags.clear();
                }
                let id = db
                    .repo
                    .save_with_conn(&db.conn.lock().unwrap(), &initial, Some(&directory))
                    .unwrap();
                input.id = id;
                Some(id)
            } else {
                None
            };
            #[cfg(feature = "engine")]
            let (id, times) = if conversion {
                repository_capture(&db, &input, &directory, false, true)
            } else {
                measured_engine::capture(&db, &input, &directory)
            };
            #[cfg(not(feature = "engine"))]
            let (id, times) = repository_capture(&db, &input, &directory, duplicate, conversion);
            if let Some(expected_id) = expected_id {
                assert_eq!(id, expected_id);
            }
            checked = verify(&db, id, &input, rows, sensitive);
            checked["identity_preserved"] =
                json!(expected_id.map_or(id > rows as i64, |expected| id == expected));
            checked["event_check"] = json!(if cfg!(feature = "engine") && !conversion {
                "one Stored event, no other events"
            } else {
                "not an engine event boundary"
            });
            if !verification && index >= 5 {
                for (metric, value) in times {
                    metrics.entry(metric).or_default().push(value);
                }
            }
            drop(db);
            // Every case is a direct child of the explicit benchmark root.
            assert_eq!(
                directory.canonicalize().unwrap().parent(),
                Some(root.as_path())
            );
            std::fs::remove_dir_all(directory).unwrap();
        }
        results.insert(name, json!({"metrics":metrics.iter_mut().map(|(name,values)|(name.clone(),summary(values))).collect::<BTreeMap<_,_>>(),"postconditions":checked}));
    }
    let report = json!({"schema_version":1,"fixture_version":1,"rows":rows,"iterations":if verification {0}else{iterations},"warmups":if verification {0}else{5},"verification_only":verification,"results":results,
        "engine":cfg!(feature="engine"),"provider":"real Windows DPAPI","fixture":"synthetic-only; SQLite seed/open/clone/verification excluded",
        "limits":["Old repository timings exclude its AppHandle pipeline; engine total is a separate scope","No native clipboard read, GUI, worker scheduling, or event transport timing","Store metrics wrap production calls; no SQL model substitutes timed behavior"]});
    let output = PathBuf::from(&args[4]);
    assert!(!output.exists(), "preserve previous report");
    std::fs::write(output, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    std::fs::remove_file(template).unwrap();
}
