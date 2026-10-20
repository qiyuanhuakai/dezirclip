use super::{CaptureFailure, CaptureStage};
use crate::database::DbState;
use crate::domain::models::ClipboardEntry;
use crate::infrastructure::attachments::{plan_html_attachments, HtmlAttachmentPlan};
use crate::infrastructure::repository::clipboard_repo::SqliteClipboardRepository;
use rusqlite::Connection;
use std::path::Path;
use std::sync::{Arc, Mutex};

pub struct PreparedCapture {
    pub entry: ClipboardEntry,
    pub image_hash: Option<i64>,
    attachments: Option<HtmlAttachmentPlan>,
}

impl PreparedCapture {
    pub fn without_attachments(entry: ClipboardEntry) -> Self {
        Self {
            entry,
            image_hash: None,
            attachments: None,
        }
    }
}

pub struct CaptureDuplicate {
    pub id: i64,
    pub content_type: String,
    pub html_content: Option<String>,
    pub tags: Vec<String>,
}

pub struct CaptureCommit {
    pub id: i64,
    pub removed_ids: Vec<i64>,
}

/// Capture-specific storage operations. A commit must recheck current protection.
pub trait CaptureStore {
    fn prepare(&self, entry: ClipboardEntry) -> Result<PreparedCapture, CaptureFailure>;
    fn find_duplicate(
        &self,
        entry: &ClipboardEntry,
        content_type: &str,
        image_hash: Option<i64>,
    ) -> Result<Option<CaptureDuplicate>, CaptureFailure>;
    fn contains(&self, content: &str) -> Result<bool, CaptureFailure>;
    fn contains_image_hash(&self, _image_hash: i64) -> Result<bool, CaptureFailure> {
        Err(CaptureFailure {
            stage: CaptureStage::Deduplicate,
            code: "CaptureImageLookupUnsupported".into(),
        })
    }
    fn commit(&self, capture: &PreparedCapture) -> Result<CaptureCommit, CaptureFailure>;
}

struct SqliteCaptureStore<'a> {
    repository: &'a SqliteClipboardRepository,
    connection: Arc<Mutex<Connection>>,
    directory: &'a Path,
}

fn failure(stage: CaptureStage, error: impl ToString) -> CaptureFailure {
    let error = error.to_string();
    let code = match error.as_str() {
        "SensitivePersistenceRejected"
        | "ProtectedRecordUnavailable"
        | "KeyUnavailable"
        | "EncryptionFailed"
        | "AttachmentPersistenceRejected"
        | "AttachmentPersistenceFailed"
        | "ClipboardRecordUnavailable"
        | "AttachmentBudgetExceeded" => error,
        _ => "CaptureStorageFailed".to_string(),
    };
    CaptureFailure { stage, code }
}

impl DbState {
    /// Transitional embedded adapter; the capture contract itself exposes no connection.
    pub fn capture_store<'a>(&'a self, directory: &'a Path) -> impl CaptureStore + 'a {
        self.repo.capture_store(directory)
    }
}

pub(crate) fn sqlite_capture_store<'a>(
    repository: &'a SqliteClipboardRepository,
    connection: Arc<Mutex<Connection>>,
    directory: &'a Path,
) -> impl CaptureStore + 'a {
    SqliteCaptureStore {
        repository,
        connection,
        directory,
    }
}

