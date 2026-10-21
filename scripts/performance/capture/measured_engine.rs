use super::*;
#[cfg(not(feature = "legacy-layout"))]
use app::services::capture::*;
#[cfg(feature = "legacy-layout")]
use app_backend::capture::*;
use std::cell::RefCell;
use std::collections::VecDeque;

#[cfg(feature = "privacy-runtime")]
#[path = "exclusive_guard.rs"]
mod exclusive_guard;
#[cfg(feature = "privacy-runtime")]
use exclusive_guard::measure_guard_excluding_publish;

struct TimedStore<T> {
    actual: T,
    times: RefCell<BTreeMap<String, f64>>,
}
impl<T: CaptureStore> TimedStore<T> {
    fn timed<R>(&self, name: &str, operation: impl FnOnce() -> R) -> R {
        let start = Instant::now();
        let result = operation();
        *self.times.borrow_mut().entry(name.into()).or_default() += milliseconds(start);
        result
    }
}
impl<T: CaptureStore> CaptureStore for TimedStore<T> {
    #[cfg(feature = "privacy-runtime")]
    fn capture_ticket(&self, now_ms: i64) -> Result<Option<app_backend::api::CaptureTicket>, CaptureFailure> {
        self.timed("privacy_ticket_ms", || self.actual.capture_ticket(now_ms))
    }
    #[cfg(feature = "privacy-runtime")]
    fn check_capture(&self, ticket: Option<&app_backend::api::CaptureTicket>, now_ms: i64) -> Result<(), CaptureFailure> {
        self.timed("privacy_check_ms", || self.actual.check_capture(ticket, now_ms))
    }
    #[cfg(feature = "privacy-runtime")]
    fn with_capture_guard(&self, ticket: Option<&app_backend::api::CaptureTicket>, now_ms: i64, publish: &mut dyn FnMut()) -> Result<(), CaptureFailure> {
        let clock = Instant::now();
        let (result, exclusive) = measure_guard_excluding_publish(
            || clock.elapsed(),
            |publish| self.actual.with_capture_guard(ticket, now_ms, publish),
            publish,
        );
        *self.times.borrow_mut().entry("privacy_guard_ms".into()).or_default() +=
            exclusive.as_secs_f64() * 1000.0;
        result
    }
    fn prepare(&self, entry: ClipboardEntry) -> Result<PreparedCapture, CaptureFailure> {
        self.timed("prepare_ms", || self.actual.prepare(entry))
    }
    fn find_duplicate(
        &self,
        entry: &ClipboardEntry,
        kind: &str,
        hash: Option<i64>,
    ) -> Result<Option<CaptureDuplicate>, CaptureFailure> {
        self.timed("deduplicate_ms", || {
            self.actual.find_duplicate(entry, kind, hash)
        })
    }
    fn contains(&self, content: &str) -> Result<bool, CaptureFailure> {
        self.timed("contains_ms", || self.actual.contains(content))
    }
    fn contains_image_hash(&self, image_hash: i64) -> Result<bool, CaptureFailure> {
        self.timed("contains_image_hash_ms", || {
            self.actual.contains_image_hash(image_hash)
        })
    }
    fn commit(&self, capture: &PreparedCapture) -> Result<CaptureCommit, CaptureFailure> {
        self.timed("commit_ms", || self.actual.commit(capture))
    }
}
#[derive(Default)]
struct Events {
    stored: usize,
    other: usize,
}
impl EventSink for Events {
    fn emit(&mut self, event: CaptureEvent) {
        if matches!(event, CaptureEvent::Stored { .. }) {
            self.stored += 1;
        } else {
            self.other += 1;
        }
    }
}

pub fn capture(
    db: &DbState,
    input: &ClipboardEntry,
    directory: &Path,
) -> (i64, BTreeMap<String, f64>) {
    let session = Mutex::new(VecDeque::new());
    let sensitive = app::database::has_sensitive_tag(&input.tags);
    let context = CaptureContext {
        source: CaptureSource {
            app_name: input.source_app.clone(),
            process_path: None,
        },
        timestamp: input.timestamp,
        settings: CaptureSettings {
            persistent: true,
            deduplicate: true,
            capture_rich_text: true,
            privacy_protection: sensitive && input.id == 0,
            privacy_custom_rules: if sensitive {
                vec!["benchmarker".into()]
            } else {
                Vec::new()
            },
            ..CaptureSettings::default()
        },
        session: &session,
        paused: false,
        sequential_echo: None,
    };
    #[cfg(feature = "legacy-layout")]
    let history = db.history_service();
    #[cfg(feature = "legacy-layout")]
    let actual = history.capture_store(directory);
    #[cfg(not(feature = "legacy-layout"))]
    let actual = db.capture_store(directory);
    let store = TimedStore {
        actual,
        times: RefCell::new(BTreeMap::new()),
    };
    let start = Instant::now();
    #[cfg(feature = "privacy-runtime")]
    let privacy_ticket = store.capture_ticket(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_millis().try_into().unwrap())
        .expect("actual Core must authorize the capture before payload construction")
        .expect("the production SQLite store must issue a real capture ticket");
    let frame = CaptureFrame {
        data: match &input.html_content {
            Some(html) => CaptureData::RichText {
                text: input.content.clone(),
                html: html.clone(),
            },
            None => CaptureData::Text(input.content.clone()),
        },
        metadata: CaptureReadMetadata {
            #[cfg(feature = "privacy-runtime")]
            privacy_ticket: Some(privacy_ticket),
            ..CaptureReadMetadata::default()
        },
    };
    let mut events = Events::default();
    let result = CaptureEngine::capture_frame(&context, frame, &store, &mut events);
    let elapsed = milliseconds(start);
    let CaptureOutcome::Stored(stored) = result else {
        panic!("actual CaptureEngine did not commit: {result:?}");
    };
    assert!(stored.persistent && stored.entry.id > 0);
    assert_eq!(events.stored, 1);
    assert_eq!(events.other, 0);
    let mut times = store.times.into_inner();
    let storage: f64 = times.values().sum();
    assert!(storage <= elapsed, "store timers must not overlap: {times:?}; engine={elapsed}");
    times.insert("engine_total_ms".into(), elapsed);
    times.insert("store_total_ms".into(), storage);
    times.insert(
        "common_store_total_ms".into(),
        times.get("deduplicate_ms").copied().unwrap_or_default()
            + times.get("commit_ms").copied().unwrap_or_default(),
    );
    times.insert("engine_non_store_ms".into(), elapsed - storage);
    (stored.entry.id, times)
}

