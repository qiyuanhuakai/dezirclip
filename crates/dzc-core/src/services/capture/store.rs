use super::{CaptureFailure, CaptureStage, CaptureSource};
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
    pub(crate) privacy_ticket: Option<crate::api::CaptureTicket>,
    attachments: Option<HtmlAttachmentPlan>,
}

impl PreparedCapture {
    pub fn without_attachments(entry: ClipboardEntry) -> Self {
        Self {
            entry,
            image_hash: None,
            privacy_ticket: None,
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
    /// Adapters must explicitly preserve the authoritative source policy.
    fn privacy_source(&self,source:&CaptureSource)->Result<crate::privacy::PrivacyDecision,CaptureFailure>;
    /// A database adapter returns its actual policy report; None is an explicit in-memory policy.
    fn assess_privacy_entry(&self,entry:&ClipboardEntry)->Result<Option<crate::api::PrivacyDecisionReport>,CaptureFailure>;

    fn capture_ticket(
        &self,
        _now_ms: i64,
    ) -> Result<Option<crate::api::CaptureTicket>, CaptureFailure> {
        Ok(None)
    }
    fn check_capture(
        &self,
        ticket: Option<&crate::api::CaptureTicket>,
        _now_ms: i64,
    ) -> Result<(), CaptureFailure> {
        if ticket.is_some() {
            return Err(failure(CaptureStage::Persist, "PrivacyStateUnavailable"));
        }
        Ok(())
    }
    fn with_capture_guard(
        &self,
        ticket: Option<&crate::api::CaptureTicket>,
        now_ms: i64,
        publish: &mut dyn FnMut(),
    ) -> Result<(), CaptureFailure> {
        self.check_capture(ticket, now_ms)?;
        publish();
        Ok(())
    }
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
    /// Only adapters with an actual committed storage receipt return Some.
    /// Existing external stores retain unknown protection through the default.
    fn commit_with_protection(&self, capture: &PreparedCapture) -> Result<(CaptureCommit, Option<bool>), CaptureFailure> {
        self.commit(capture).map(|commit| (commit, None))
    }
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
        | "AttachmentBudgetExceeded" | "PrivacyRulesUnavailable" | "PrivacyRuleLimitExceeded" | "PrivacySettingsInvalid" | "PrivacySourceExcluded" => error,
        "PrivacyStateUnavailable" | "PrivacyEpochChanged" | "PrivacySessionActive" => error,
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
    fn privacy_source(&self,source:&CaptureSource)->Result<crate::privacy::PrivacyDecision,CaptureFailure>{
        let conn=self.connection.lock().map_err(|_|failure(CaptureStage::Read,"PrivacyStateUnavailable"))?;
        crate::infrastructure::repository::privacy_policy::assess_source(&conn,source).map_err(|error|failure(CaptureStage::Read,error))
    }
    fn assess_privacy_entry(&self,entry:&ClipboardEntry)->Result<Option<crate::api::PrivacyDecisionReport>,CaptureFailure>{
        self.repository.assess_privacy_entry(entry,true).map(Some).map_err(|error|failure(CaptureStage::Prepare,error))
    }

    fn capture_ticket(
        &self,
        now_ms: i64,
    ) -> Result<Option<crate::api::CaptureTicket>, CaptureFailure> {
        self.repository
            .privacy_service()
            .capture_ticket(now_ms)
            .map(Some)
            .map_err(|error| failure(CaptureStage::Read, error))
    }

    fn check_capture(
        &self,
        ticket: Option<&crate::api::CaptureTicket>,
        now_ms: i64,
    ) -> Result<(), CaptureFailure> {
        let ticket =
            ticket.ok_or_else(|| failure(CaptureStage::Persist, "PrivacyStateUnavailable"))?;
        self.repository
            .privacy_service()
            .check_capture(ticket, now_ms)
            .map_err(|error| failure(CaptureStage::Persist, error))
    }

    fn with_capture_guard(
        &self,
        ticket: Option<&crate::api::CaptureTicket>,
        now_ms: i64,
        publish: &mut dyn FnMut(),
    ) -> Result<(), CaptureFailure> {
        let ticket =
            ticket.ok_or_else(|| failure(CaptureStage::Persist, "PrivacyStateUnavailable"))?;
        let conn = self
            .connection
            .lock()
            .map_err(|_| failure(CaptureStage::Persist, "PrivacyStateUnavailable"))?;
        let tx =
            rusqlite::Transaction::new_unchecked(&conn, rusqlite::TransactionBehavior::Immediate)
                .map_err(|_| failure(CaptureStage::Persist, "PrivacyStateUnavailable"))?;
        crate::api::privacy_session::check_capture_with_conn(&tx, ticket, now_ms)
            .map_err(|error| failure(CaptureStage::Persist, error))?;
        publish();
        tx.commit()
            .map_err(|_| failure(CaptureStage::Persist, "PrivacyStateUnavailable"))
    }

    fn prepare(&self, mut entry: ClipboardEntry) -> Result<PreparedCapture, CaptureFailure> {
        let privacy_ticket = self.capture_ticket(chrono::Utc::now().timestamp_millis())?;
        let attachments = if crate::database::has_sensitive_tag(&entry.tags) { None } else { entry
            .html_content
            .as_deref()
            .map(|html| plan_html_attachments(html, self.directory))
            .transpose()
            .map_err(|error| failure(CaptureStage::Prepare, error))? };
        if let Some(plan) = &attachments {
            entry.html_content = Some(plan.html().to_string());
        }
        Ok(PreparedCapture {
            entry,
            attachments,
            image_hash: None,
            privacy_ticket,
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
        self.commit_with_protection(capture).map(|(commit, _)| commit)
    }
    fn commit_with_protection(&self, capture: &PreparedCapture) -> Result<(CaptureCommit, Option<bool>), CaptureFailure> {
        let connection = self
            .connection
            .lock()
            .map_err(|error| failure(CaptureStage::Persist, error))?;
        let (id, protected) = self
            .repository
            .save_automatic_capture_with_protection(
                &connection,
                &capture.entry,
                Some(self.directory),
                capture.image_hash,
                capture.attachments.as_ref(),
                capture
                    .privacy_ticket
                    .as_ref()
                    .ok_or_else(|| failure(CaptureStage::Persist, "PrivacyStateUnavailable"))?,
                chrono::Utc::now().timestamp_millis(),
            )
            .map_err(|error| failure(CaptureStage::Persist, error))?;
        let removed_ids = self
            .repository
            .enforce_limit_with_conn(&connection, Some(self.directory))
            .unwrap_or_default();
        Ok((CaptureCommit { id, removed_ids }, Some(protected)))
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

    fn authorized_frame(context: &CaptureContext<'_>, mut frame: CaptureFrame, store: &dyn CaptureStore, events: &mut dyn EventSink) -> CaptureOutcome {
        if frame.metadata.privacy_ticket.is_none() {
            frame.metadata.privacy_ticket = store.capture_ticket(chrono::Utc::now().timestamp_millis()).unwrap();
        }
        CaptureEngine::capture_frame(context, frame, store, events)
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
    fn real_capture_mixed_html_survives_summary_and_thumbnail_reads() {
        use base64::Engine as _;
        use crate::api::{HistoryQuery, PrivacyAccessIntent, SensitiveAccess};
        let (directory, state) = database();
        state.conn.lock().unwrap().execute("INSERT OR REPLACE INTO settings(key,value) VALUES('app.privacy_protection','false')", []).unwrap();
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(2, 2, image::Rgba([31, 71, 113, 255])))
            .write_to(&mut png, image::ImageFormat::Png).unwrap();
        let data = format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png.into_inner()));
        let html = format!("<p>Known mixed text</p><img src='{data}'><!--TIEZ_RICH_IMAGE:{data}-->");
        let session = Default::default();
        let context = context(&session);
        let store = state.capture_store(&directory);
        let frame = CaptureFrame { data: CaptureData::RichText { text: "Known mixed text".into(), html }, metadata: CaptureReadMetadata::default() };
        let CaptureOutcome::Stored(captured) = authorized_frame(&context, frame, &store, &mut ()) else { panic!("mixed capture should commit"); };
        let history = crate::api::HistoryService::open(&directory.join("clipboard.db")).unwrap();
        let summaries = history.query(&HistoryQuery { limit: 50, metadata: true, ..Default::default() }).unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].id, captured.entry.id);
        assert_eq!(summaries[0].content, "");
        assert_eq!(summaries[0].html_content.as_deref(), Some(""));
        let now = chrono::Utc::now().timestamp_millis();
        let permit = history.privacy().authorize_read(PrivacyAccessIntent::InteractiveRead, SensitiveAccess::Denied, None, now).unwrap();
        for _ in 0..2 {
            let preview = history.get_entry_thumbnail_authorized(captured.entry.id, &permit, now, 560).unwrap().unwrap();
            assert_eq!(preview.content, "Known mixed text");
            let html = preview.html_content.unwrap();
            assert!(html.contains("<p>Known mixed text</p>"));
            assert!(html.contains("<img src='data:image/png;base64,"));
            assert!(html.contains("<!--TIEZ_RICH_IMAGE:data:image/png;base64,"));
            assert!(!html.contains(directory.to_string_lossy().as_ref()));
            let copy = history.privacy().authorize_selected_copy(captured.entry.id, None, SensitiveAccess::Denied, Some(permit.epoch()), now).unwrap();
            let crate::ports::ClipboardPayload::Html { text, html } = history.clipboard_payload_authorized(captured.entry.id, &copy, now).unwrap()
                else { panic!("mixed record must keep its authorized HTML export"); };
            assert_eq!(text, "Known mixed text");
            assert!(html.contains("<p>Known mixed text</p>"));
            assert!(html.contains("<img src="));
            history.increment_use_count(captured.entry.id).unwrap();
        }
        drop(store); drop(history); drop(state);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn real_capture_deduplicates_stable_html_assets_and_preserves_existing_ocr() {
        let (directory, state) = database();
        let session = Mutex::new(VecDeque::new());
        let context = context(&session);
        let store = state.capture_store(&directory);
        let CaptureOutcome::Stored(first) =
            authorized_frame(&context, rich_frame(), &store, &mut ())
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
            authorized_frame(&context, rich_frame(), &store, &mut ())
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
    fn rich_text_exceptions_match_planned_ram_images_without_premature_materialization() {
        use crate::api::{PrivacyAccessIntent, PrivacyFeedbackAction, SensitiveAccess};
        use crate::infrastructure::repository::clipboard_repo::ClipboardRepository;
        use rusqlite::functions::FunctionFlags;
        use std::sync::atomic::{AtomicUsize, Ordering};
        for matches_exception in [false, true] {
            let (directory, state) = database();
            state.conn.lock().unwrap().execute("INSERT OR REPLACE INTO settings(key,value) VALUES('app.privacy_protection','false')",[]).unwrap();
            let image = |pixel: u8| {
                let mut bytes = std::io::Cursor::new(Vec::new());
                image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
                    2,
                    2,
                    image::Rgba([pixel, 17, 23, 255]),
                ))
                .write_to(&mut bytes, image::ImageFormat::Png)
                .unwrap();
                bytes.into_inner()
            };
            let png = image(37);
            let source = directory.join("synthetic-original.png");
            std::fs::write(&source, &png).unwrap();
            let html = format!(
                "<p>freshordinarycapture</p><img src='file://{}'>",
                source.to_string_lossy().replace('\\', "/")
            );
            let seed_html = if matches_exception {
                html.clone()
            } else {
                "<p>different exception without images</p>".into()
            };
            let seed_text = if matches_exception {
                "freshordinarycapture"
            } else {
                "different exception"
            };
            let seed:ClipboardEntry=serde_json::from_value(serde_json::json!({
                "id":0,"content_type":"rich_text","content":seed_text,"html_content":seed_html,
                "preview":"synthetic","source_app":"Editor","timestamp":1,"is_pinned":false,"tags":["sensitive"]
            })).unwrap();
            let id = state.repo.save(&seed, None).unwrap();
            let permission = state
                .repo
                .privacy_service()
                .authorize_read(
                    PrivacyAccessIntent::InteractiveRead,
                    SensitiveAccess::ExplicitUserAction,
                    None,
                    1,
                )
                .unwrap();
            state
                .repo
                .privacy_feedback(
                    id,
                    None,
                    PrivacyFeedbackAction::ExactException,
                    &permission,
                    1,
                )
                .unwrap();
            assert!(!directory.join("attachments").exists());
            let inspected = Arc::new(AtomicUsize::new(0));
            let inspected_sql = inspected.clone();
            let attachments = directory.join("attachments");
            state
                .conn
                .lock()
                .unwrap()
                .create_scalar_function(
                    "fixture_no_early_assets",
                    0,
                    FunctionFlags::SQLITE_UTF8,
                    move |_| {
                        inspected_sql.fetch_add(1, Ordering::SeqCst);
                        Ok(!attachments.exists())
                    },
                )
                .unwrap();
            state.conn.lock().unwrap().execute_batch("CREATE TRIGGER inspect_planned_capture BEFORE INSERT ON clipboard_history BEGIN SELECT CASE WHEN fixture_no_early_assets()=0 THEN RAISE(ABORT,'premature synthetic materialization') END; END;").unwrap();
            let session = Mutex::new(VecDeque::new());
            let mut context = context(&session);
            context.settings.capture_rich_text = true;
            context.settings.deduplicate = false;
            let store = state.capture_store(&directory);
            let planned=store.prepare(serde_json::from_value(serde_json::json!({
                "id":0,"content_type":"rich_text","content":"freshordinarycapture","html_content":html.clone(),
                "preview":"synthetic","source_app":"Editor","timestamp":1,"is_pinned":false,"tags":[]
            })).unwrap()).unwrap();
            assert!(!directory.join("attachments").exists());
            assert_eq!(
                state
                    .repo
                    .assess_privacy_entry(&planned.entry, true)
                    .unwrap_err(),
                "AttachmentReadRejected"
            );
            let frame = |text: &str, html: String| CaptureFrame {
                data: CaptureData::RichText {
                    text: text.into(),
                    html,
                },
                metadata: CaptureReadMetadata::default(),
            };
            let outcome = authorized_frame(
                &context,
                frame("freshordinarycapture", html),
                &store,
                &mut (),
            );
            let CaptureOutcome::Stored(stored) = outcome else {
                panic!("valid RAM attachment plan must commit with a same-source rich-text exception: {outcome:?}")
            };
            assert_eq!(inspected.load(Ordering::SeqCst), 1);
            assert_eq!(
                stored.privacy.reason,
                if matches_exception {
                    crate::privacy::PrivacyReason::ExactExceptionMatched
                } else {
                    crate::privacy::PrivacyReason::NoProtectionMatch
                }
            );
            assert!(!crate::database::has_sensitive_tag(&stored.entry.tags));
            let paths: Vec<_> = std::fs::read_dir(directory.join("attachments"))
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect();
            assert_eq!(paths.len(), 1);
            assert_eq!(std::fs::read(&paths[0]).unwrap(), png);
            let conn = state.conn.lock().unwrap();
            let raw: String = conn
                .query_row(
                    "SELECT html_content FROM clipboard_history WHERE id=?1",
                    [stored.entry.id],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(raw.contains("attachments"));
            assert!(!raw.contains("data:image/"));
            conn.execute_batch("DROP TRIGGER inspect_planned_capture; PRAGMA foreign_keys=ON;
                CREATE TABLE reject_planned_capture(history_id INTEGER REFERENCES clipboard_history(id) DEFERRABLE INITIALLY DEFERRED);
                CREATE TRIGGER reject_planned_commit AFTER INSERT ON clipboard_history BEGIN INSERT INTO reject_planned_capture VALUES(-999); END;").unwrap();
            drop(conn);
            let other = directory.join("synthetic-second.png");
            std::fs::write(&other, image(73)).unwrap();
            let html = format!(
                "<p>failedordinarycapture</p><img src='file://{}'>",
                other.to_string_lossy().replace('\\', "/")
            );
            let mut events = Events::default();
            assert!(matches!(
                authorized_frame(
                    &context,
                    frame("failedordinarycapture", html),
                    &store,
                    &mut events
                ),
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
                2
            );
            assert_eq!(
                std::fs::read_dir(directory.join("attachments"))
                    .unwrap()
                    .count(),
                1
            );
            assert_eq!(std::fs::read(&paths[0]).unwrap(), png);
            assert!(source.exists());
            assert!(other.exists());
            drop(store);
            drop(state);
            std::fs::remove_dir_all(directory).unwrap();
        }
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
            authorized_frame(&context, rich_frame(), &store, &mut events),
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
            authorized_frame(&context, rich_frame(), &store, &mut events),
            CaptureOutcome::Failed(CaptureFailure { code, .. }) if code == "AttachmentPersistenceRejected"
        ));
        assert!(matches!(events.0.as_slice(),[CaptureEvent::Failed(CaptureFailure { code, .. })] if code=="AttachmentPersistenceRejected"));
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
            authorized_frame(&context, frame, &store, &mut events),
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
    fn protected_session_files_keep_complete_bytes_in_ram_and_old_tickets_cannot_publish() {
        let (directory, state) = database();
        let source = directory.join("synthetic.txt");
        std::fs::write(&source, b"synthetic session private bytes").unwrap();
        let session = Mutex::new(VecDeque::new());
        let mut context = context(&session);
        context.settings.persistent = false;
        let store = state.capture_store(&directory);
        let frame = |ticket| CaptureFrame {
            data: CaptureData::Files(vec![source.to_string_lossy().into_owned()]),
            metadata: CaptureReadMetadata {
                privacy_ticket: Some(ticket),
                inherited_protection: true,
                ..Default::default()
            },
        };
        let ticket = store.capture_ticket(1).unwrap().unwrap();
        let result = CaptureEngine::capture_frame(&context, frame(ticket), &store, &mut ());
        let CaptureOutcome::Stored(result) = result else {
            panic!("complete protected session archive should publish")
        };
        assert!(result.entry.id < 0);
        assert!(crate::database::has_sensitive_tag(&result.entry.tags));
        let decoded =
            crate::infrastructure::protected_resources::decode_files(&result.entry.content)
                .unwrap();
        assert_eq!(decoded[0].bytes, b"synthetic session private bytes");
        assert!(!result.entry.is_external);
        assert!(!directory.join("attachments").exists());
        let old = store.capture_ticket(1).unwrap().unwrap();
        state
            .repo
            .privacy_service()
            .set_session(
                crate::api::PrivacySessionCommand::On { duration_ms: None },
                2,
            )
            .unwrap();
        state
            .repo
            .privacy_service()
            .set_session(crate::api::PrivacySessionCommand::Off, 3)
            .unwrap();
        assert!(matches!(
            CaptureEngine::capture_frame(&context, frame(old), &store, &mut ()),
            CaptureOutcome::Skipped(CaptureSkip::PrivacyEpochChanged)
        ));
        assert_eq!(session.lock().unwrap().len(), 1);
        assert_eq!(
            std::fs::read(&source).unwrap(),
            b"synthetic session private bytes"
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
            authorized_frame(&context, frame(image.clone()), &store, &mut ())
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
            authorized_frame(&context, frame(image.repeat(65)), &store, &mut events),
            CaptureOutcome::Rejected(CaptureRejection::AttachmentBudgetExceeded)
        ));
        std::fs::File::create(&source)
            .unwrap()
            .set_len(16 * 1024 * 1024 + 1)
            .unwrap();
        assert!(matches!(
            authorized_frame(&context, frame(image), &store, &mut events),
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
            matches!(authorized_frame(&context, frame, &store, &mut events), CaptureOutcome::Failed(CaptureFailure { stage: CaptureStage::Prepare, ref code }) if code == "AttachmentReadRejected")
        );
        assert!(matches!(events.0.as_slice(), [CaptureEvent::Failed(_)]));
        assert!(session.lock().unwrap().is_empty());
        assert!(!directory.join("attachments").exists());
        drop(store);
        drop(state);
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn shared_session_stops_before_metadata_or_payload_and_old_frames_never_resume() {
        struct Reader {
            calls: usize,
        }
        impl CaptureReader for Reader {
            fn preflight_privacy(&mut self) -> Result<PrivacyPreflight, CaptureFailure> {
                self.calls += 1;
                Ok(PrivacyPreflight::default())
            }
            fn read(
                &mut self,
                _: CaptureReadRequest,
            ) -> Result<Option<CaptureData>, CaptureFailure> {
                self.calls += 1;
                Ok(Some(CaptureData::Text("synthetic body".into())))
            }
        }
        let (directory, state) = database();
        let other = crate::api::HistoryService::open(&directory.join("clipboard.db")).unwrap();
        let store = state.capture_store(&directory);
        let session = Mutex::new(VecDeque::new());
        let context = context(&session);
        let now = chrono::Utc::now().timestamp_millis();
        let old_ticket = store.capture_ticket(now).unwrap().unwrap();
        other
            .privacy()
            .set_session(
                crate::api::PrivacySessionCommand::On { duration_ms: None },
                now,
            )
            .unwrap();
        let mut reader = Reader { calls: 0 };
        let mut events = Events::default();
        assert!(matches!(
            CaptureEngine::capture(&context, &mut reader, &store, &mut events),
            CaptureOutcome::Skipped(CaptureSkip::PrivacySession)
        ));
        assert_eq!(reader.calls, 0);
        other
            .privacy()
            .set_session(crate::api::PrivacySessionCommand::Off, now)
            .unwrap();
        let mut frame = rich_frame();
        frame.metadata.privacy_ticket = Some(old_ticket);
        assert!(matches!(
            authorized_frame(&context, frame, &store, &mut events),
            CaptureOutcome::Skipped(CaptureSkip::PrivacyEpochChanged)
        ));
        assert_eq!(
            events.0,
            vec![
                CaptureEvent::Skipped(CaptureSkip::PrivacySession),
                CaptureEvent::Skipped(CaptureSkip::PrivacyEpochChanged)
            ]
        );
        assert!(session.lock().unwrap().is_empty());
        assert!(!directory.join("attachments").exists());
        assert_eq!(other.get_count().unwrap(), 0);
        drop(store);
        drop(state);
        drop(other);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn prepared_attachment_commit_checks_epoch_under_the_write_transaction() {
        let (directory, state) = database();
        let other = crate::api::HistoryService::open(&directory.join("clipboard.db")).unwrap();
        let store = state.capture_store(&directory);
        let entry: ClipboardEntry = serde_json::from_value(serde_json::json!({"id":0,"content_type":"rich_text","content":"ordinary capture","preview":"ordinary","html_content":"<p>ordinary</p><!--TIEZ_RICH_IMAGE:data:image/png;base64,b3RoZXI=-->","source_app":"test","timestamp":1,"is_pinned":false})).unwrap();
        let prepared = store.prepare(entry).unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        other
            .privacy()
            .set_session(
                crate::api::PrivacySessionCommand::On { duration_ms: None },
                now,
            )
            .unwrap();
        other
            .privacy()
            .set_session(crate::api::PrivacySessionCommand::Off, now)
            .unwrap();
        assert!(
            matches!(store.commit(&prepared), Err(CaptureFailure { ref code, .. }) if code == "PrivacyEpochChanged")
        );
        assert_eq!(other.get_count().unwrap(), 0);
        let conn = state.conn.lock().unwrap();
        assert_eq!(
            conn.query_row("SELECT count(*) FROM entry_tags", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row("SELECT count(*) FROM clipboard_fts_docsize", [], |row| row
                .get::<_, i64>(
                0
            ))
            .unwrap(),
            0
        );
        assert!(!directory.join("attachments").exists());
        drop(conn);
        drop(store);
        drop(state);
        drop(other);
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn unleased_automatic_frame_cannot_acquire_a_new_epoch_at_commit_time() {
        let (directory, state) = database();
        let store = state.capture_store(&directory);
        let session = Mutex::new(VecDeque::new());
        let context = context(&session);
        let outcome = CaptureEngine::capture_frame(&context, rich_frame(), &store, &mut ());
        assert!(matches!(outcome, CaptureOutcome::Failed(CaptureFailure { ref code, .. }) if code == "PrivacyCaptureTicketMissing"));
        assert!(!directory.join("attachments").exists());
        drop(store);
        drop(state);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn malformed_pem_prefixes_cannot_hide_a_later_valid_key_from_persistence_protection() {
        // This checks parsing and transactional persistence, independently of OS scheduling.
        crate::privacy::with_scan_clock(|| std::time::Duration::ZERO, || {
        use base64::Engine;
        for public_key in [false, true] {
        let mut der = vec![0x30, 46, 2, 1, 0, 0x30, 5, 6, 3, 0x2b, 0x65, 0x70, 4, 34, 4, 32];
        der.extend([7; 32]);
        if public_key { der[4]=1;der[1]+=35;der.extend([0x81,33,0]);der.extend([9;32]); }
        let valid = format!("-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----", base64::engine::general_purpose::STANDARD.encode(der));
        for prefix in ["-----BEGIN PRIVATE KEY-----\nY29udGVudA==\n-----END PRIVATE KEY-----\n", "-----BEGIN PRIVATE KEY-----\nmissing terminator\n", "-----BEGIN PRIVATE KEY-----\n%%%invalid%%%\n-----END PRIVATE KEY-----\n"] {
            let (directory, state) = database();
            let store = state.capture_store(&directory);
            let session = Mutex::new(VecDeque::new());
            let mut context = context(&session);
            context.settings.privacy_protection = true;
            let text = format!("{prefix}{valid}");
            let frame = CaptureFrame { data: CaptureData::Text(text.clone()), metadata: Default::default() };
            let CaptureOutcome::Stored(capture) = authorized_frame(&context, frame, &store, &mut ()) else { panic!("structured key must persist under protection"); };
            assert_eq!(capture.privacy.action, crate::privacy::PrivacyAction::Encrypt, "capture decision: {:?}", capture.privacy);
            let conn = state.conn.lock().unwrap();
            let stored: String = conn.query_row("SELECT content FROM clipboard_history WHERE id=?1", [capture.entry.id], |row| row.get(0)).unwrap();
            assert!(crate::infrastructure::encryption::has_ciphertext_envelope(&stored));
            assert_eq!(crate::infrastructure::encryption::decode_for_mutation(&stored).unwrap(), text);
            assert_eq!(conn.query_row("SELECT count(*) FROM clipboard_fts_docsize", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
            drop(conn); drop(store); drop(state);
            std::fs::remove_dir_all(directory).unwrap();
        }
        }
        });
    }
    #[test]
    fn committed_protection_is_reflected_when_the_initial_scan_budget_expires() {
        use base64::Engine;
        use std::{cell::Cell,time::Duration};
        thread_local! {static CLOCK_CALLS: Cell<usize> = const {Cell::new(0)};}
        fn clock() -> Duration {CLOCK_CALLS.with(|calls| {let count=calls.get();calls.set(count+1);if count==0 {Duration::from_millis(6)} else {Duration::ZERO}})}
        CLOCK_CALLS.with(|calls|calls.set(0));
        crate::privacy::with_scan_clock(clock, || {
            let (directory,state)=database();let store=state.capture_store(&directory);
            let session=Mutex::new(VecDeque::new());let context=context(&session);
            let mut der=vec![0x30,46,2,1,0,0x30,5,6,3,0x2b,0x65,0x70,4,34,4,32];der.extend([7;32]);
            let text=format!("-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----",base64::engine::general_purpose::STANDARD.encode(der));
            let frame=CaptureFrame{data:CaptureData::Text(text),metadata:Default::default()};
            let CaptureOutcome::Stored(capture)=authorized_frame(&context,frame,&store,&mut ()) else {panic!("capture must commit");};
            let conn=state.conn.lock().unwrap();
            let (content,tags):(String,String)=conn.query_row("SELECT content,tags FROM clipboard_history WHERE id=?1",[capture.entry.id],|row|Ok((row.get(0)?,row.get(1)?))).unwrap();
            assert!(crate::infrastructure::encryption::has_ciphertext_envelope(&content));
            assert!(tags.contains("sensitive"));
            assert!(capture.privacy.evidence.iter().any(|item|item.reason==crate::privacy::PrivacyReason::ScanIncomplete));
            assert_eq!(capture.privacy.action,crate::privacy::PrivacyAction::Encrypt,"first assessment expired, commit stored ciphertext; capture report: {:?}",capture.privacy);
            assert!(crate::database::has_sensitive_tag(&capture.entry.tags));
            drop(conn);drop(store);drop(state);std::fs::remove_dir_all(directory).unwrap();
        });
    }

}
