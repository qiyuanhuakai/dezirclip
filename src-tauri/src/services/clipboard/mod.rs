mod utils;

use crate::app_state::{AppDataDir, PasteQueue, SessionHistory, SettingsState};
pub use crate::database::DbState;
use dzc_core::capture::{
    CaptureContext, CaptureEngine, CaptureEvent, CaptureFrame, CaptureNotificationState,
    CaptureOutcome, CaptureReadMetadata, CaptureSettings, CaptureSource, EventSink,
    OwnClipboardCopy, StoredCapture,
};
use dzc_platform::capture::{self as native, NativeCaptureReader};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

#[cfg(target_os = "linux")]
use crate::infrastructure::linux_api::window_tracker::{
    get_clipboard_capture_app_info as resolve_source, ActiveAppInfo,
};
#[cfg(target_os = "windows")]
use crate::infrastructure::windows_api::window_tracker::{
    get_clipboard_source_app_info as resolve_source, ActiveAppInfo,
};

pub use dzc_core::capture::CaptureData as ClipboardData;
pub use utils::truncate_html_for_preview;

pub(crate) fn summarize_captured_entry(
    entry: crate::domain::models::ClipboardEntry,
) -> crate::domain::models::ClipboardEntry {
    match utils::truncate_entry_for_ui(&entry) {
        std::borrow::Cow::Borrowed(_) => entry,
        std::borrow::Cow::Owned(summary) => summary,
    }
}

struct CaptureTask {
    app: AppHandle,
    frame: CaptureFrame,
    source: CaptureSource,
    timestamp: i64,
}

fn timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn settings_snapshot(settings: &SettingsState) -> CaptureSettings {
    CaptureSettings {
        persistent: settings.persistent.load(Ordering::Relaxed),
        deduplicate: settings.deduplicate.load(Ordering::Relaxed),
        capture_files: settings.capture_files.load(Ordering::Relaxed),
        capture_rich_text: settings.capture_rich_text.load(Ordering::Relaxed),
        privacy_protection: settings.privacy_protection.load(Ordering::Relaxed),
        privacy_kinds: settings
            .privacy_protection_kinds
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone(),
        privacy_custom_rules: settings
            .privacy_protection_custom_rules
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone(),
        excluded_source_apps: settings
            .excluded_source_apps
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone(),
        ocr_enabled: settings.ocr_enabled.load(Ordering::Relaxed),
    }
}

fn context<'a>(
    app: &AppHandle,
    source: CaptureSource,
    timestamp: i64,
    session: &'a SessionHistory,
) -> CaptureContext<'a> {
    let settings = app.state::<SettingsState>();
    let sequential_echo = if settings.sequential_mode.load(Ordering::Relaxed) {
        let queue_state = app.state::<PasteQueue>();
        let queue = queue_state
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if queue.last_action_was_paste {
            queue.last_pasted_content.clone()
        } else {
            None
        }
    } else {
        None
    };
    CaptureContext {
        source,
        timestamp,
        settings: settings_snapshot(&settings),
        session: &session.0,
        paused: crate::CLIPBOARD_MONITOR_PAUSED.load(Ordering::Relaxed),
        sequential_echo,
    }
}

fn source(snapshot: Option<ActiveAppInfo>, override_name: Option<String>) -> CaptureSource {
    let snapshot = snapshot.unwrap_or_else(resolve_source);
    CaptureSource {
        app_name: override_name.clone().unwrap_or(snapshot.app_name),
        process_path: if override_name.is_some() {
            None
        } else {
            snapshot.process_path
        },
    }
}

struct DesktopEvents<'a> {
    app: &'a AppHandle,
}

impl EventSink for DesktopEvents<'_> {
    fn emit(&mut self, event: CaptureEvent) {
        match event {
            CaptureEvent::Removed { id } => {
                if deliver_capture() {
                    let _ = self.app.emit("clipboard-removed", id);
                }
            }
            CaptureEvent::Rejected(reason) => crate::warn!("[capture] rejected: {:?}", reason),
            CaptureEvent::Failed(error) => {
                crate::warn!("[capture] {:?}: {}", error.stage, error.code)
            }
            _ => {}
        }
    }
}

fn deliver_capture() -> bool {
    crate::app::idle_destroyer::should_deliver_capture(
        crate::app::idle_destroyer::main_window_on_screen(),
        !crate::global_state::IS_DESTROYED.load(Ordering::Relaxed),
    )
}