#[cfg(all(test, feature = "privacy-runtime", target_os = "windows", feature = "native-provider"))]
mod tests {
    use super::*;

    fn actual_contract2_partition(sensitive: bool) {
        dzc_platform::install_default_key_provider();
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "dzc-capture-timer-contract2-{}-{nonce}-{sensitive}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        let database = directory.join("clipboard.db");
        seed(&database, 1000);
        let db = state(&database);
        let input = entry("benchmarker synthetic timer partition 中文🙂", sensitive, sensitive);
        let (id, times) = capture(&db, &input, &directory);
        assert!(times["commit_ms"] > 0.0);
        assert!(times["privacy_ticket_ms"] >= 0.0);
        assert!(times["privacy_check_ms"] >= 0.0);
        assert!(times["store_total_ms"] <= times["engine_total_ms"]);
        assert_eq!(times["engine_non_store_ms"], times["engine_total_ms"] - times["store_total_ms"]);
        let postconditions = verify(&db, id, &input, 1000, sensitive);
        eprintln!("contract2-functional-only {}", json!({"sensitive":sensitive,"metrics":times,"postconditions":postconditions}));
        drop(db);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn actual_contract2_ordinary_capture_has_nonoverlapping_timers() {
        actual_contract2_partition(false);
    }

    #[test]
    fn actual_contract2_protected_capture_has_nonoverlapping_timers() {
        actual_contract2_partition(true);
    }

    #[test]
    fn actual_contract2_session_capture_measures_guard_without_a_store_commit() {
        dzc_platform::install_default_key_provider();
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "dzc-session-timer-contract2-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        let db = state(&directory.join("clipboard.db"));
        {
        let history = db.history_service();
        let store = TimedStore {
            actual: history.capture_store(&directory),
            times: RefCell::new(BTreeMap::new()),
        };
        let session = Mutex::new(VecDeque::new());
        let context = CaptureContext {
            source: CaptureSource { app_name: "synthetic session timer".into(), process_path: None },
            timestamp: 100_000,
            settings: CaptureSettings { persistent: false, ..CaptureSettings::default() },
            session: &session,
            paused: false,
            sequential_echo: None,
        };
        let start = Instant::now();
        let ticket = store.capture_ticket(chrono_now_ms()).unwrap().unwrap();
        let mut events = Events::default();
        let result = CaptureEngine::capture_frame(&context, CaptureFrame {
            data: CaptureData::Text("synthetic session timer payload".into()),
            metadata: CaptureReadMetadata { privacy_ticket: Some(ticket), ..CaptureReadMetadata::default() },
        }, &store, &mut events);
        let elapsed = milliseconds(start);
        let CaptureOutcome::Stored(stored) = result else { panic!("session capture rejected: {result:?}") };
        assert!(!stored.persistent && stored.entry.id < 0);
        assert_eq!((events.stored, events.other), (1, 0));
        assert_eq!(session.lock().unwrap().len(), 1);
        let times = store.times.into_inner();
        assert!(times["privacy_guard_ms"] >= 0.0);
        assert!(!times.contains_key("commit_ms"));
        let storage: f64 = times.values().sum();
        assert!(storage <= elapsed, "session store timers overlap: {times:?}; engine={elapsed}");
        assert_eq!(db.conn.lock().unwrap().query_row("SELECT COUNT(*) FROM clipboard_history", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
        eprintln!("contract2-session-functional-only {}", json!({"metrics":times,"engine_total_ms":elapsed,"store_total_ms":storage,"engine_non_store_ms":elapsed-storage,"session_rows":1,"database_rows":0}));
        }
        drop(db);
        std::fs::remove_dir_all(directory).unwrap();
    }

    fn chrono_now_ms() -> i64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .unwrap().as_millis().try_into().unwrap()
    }
}