impl CaptureStore for SqliteCaptureStore<'_> {
    fn prepare(&self, mut entry: ClipboardEntry) -> Result<PreparedCapture, CaptureFailure> {
        let attachments = entry
            .html_content
            .as_deref()
            .map(|html| plan_html_attachments(html, self.directory))
            .transpose()
            .map_err(|error| failure(CaptureStage::Prepare, error))?;
        if let Some(plan) = &attachments {
            entry.html_content = Some(plan.html().to_string());
        }
        Ok(PreparedCapture {
            entry,
            attachments,
            image_hash: None,
        })
    }

    fn find_duplicate(
        &self,
        entry: &ClipboardEntry,
        content_type: &str,
        image_hash: Option<i64>,
    ) -> Result<Option<CaptureDuplicate>, CaptureFailure> {
        let connection = self
            .connection
            .lock()
            .map_err(|error| failure(CaptureStage::Deduplicate, error))?;
        let id = self
            .repository
            .find_by_content_with_hash(&connection, &entry.content, Some(content_type), image_hash)
            .map_err(|error| failure(CaptureStage::Deduplicate, error))?;
        let Some(id) = id else {
            return Ok(None);
        };
        let (_, content_type, html_content) = self
            .repository
            .get_entry_content_with_html_with_conn(&connection, id)
            .map_err(|error| failure(CaptureStage::Deduplicate, error))?
            .ok_or_else(|| failure(CaptureStage::Deduplicate, "ClipboardRecordUnavailable"))?;
        let tags: String = connection
            .query_row(
                "SELECT tags FROM clipboard_history WHERE id=?1",
                [id],
                |row| row.get(0),
            )
            .map_err(|error| failure(CaptureStage::Deduplicate, error))?;
        let tags = serde_json::from_str(&tags)
            .map_err(|_| failure(CaptureStage::Deduplicate, "SensitivePersistenceRejected"))?;
        Ok(Some(CaptureDuplicate {
            id,
            content_type,
            html_content,
            tags,
        }))
    }

    fn contains(&self, content: &str) -> Result<bool, CaptureFailure> {
        let connection = self
            .connection
            .lock()
            .map_err(|error| failure(CaptureStage::Deduplicate, error))?;
        self.repository
            .find_by_content_with_conn(&connection, content, None)
            .map(|entry| entry.is_some())
            .map_err(|error| failure(CaptureStage::Deduplicate, error))
    }

    fn contains_image_hash(&self, image_hash: i64) -> Result<bool, CaptureFailure> {
        let connection = self
            .connection
            .lock()
            .map_err(|error| failure(CaptureStage::Deduplicate, error))?;
        connection.query_row("SELECT EXISTS(SELECT 1 FROM clipboard_history WHERE content_type='image' AND content_hash=?1)", [image_hash], |row| row.get(0))
            .map_err(|error| failure(CaptureStage::Deduplicate, error))
    }

    fn commit(&self, capture: &PreparedCapture) -> Result<CaptureCommit, CaptureFailure> {
        let connection = self
            .connection
            .lock()
            .map_err(|error| failure(CaptureStage::Persist, error))?;
        let id = self
            .repository
            .save_capture_with_conn(
                &connection,
                &capture.entry,
                Some(self.directory),
                capture.image_hash,
                capture.attachments.as_ref(),
            )
            .map_err(|error| failure(CaptureStage::Persist, error))?;
        let removed_ids = self
            .repository
            .enforce_limit_with_conn(&connection, Some(self.directory))
            .unwrap_or_default();
        Ok(CaptureCommit { id, removed_ids })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::repository::{
        clipboard_repo::SqliteClipboardRepository, settings_repo::SqliteSettingsRepository,
        tag_repo::SqliteTagRepository,
    };
    use crate::services::capture::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    fn database() -> (std::path::PathBuf, DbState) {
        let directory =
            std::env::temp_dir().join(format!("dezirclip-capture-engine-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let connection = Arc::new(Mutex::new(
            crate::database::init_db(directory.join("clipboard.db").to_str().unwrap()).unwrap(),
        ));
        let state = DbState {
            repo: SqliteClipboardRepository::new(connection.clone()),
            settings_repo: SqliteSettingsRepository::new(connection.clone()),
            tag_repo: SqliteTagRepository::new(connection.clone()),
            conn: connection,
        };
        (directory, state)
    }

    fn context(session: &Mutex<VecDeque<ClipboardEntry>>) -> CaptureContext<'_> {
        CaptureContext {
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
            session,
            paused: false,
            sequential_echo: None,
        }
    }

    fn rich_frame() -> CaptureFrame {
        CaptureFrame {
            data: CaptureData::RichText {
                text: "ordinary capture".into(),
                html:
                    "<p>ordinary capture</p><!--TIEZ_RICH_IMAGE:data:image/png;base64,b3RoZXI=-->"
                        .into(),
            },
            metadata: CaptureReadMetadata::default(),
        }
    }

    #[derive(Default)]
    struct Events(Vec<CaptureEvent>);
    impl EventSink for Events {
        fn emit(&mut self, event: CaptureEvent) {
            self.0.push(event);
        }
    }

    #[test]
    fn real_capture_deduplicates_stable_html_assets_and_preserves_existing_ocr() {
        let (directory, state) = database();
        let session = Mutex::new(VecDeque::new());
        let context = context(&session);
        let store = state.capture_store(&directory);
        let CaptureOutcome::Stored(first) =
            CaptureEngine::capture_frame(&context, rich_frame(), &store, &mut ())
        else {
            panic!("capture should commit");
        };
        state
            .repo
            .update_ocr_text_with_conn(
                &state.conn.lock().unwrap(),
                first.entry.id,
                "previous OCR",
                "done",
            )
            .unwrap();
        let CaptureOutcome::Stored(second) =
            CaptureEngine::capture_frame(&context, rich_frame(), &store, &mut ())
        else {
            panic!("recapture should commit");
        };
        assert_eq!(first.entry.id, second.entry.id);
        let conn = state.conn.lock().unwrap();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM clipboard_history", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row("SELECT ocr_text FROM clipboard_history", [], |row| row
                .get::<_, String>(
                0
            ))
            .unwrap(),
            "previous OCR"
        );
        assert_eq!(
            std::fs::read_dir(directory.join("attachments"))
                .unwrap()
                .count(),
            1
        );
        drop(conn);
        drop(store);
        drop(state);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn deferred_commit_failure_emits_no_success_and_removes_only_new_assets() {
        let (directory, state) = database();
        state.conn.lock().unwrap().execute_batch("PRAGMA foreign_keys=ON;
            CREATE TABLE reject_commit(history_id INTEGER REFERENCES clipboard_history(id) DEFERRABLE INITIALLY DEFERRED);
            CREATE TRIGGER reject_capture AFTER INSERT ON clipboard_history BEGIN INSERT INTO reject_commit VALUES(-12345); END;").unwrap();
        let session = Mutex::new(VecDeque::new());
        let context = context(&session);
        let store = state.capture_store(&directory);
        let mut events = Events::default();
        assert!(matches!(
            CaptureEngine::capture_frame(&context, rich_frame(), &store, &mut events),
            CaptureOutcome::Failed(_)
        ));
        assert!(matches!(events.0.as_slice(), [CaptureEvent::Failed(_)]));
        assert_eq!(
            state
                .conn
                .lock()
                .unwrap()
                .query_row("SELECT count(*) FROM clipboard_history", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            std::fs::read_dir(directory.join("attachments"))
                .unwrap()
                .count(),
            0
        );
        assert!(session.lock().unwrap().is_empty());
        drop(store);
        drop(state);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn dedup_inherits_manual_protection_before_materializing_new_html_resources() {
        let (directory, state) = database();
        state.conn.lock().unwrap().execute("INSERT INTO clipboard_history(content_type,content,preview,source_app,timestamp,tags) VALUES('text','ordinary capture','ordinary','test',1,'[\"sensitive\"]')", []).unwrap();
        let session = Mutex::new(VecDeque::new());
        let context = context(&session);
        let store = state.capture_store(&directory);
        let mut events = Events::default();
        assert!(matches!(
            CaptureEngine::capture_frame(&context, rich_frame(), &store, &mut events),
            CaptureOutcome::Rejected(CaptureRejection::SensitivePersistenceRejected)
        ));
        assert_eq!(
            events.0,
            vec![CaptureEvent::Rejected(
                CaptureRejection::SensitivePersistenceRejected
            )]
        );
        assert!(!directory.join("attachments").exists());
        let conn = state.conn.lock().unwrap();
        assert_eq!(
            conn.query_row("SELECT content FROM clipboard_history", [], |row| row
                .get::<_, String>(0))
                .unwrap(),
            "ordinary capture"
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM clipboard_history", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        drop(conn);
        drop(store);
        drop(state);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn real_attachment_budget_rejection_creates_no_files_or_rows() {
        let (directory, state) = database();
        let session = Mutex::new(VecDeque::new());
        let context = context(&session);
        let store = state.capture_store(&directory);
        let source = directory.join("source.png");
        std::fs::write(&source, b"fixture image").unwrap();
        let html = format!(
            "<img src='file://{}'>",
            source.to_string_lossy().replace('\\', "/")
        )
        .repeat(65);
        let frame = CaptureFrame {
            data: CaptureData::RichText {
                text: "ordinary".into(),
                html,
            },
            metadata: CaptureReadMetadata::default(),
        };
        let mut events = Events::default();
        assert!(matches!(
            CaptureEngine::capture_frame(&context, frame, &store, &mut events),
            CaptureOutcome::Rejected(CaptureRejection::AttachmentBudgetExceeded)
        ));
        assert_eq!(
            events.0,
            vec![CaptureEvent::Rejected(
                CaptureRejection::AttachmentBudgetExceeded
            )]
        );
        assert_eq!(
            state
                .conn
                .lock()
                .unwrap()
                .query_row("SELECT count(*) FROM clipboard_history", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert!(!directory.join("attachments").exists());
        assert_eq!(std::fs::read(source).unwrap(), b"fixture image");
        drop(store);
        drop(state);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn session_html_embeds_with_a_budget_and_failed_reads_leave_session_unchanged() {
        let (directory, state) = database();
        let source = directory.join("source.png");
        std::fs::write(&source, b"fixture image").unwrap();
        let image = format!(
            "<img src='file://{}'>",
            source.to_string_lossy().replace('\\', "/")
        );
        let session = Mutex::new(VecDeque::new());
        let mut context = context(&session);
        context.settings.persistent = false;
        let store = state.capture_store(&directory);
        let frame = |html| CaptureFrame {
            data: CaptureData::RichText {
                text: "session fixture".into(),
                html,
            },
            metadata: CaptureReadMetadata::default(),
        };
        let CaptureOutcome::Stored(first) =
            CaptureEngine::capture_frame(&context, frame(image.clone()), &store, &mut ())
        else {
            panic!("bounded session HTML should store");
        };
        assert!(first.entry.id < 0);
        assert!(first
            .entry
            .html_content
            .unwrap()
            .contains("data:image/png;base64,Zml4dHVyZSBpbWFnZQ=="));
        let mut events = Events::default();
        assert!(matches!(
            CaptureEngine::capture_frame(&context, frame(image.repeat(65)), &store, &mut events),
            CaptureOutcome::Rejected(CaptureRejection::AttachmentBudgetExceeded)
        ));
        std::fs::File::create(&source)
            .unwrap()
            .set_len(16 * 1024 * 1024 + 1)
            .unwrap();
        assert!(matches!(
            CaptureEngine::capture_frame(&context, frame(image), &store, &mut events),
            CaptureOutcome::Rejected(CaptureRejection::AttachmentBudgetExceeded)
        ));
        assert_eq!(
            events.0,
            vec![CaptureEvent::Rejected(CaptureRejection::AttachmentBudgetExceeded); 2]
        );
        assert_eq!(session.lock().unwrap().len(), 1);
        assert_eq!(
            state
                .conn
                .lock()
                .unwrap()
                .query_row("SELECT count(*) FROM clipboard_history", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert!(!directory.join("attachments").exists());
        drop(store);
        drop(state);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn session_html_rejects_fifo_without_waiting_for_a_writer() {
        let (directory, state) = database();
        let source = directory.join("source.png");
        assert!(std::process::Command::new("mkfifo")
            .arg(&source)
            .status()
            .unwrap()
            .success());
        let session = Mutex::new(VecDeque::new());
        let mut context = context(&session);
        context.settings.persistent = false;
        let store = state.capture_store(&directory);
        let frame = CaptureFrame {
            data: CaptureData::RichText {
                text: "session fixture".into(),
                html: format!("<img src='file://{}'>", source.to_string_lossy()),
            },
            metadata: CaptureReadMetadata::default(),
        };
        let mut events = Events::default();
        assert!(
            matches!(CaptureEngine::capture_frame(&context, frame, &store, &mut events), CaptureOutcome::Failed(CaptureFailure { stage: CaptureStage::Prepare, ref code }) if code == "AttachmentReadRejected")
        );
        assert!(matches!(events.0.as_slice(), [CaptureEvent::Failed(_)]));
        assert!(session.lock().unwrap().is_empty());
        assert!(!directory.join("attachments").exists());
        drop(store);
        drop(state);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