#[derive(Clone, serde::Serialize)]
struct CapturedEntryId {
    id: i64,
}

fn distribute(app: &AppHandle, capture: StoredCapture) {
    let settings = app.state::<SettingsState>();
    let entry = &capture.entry;
    if let Some(id) = capture
        .ocr_target
        .filter(|_| settings.ocr_enabled.load(Ordering::Relaxed))
    {
        if let Some(bytes) = crate::services::clipboard_ops::resolve_image_bytes(&entry.content) {
            tauri::async_runtime::spawn(
                crate::services::clipboard_ops::trigger_ocr_for_image_item(id, bytes, app.clone()),
            );
        }
    }
    let deliver = deliver_capture();
    if !deliver {
        crate::app::idle_destroyer::mark_frontend_catchup_pending();
    }
    if settings.sequential_mode.load(Ordering::Relaxed) {
        let queue_state = app.state::<PasteQueue>();
        let length = {
            let mut queue = queue_state
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if queue.last_action_was_paste {
                queue.items.clear();
                queue.last_action_was_paste = false;
                queue.last_pasted_content = None;
            }
            queue.items.push_back(entry.id);
            queue.items.len()
        };
        let _ = app.emit("paste-queue-changed", length);
    }
    if settings.sound_enabled.load(Ordering::Relaxed) {
        let _ = app.emit("play-sound", "copy");
    }
    if deliver {
        let _ = app.emit("clipboard-captured", CapturedEntryId { id: entry.id });
    }
}

fn process(task: CaptureTask) {
    let session = task.app.state::<SessionHistory>();
    let context = context(&task.app, task.source, task.timestamp, &session);
    let data = task
        .app
        .state::<AppDataDir>()
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let database = task.app.state::<DbState>();
    let history = database.history_service();
    let store = history.capture_store(&data);
    let mut events = DesktopEvents { app: &task.app };
    if let CaptureOutcome::Stored(capture) =
        CaptureEngine::capture_frame(&context, task.frame, &store, &mut events)
    {
        distribute(&task.app, capture);
    }
}

fn sender() -> &'static Sender<CaptureTask> {
    static SENDER: OnceLock<Sender<CaptureTask>> = OnceLock::new();
    SENDER.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        let _ = std::thread::Builder::new()
            .name("clipboard-process-worker".into())
            .spawn(move || {
                while let Ok(task) = rx.recv() {
                    process(task);
                }
            });
        tx
    })
}

fn own_copy(timestamp: i64) -> OwnClipboardCopy {
    OwnClipboardCopy {
        primary: crate::LAST_APP_SET_HASH.load(Ordering::SeqCst),
        alternative: crate::LAST_APP_SET_HASH_ALT.load(Ordering::SeqCst),
        age_seconds: (timestamp.max(0) as u64 / 1000)
            .saturating_sub(crate::LAST_APP_SET_TIMESTAMP.load(Ordering::SeqCst)),
    }
}

fn clear_own_copy() {
    crate::LAST_APP_SET_HASH.store(0, Ordering::SeqCst);
    crate::LAST_APP_SET_HASH_ALT.store(0, Ordering::SeqCst);
}

pub fn start_clipboard_monitor(app: AppHandle) {
    let notifications = Arc::new(Mutex::new(CaptureNotificationState::at_sequence(
        native::sequence_number(),
    )));
    native::listen_clipboard(Arc::new(move |change| {
        let mut notifications = notifications
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !notifications.accept_sequence(change.sequence) {
            return;
        }
        let timestamp = timestamp();
        let source = if crate::CLIPBOARD_MONITOR_PAUSED.load(Ordering::Relaxed) {
            CaptureSource::default()
        } else {
            source(None, None)
        };
        let session = app.state::<SessionHistory>();
        let context = context(&app, source.clone(), timestamp, &session);
        notifications.set_persistence_mode(context.settings.persistent);
        let mut events = DesktopEvents { app: &app };
        let data = app.state::<AppDataDir>().0.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        let database = app.state::<DbState>();
        let history = database.history_service();
        let store = history.capture_store(&data);
        let Ok(frame) =
            CaptureEngine::read_authorized(&context, &mut NativeCaptureReader::default(), &store, &mut events)
        else {
            return;
        };
        let repeated_file_exists = if notifications.repeats_file(frame.metadata) {
            match CaptureEngine::file_history_exists(&context, &frame, &store) {
                Ok(exists) => exists,
                Err(error) => {
                    events.emit(CaptureEvent::Failed(error));
                    return;
                }
            }
        } else {
            false
        };
        if let Err(reason) = notifications.inspect(
            frame.metadata,
            timestamp.max(0) as u64,
            own_copy(timestamp),
            repeated_file_exists,
        ) {
            if reason == dzc_core::capture::CaptureSkip::OwnClipboard {
                clear_own_copy();
            }
            events.emit(CaptureEvent::Skipped(reason));
            return;
        }
        clear_own_copy();
        let task = CaptureTask {
            app: app.clone(),
            frame,
            source,
            timestamp,
        };
        if let Err(error) = sender().send(task) {
            process(error.0);
        }
    }));
}

pub fn process_new_entry(
    app: &AppHandle,
    data: ClipboardData,
    source_override: Option<String>,
    source_snapshot: Option<ActiveAppInfo>,
) {
    process(CaptureTask {
        app: app.clone(),
        frame: CaptureFrame {
            data,
            metadata: CaptureReadMetadata::default(),
        },
        source: source(source_snapshot, source_override),
        timestamp: timestamp(),
    });
}

#[cfg(all(test, target_os = "windows", not(feature = "portable")))]
mod tests {
    use super::*;
    use crate::infrastructure::repository::{
        clipboard_repo::SqliteClipboardRepository,
        settings_repo::SqliteSettingsRepository,
        tag_repo::{SqliteTagRepository, TagRepository},
    };
    use dzc_core::privacy::{PrivacyAction, PrivacyReason};
    use std::collections::VecDeque;

    #[test]
    fn engine_recapture_keeps_manually_protected_text_encrypted_in_the_same_row() {
        dzc_platform::install_default_key_provider();
        let directory =
            std::env::temp_dir().join(format!("dezirclip-capture-dpapi-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let connection = Arc::new(Mutex::new(crate::database::init_db(":memory:").unwrap()));
        let state = DbState {
            repo: SqliteClipboardRepository::new(connection.clone()),
            settings_repo: SqliteSettingsRepository::new(connection.clone()),
            tag_repo: SqliteTagRepository::new(connection.clone()),
            conn: connection,
        };
        let session = Mutex::new(VecDeque::new());
        let mut context = CaptureContext {
            source: CaptureSource {
                app_name: "Editor".into(),
                process_path: None,
            },
            timestamp: 1,
            settings: CaptureSettings {
                persistent: true,
                deduplicate: true,
                ..CaptureSettings::default()
            },
            session: &session,
            paused: false,
            sequential_echo: None,
        };
        let history = state.history_service();
        let store = history.capture_store(&directory);
        let text = "ordinary manually protected text";
        let frame = || CaptureFrame {
            data: ClipboardData::Text(text.into()),
            metadata: CaptureReadMetadata::default(),
        };
        let CaptureOutcome::Stored(first) =
            CaptureEngine::capture_frame(&context, frame(), &store, &mut ())
        else {
            panic!("first capture should store");
        };
        state
            .tag_repo
            .update_entry_tags(first.entry.id, vec!["sensitive".into(), "work".into()])
            .unwrap();
        context.timestamp = 2;
        let CaptureOutcome::Stored(second) =
            CaptureEngine::capture_frame(&context, frame(), &store, &mut ())
        else {
            panic!("protected recapture should store");
        };
        assert_eq!(first.entry.id, second.entry.id);
        assert_eq!(second.entry.tags, vec!["sensitive", "work"]);
        assert_eq!(second.privacy.action, PrivacyAction::Encrypt);
        assert_eq!(
            second.privacy.reason,
            PrivacyReason::ExistingRecordProtected
        );
        let connection = state.conn.lock().unwrap();
        let (ciphertext, timestamp): (String, i64) = connection
            .query_row(
                "SELECT content,timestamp FROM clipboard_history WHERE id=?1",
                [first.entry.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!(crate::infrastructure::encryption::has_ciphertext_envelope(
            &ciphertext
        ));
        assert_eq!(
            crate::infrastructure::encryption::decode_for_mutation(&ciphertext).unwrap(),
            text
        );
        assert_eq!(timestamp, 2);
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM clipboard_history", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM clipboard_fts WHERE clipboard_fts MATCH 'protected'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        drop(connection);
        drop(store);
        drop(state);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
