use super::source_exclusion::source_app_is_excluded;
use super::*;
use crate::database::{has_sensitive_tag, is_text_type};
use crate::privacy::{
    EvidenceLevel, EvidenceSource, PrivacyAction, PrivacyDecision, PrivacyEvidence, PrivacyPolicy,
    PrivacyReason,
};

const MAX_PERSISTED_TEXT_BYTES: usize = 10 * 1024 * 1024;

pub struct CaptureEngine;

impl CaptureEngine {
    /// Resolve a repeated file notification against its actual durable representation.
    pub fn file_history_exists(
        frame: &CaptureFrame,
        store: &dyn CaptureStore,
    ) -> Result<bool, CaptureFailure> {
        if !frame.metadata.files {
            return Ok(false);
        }
        if let Some(hash) = frame.metadata.image_content_hash {
            return store.contains_image_hash(hash);
        }
        if let CaptureData::Files(files) = &frame.data {
            return store.contains(&files.join("\n"));
        }
        Ok(false)
    }

    pub fn capture(
        context: &CaptureContext<'_>,
        reader: &mut dyn CaptureReader,
        store: &dyn CaptureStore,
        events: &mut dyn EventSink,
    ) -> CaptureOutcome {
        match Self::read(context, reader, events) {
            Ok(frame) => Self::capture_frame(context, frame, store, events),
            Err(outcome) => outcome,
        }
    }

    /// A listener may freeze a frame immediately and submit it to its existing worker.
    pub fn capture_frame(
        context: &CaptureContext<'_>,
        frame: CaptureFrame,
        store: &dyn CaptureStore,
        events: &mut dyn EventSink,
    ) -> CaptureOutcome {
        let outcome = process(context, frame, store);
        match &outcome {
            CaptureOutcome::Stored(capture) => {
                for id in &capture.removed_ids {
                    events.emit(CaptureEvent::Removed { id: *id });
                }
                events.emit(CaptureEvent::Stored {
                    id: capture.entry.id,
                    persistent: capture.persistent,
                });
                if let Some(id) = capture.ocr_target {
                    events.emit(CaptureEvent::OcrRequested { id });
                }
            }
            CaptureOutcome::Skipped(reason) => events.emit(CaptureEvent::Skipped(*reason)),
            CaptureOutcome::Rejected(reason) => events.emit(CaptureEvent::Rejected(*reason)),
            CaptureOutcome::Failed(error) => events.emit(CaptureEvent::Failed(error.clone())),
        }
        outcome
    }
    /// Source controls run before a reader can open or decode clipboard payloads.
    pub fn read(
        context: &CaptureContext<'_>,
        reader: &mut dyn CaptureReader,
        events: &mut dyn EventSink,
    ) -> Result<CaptureFrame, CaptureOutcome> {
        let skip = policy_skip(&source_decision(context));
        if let Some(reason) = skip {
            events.emit(CaptureEvent::Skipped(reason));
            return Err(CaptureOutcome::Skipped(reason));
        }
        match reader.read(CaptureReadRequest {
            files: context.settings.capture_files,
            rich_text: context.settings.capture_rich_text,
        }) {
            Ok(Some(data)) => Ok(CaptureFrame {
                data,
                metadata: reader.metadata(),
            }),
            Ok(None) => {
                let reason = reader.empty_reason();
                events.emit(CaptureEvent::Skipped(reason));
                Err(CaptureOutcome::Skipped(reason))
            }
            Err(error) => {
                events.emit(CaptureEvent::Failed(error.clone()));
                Err(CaptureOutcome::Failed(error))
            }
        }
    }
}

fn source_decision(context: &CaptureContext<'_>) -> PrivacyDecision {
    let mut evidence = Vec::new();
    if context.paused {
        evidence.push(PrivacyEvidence::new(
            EvidenceSource::UserControl,
            EvidenceLevel::Explicit,
            PrivacyReason::CapturePaused,
        ));
    }
    if source_app_is_excluded(
        &context.settings.excluded_source_apps,
        &context.source.app_name,
        context.source.process_path.as_deref(),
    ) {
        evidence.push(PrivacyEvidence::new(
            EvidenceSource::SourceFilter,
            EvidenceLevel::Explicit,
            PrivacyReason::SourceExcluded,
        ));
    }
    PrivacyPolicy.decide(evidence)
}

fn policy_skip(decision: &PrivacyDecision) -> Option<CaptureSkip> {
    if decision.action != PrivacyAction::Skip {
        return None;
    }
    Some(if decision.reason == PrivacyReason::CapturePaused {
        CaptureSkip::Paused
    } else {
        CaptureSkip::ExcludedSource
    })
}

fn failure(error: CaptureFailure) -> CaptureOutcome {
    match error.code.as_str() {
        "SensitivePersistenceRejected" => {
            CaptureOutcome::Rejected(CaptureRejection::SensitivePersistenceRejected)
        }
        "ProtectedRecordUnavailable" => {
            CaptureOutcome::Rejected(CaptureRejection::ProtectedRecordUnavailable)
        }
        "AttachmentBudgetExceeded" => {
            CaptureOutcome::Rejected(CaptureRejection::AttachmentBudgetExceeded)
        }
        _ => CaptureOutcome::Failed(error),
    }
}

fn process(
    context: &CaptureContext<'_>,
    frame: CaptureFrame,
    store: &dyn CaptureStore,
) -> CaptureOutcome {
    let read_image_hash = frame.metadata.image_content_hash;
    let initial_decision = source_decision(context);
    if let Some(reason) = policy_skip(&initial_decision) {
        return CaptureOutcome::Skipped(reason);
    }
    let mut evidence = initial_decision.evidence;
    let mut entry = match super::discovery::entry_from_capture(frame.data, context) {
        Ok(entry) => entry,
        Err(error) => return failure(error),
    };
    if context.sequential_echo.as_deref() == Some(&entry.content) {
        return CaptureOutcome::Skipped(CaptureSkip::SequentialEcho);
    }
    if context.settings.privacy_protection && is_text_type(&entry.content_type) {
        let matched = content::contains_sensitive_info(
            &entry.content,
            &context.settings.privacy_kinds,
            &context.settings.privacy_custom_rules,
        );
        if let Some(rule) = PrivacyPolicy::configured_rule(matched) {
            evidence.push(rule);
            entry.tags.push("sensitive".into());
        } else if entry.content.len() > 5000 {
            evidence.push(PrivacyEvidence::new(
                EvidenceSource::ConfiguredRule,
                EvidenceLevel::Incomplete,
                PrivacyReason::ScanIncomplete,
            ));
        }
    }
    if !context.settings.persistent {
        evidence.push(PrivacyEvidence::new(
            EvidenceSource::UserControl,
            EvidenceLevel::Explicit,
            PrivacyReason::SessionOnlyRequested,
        ));
    }
    let mut decision = PrivacyPolicy.decide(evidence.clone());
    let persistent = context.settings.persistent && decision.action != PrivacyAction::SessionOnly;
    if persistent
        && decision.action == PrivacyAction::Encrypt
        && crate::infrastructure::repository::protection::validate_sensitive_record(
            &entry.content_type,
            entry.is_external,
            entry.html_content.as_deref(),
        )
        .is_err()
    {
        return CaptureOutcome::Rejected(CaptureRejection::SensitivePersistenceRejected);
    }
    let mut prepared = if persistent {
        match store.prepare(entry) {
            Ok(prepared) => prepared,
            Err(error) => return failure(error),
        }
    } else {
        if let Some(html) = &entry.html_content {
            entry.html_content = Some(match content::embed_local_images(html) {
                Ok(html) => html,
                Err(code) => {
                    return failure(CaptureFailure {
                        stage: CaptureStage::Prepare,
                        code,
                    })
                }
            });
        }
        PreparedCapture::without_attachments(entry)
    };
    let entry = &mut prepared.entry;
    if entry.content_type == "image" {
        prepared.image_hash = read_image_hash;
    }
    if is_text_type(&entry.content_type)
        && entry
            .content
            .len()
            .saturating_add(entry.preview.len())
            .saturating_add(entry.html_content.as_deref().map_or(0, str::len))
            > MAX_PERSISTED_TEXT_BYTES
    {
        return CaptureOutcome::Rejected(CaptureRejection::Oversized);
    }
    if context.settings.deduplicate && persistent {
        prepared.image_hash = if entry.content_type == "image" {
            prepared
                .image_hash
                .or_else(|| crate::database::calc_image_hash(&entry.content))
        } else {
            None
        };
        let types: &[&str] = if entry.content_type == "rich_text" {
            &["rich_text", "text", "code", "url"]
        } else {
            &[&entry.content_type]
        };
        for content_type in types {
            let existing = match store.find_duplicate(entry, content_type, prepared.image_hash) {
                Ok(existing) => existing,
                Err(error) => return failure(error),
            };
            if let Some(existing) = existing {
                if entry.content_type == "rich_text"
                    && *content_type == "rich_text"
                    && (existing.content_type != "rich_text"
                        || existing.html_content != entry.html_content)
                {
                    continue;
                }
                entry.id = existing.id;
                let existing_protected = has_sensitive_tag(&existing.tags);
                for tag in existing.tags {
                    if !entry.tags.contains(&tag) {
                        entry.tags.push(tag);
                    }
                }
                if existing_protected {
                    evidence.push(PrivacyEvidence::new(
                        EvidenceSource::ExistingRecord,
                        EvidenceLevel::Explicit,
                        PrivacyReason::ExistingRecordProtected,
                    ));
                    decision = PrivacyPolicy.decide(evidence.clone());
                }
                break;
            }
        }
    }
    let mut pending_removals = Vec::new();
    let mut reused = None;
    if context.settings.deduplicate {
        let session = match context.session.lock() {
            Ok(session) => session,
            Err(_) => {
                return failure(CaptureFailure {
                    stage: CaptureStage::Deduplicate,
                    code: "CaptureSessionUnavailable".into(),
                })
            }
        };
        for item in session.iter() {
            let html_matches = entry.content_type != "rich_text"
                || item.content_type != "rich_text"
                || item.html_content == entry.html_content;
            if item.content == entry.content && html_matches {
                pending_removals.push(item.id);
                if !persistent {
                    reused = Some(item.clone());
                }
            }
        }
    }
    let new_image = entry.id == 0 && entry.content_type == "image";
    let mut removed_ids;
    if persistent {
        if decision.action == PrivacyAction::Encrypt
            && crate::infrastructure::repository::protection::validate_sensitive_record(
                &prepared.entry.content_type,
                prepared.entry.is_external,
                prepared.entry.html_content.as_deref(),
            )
            .is_err()
        {
            return CaptureOutcome::Rejected(CaptureRejection::SensitivePersistenceRejected);
        }
        let committed = match store.commit(&prepared) {
            Ok(committed) => committed,
            Err(error) => return failure(error),
        };
        if committed.id <= 0 {
            return failure(CaptureFailure {
                stage: CaptureStage::Persist,
                code: "InvalidStoredIdentity".into(),
            });
        }
        prepared.entry.id = committed.id;
        removed_ids = committed.removed_ids;
    } else {
        let entry = &mut prepared.entry;
        if let Some(previous) = reused {
            if has_sensitive_tag(&previous.tags) {
                evidence.push(PrivacyEvidence::new(
                    EvidenceSource::ExistingRecord,
                    EvidenceLevel::Explicit,
                    PrivacyReason::ExistingRecordProtected,
                ));
                decision = PrivacyPolicy.decide(evidence.clone());
            }
            entry.id = previous.id;
            entry.is_pinned = previous.is_pinned;
            entry.pinned_order = previous.pinned_order;
            entry.use_count = previous.use_count.saturating_add(1);
            for tag in previous.tags {
                if !entry.tags.contains(&tag) {
                    entry.tags.push(tag);
                }
            }
        } else {
            entry.id = next_session_id(context.timestamp);
        }
        removed_ids = Vec::new();
    }
    pending_removals.retain(|id| *id != prepared.entry.id);
    {
        let mut session = context
            .session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        session.retain(|item| !pending_removals.contains(&item.id));
        if !persistent {
            if let Some(previous) = session.iter_mut().find(|item| item.id == prepared.entry.id) {
                *previous = prepared.entry.clone();
            } else {
                session.push_back(prepared.entry.clone());
            }
            if session.len() > 500 {
                if let Some(removed) = session.pop_front() {
                    removed_ids.push(removed.id);
                }
            }
        }
    }
    removed_ids.extend(pending_removals);
    removed_ids.sort_unstable();
    removed_ids.dedup();
    let ocr_target =
        (persistent && new_image && context.settings.ocr_enabled).then_some(prepared.entry.id);
    CaptureOutcome::Stored(StoredCapture {
        entry: prepared.entry,
        persistent,
        removed_ids,
        ocr_target,
        privacy: decision,
    })
}

fn next_session_id(timestamp: i64) -> i64 {
    use std::sync::atomic::{AtomicI64, Ordering};
    static PREVIOUS: AtomicI64 = AtomicI64::new(0);
    let candidate = timestamp.saturating_abs().saturating_neg().min(-1);
    let previous = PREVIOUS
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |previous| {
            Some(candidate.min(previous.saturating_sub(1)))
        })
        .expect("the session ID update always supplies a value");
    candidate.min(previous.saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    struct Store {
        preparations: Cell<usize>,
        commits: Cell<usize>,
        committed: Cell<bool>,
        fail: bool,
        preparation_error: Option<CaptureFailure>,
    }

    impl Default for Store {
        fn default() -> Self {
            Self {
                preparations: Cell::new(0),
                commits: Cell::new(0),
                committed: Cell::new(false),
                fail: false,
                preparation_error: None,
            }
        }
    }

    impl CaptureStore for Store {
        fn prepare(
            &self,
            entry: crate::domain::models::ClipboardEntry,
        ) -> Result<PreparedCapture, CaptureFailure> {
            self.preparations.set(self.preparations.get() + 1);
            if let Some(error) = &self.preparation_error {
                return Err(error.clone());
            }
            Ok(PreparedCapture::without_attachments(entry))
        }
        fn find_duplicate(
            &self,
            _: &crate::domain::models::ClipboardEntry,
            _: &str,
            _: Option<i64>,
        ) -> Result<Option<CaptureDuplicate>, CaptureFailure> {
            Ok(None)
        }
        fn contains(&self, _: &str) -> Result<bool, CaptureFailure> {
            Ok(false)
        }
        fn commit(&self, _: &PreparedCapture) -> Result<CaptureCommit, CaptureFailure> {
            self.commits.set(self.commits.get() + 1);
            if self.fail {
                return Err(CaptureFailure {
                    stage: CaptureStage::Persist,
                    code: "CaptureStorageFailed".into(),
                });
            }
            self.committed.set(true);
            Ok(CaptureCommit {
                id: 42,
                removed_ids: vec![7],
            })
        }
    }

    struct Reader {
        requests: Vec<CaptureReadRequest>,
        result: Result<Option<CaptureData>, CaptureFailure>,
    }

    impl CaptureReader for Reader {
        fn read(
            &mut self,
            request: CaptureReadRequest,
        ) -> Result<Option<CaptureData>, CaptureFailure> {
            self.requests.push(request);
            std::mem::replace(&mut self.result, Ok(None))
        }
    }

    #[derive(Default)]
    struct Events(Vec<CaptureEvent>);

    impl EventSink for Events {
        fn emit(&mut self, event: CaptureEvent) {
            self.0.push(event);
        }
    }

    fn context(
        session: &Mutex<VecDeque<crate::domain::models::ClipboardEntry>>,
    ) -> CaptureContext<'_> {
        CaptureContext {
            source: CaptureSource {
                app_name: "Editor.exe".into(),
                process_path: None,
            },
            timestamp: 1,
            settings: CaptureSettings::default(),
            session,
            paused: false,
            sequential_echo: None,
        }
    }

    #[test]
    fn pause_and_excluded_source_never_read_a_payload() {
        let session = Mutex::new(VecDeque::new());
        for paused in [false, true] {
            let mut context = context(&session);
            context.paused = paused;
            context.settings.excluded_source_apps = vec!["editor.exe".into()];
            let mut reader = Reader {
                requests: Vec::new(),
                result: Ok(Some(CaptureData::Text("private".into()))),
            };
            let mut events = Events::default();
            let reason = if paused {
                CaptureSkip::Paused
            } else {
                CaptureSkip::ExcludedSource
            };
            assert!(
                matches!(CaptureEngine::read(&context, &mut reader, &mut events), Err(CaptureOutcome::Skipped(value)) if value == reason)
            );
            assert!(reader.requests.is_empty());
            assert_eq!(events.0, vec![CaptureEvent::Skipped(reason)]);
        }
    }

    #[test]
    fn enabled_formats_are_requested_once_and_errors_have_no_success_event() {
        let session = Mutex::new(VecDeque::new());
        let mut context = context(&session);
        context.settings.capture_rich_text = true;
        let failure = CaptureFailure {
            stage: CaptureStage::Read,
            code: "ClipboardReadUnavailable".into(),
        };
        let mut reader = Reader {
            requests: Vec::new(),
            result: Err(failure.clone()),
        };
        let mut events = Events::default();
        assert!(
            matches!(CaptureEngine::read(&context, &mut reader, &mut events), Err(CaptureOutcome::Failed(value)) if value == failure)
        );
        assert_eq!(reader.requests.len(), 1);
        assert!(!reader.requests[0].files);
        assert!(reader.requests[0].rich_text);
        assert_eq!(events.0, vec![CaptureEvent::Failed(failure)]);
    }

    fn frame(data: CaptureData) -> CaptureFrame {
        CaptureFrame {
            data,
            metadata: CaptureReadMetadata::default(),
        }
    }

    #[test]
    fn echo_and_unsupported_sensitive_html_stop_before_preparation_or_commit() {
        let session = Mutex::new(VecDeque::new());
        let mut context = context(&session);
        context.settings.persistent = true;
        context.sequential_echo = Some("queue echo".into());
        let store = Store::default();
        let mut events = Events::default();
        assert!(matches!(
            CaptureEngine::capture_frame(
                &context,
                frame(CaptureData::Text("queue echo".into())),
                &store,
                &mut events
            ),
            CaptureOutcome::Skipped(CaptureSkip::SequentialEcho)
        ));
        context.settings.privacy_protection = true;
        context.settings.privacy_kinds = vec!["email".into()];
        assert!(matches!(
            CaptureEngine::capture_frame(
                &context,
                frame(CaptureData::RichText {
                    text: "private@example.com".into(),
                    html: "<img src='file:///private.png'>".into()
                }),
                &store,
                &mut events
            ),
            CaptureOutcome::Rejected(CaptureRejection::SensitivePersistenceRejected)
        ));
        assert_eq!(store.preparations.get(), 0);
        assert_eq!(store.commits.get(), 0);
        assert_eq!(
            events.0,
            vec![
                CaptureEvent::Skipped(CaptureSkip::SequentialEcho),
                CaptureEvent::Rejected(CaptureRejection::SensitivePersistenceRejected)
            ]
        );
    }

    #[test]
    fn failed_commit_has_no_stored_removal_or_ocr_event() {
        let session = Mutex::new(VecDeque::new());
        let mut context = context(&session);
        context.settings.persistent = true;
        context.settings.ocr_enabled = true;
        let store = Store {
            fail: true,
            ..Store::default()
        };
        let mut events = Events::default();
        let outcome = CaptureEngine::capture_frame(
            &context,
            frame(CaptureData::RgbaImage {
                width: 1,
                height: 1,
                bytes: vec![0, 0, 0, 255],
            }),
            &store,
            &mut events,
        );
        assert!(matches!(
            outcome,
            CaptureOutcome::Failed(CaptureFailure {
                stage: CaptureStage::Persist,
                ..
            })
        ));
        assert!(matches!(events.0.as_slice(), [CaptureEvent::Failed(_)]));
        assert!(!store.committed.get());
        assert!(session.lock().unwrap().is_empty());
    }

    #[test]
    fn attachment_budget_rejection_propagates_without_commit_or_success_event() {
        let session = Mutex::new(VecDeque::new());
        let mut context = context(&session);
        context.settings.persistent = true;
        let store = Store {
            preparation_error: Some(CaptureFailure {
                stage: CaptureStage::Prepare,
                code: "AttachmentBudgetExceeded".into(),
            }),
            ..Store::default()
        };
        let mut events = Events::default();
        let outcome = CaptureEngine::capture_frame(
            &context,
            frame(CaptureData::RichText {
                text: "ordinary".into(),
                html: "<p>ordinary</p>".into(),
            }),
            &store,
            &mut events,
        );
        assert!(matches!(
            outcome,
            CaptureOutcome::Rejected(CaptureRejection::AttachmentBudgetExceeded)
        ));
        assert_eq!(
            events.0,
            vec![CaptureEvent::Rejected(
                CaptureRejection::AttachmentBudgetExceeded
            )]
        );
        assert_eq!(store.commits.get(), 0);
        assert!(session.lock().unwrap().is_empty());
    }

    #[test]
    fn stored_events_follow_commit_and_ocr_is_requested_for_saved_new_images_only() {
        struct CommittedEvents<'a> {
            committed: &'a Cell<bool>,
            events: Vec<CaptureEvent>,
        }
        impl EventSink for CommittedEvents<'_> {
            fn emit(&mut self, event: CaptureEvent) {
                assert!(self.committed.get());
                self.events.push(event);
            }
        }
        let session = Mutex::new(VecDeque::new());
        let mut context = context(&session);
        context.settings.persistent = true;
        context.settings.ocr_enabled = true;
        let store = Store::default();
        let mut events = CommittedEvents {
            committed: &store.committed,
            events: Vec::new(),
        };
        let outcome = CaptureEngine::capture_frame(
            &context,
            frame(CaptureData::RgbaImage {
                width: 1,
                height: 1,
                bytes: vec![0, 0, 0, 255],
            }),
            &store,
            &mut events,
        );
        assert!(matches!(
            outcome,
            CaptureOutcome::Stored(StoredCapture {
                ocr_target: Some(42),
                ..
            })
        ));
        assert_eq!(
            events.events,
            vec![
                CaptureEvent::Removed { id: 7 },
                CaptureEvent::Stored {
                    id: 42,
                    persistent: true
                },
                CaptureEvent::OcrRequested { id: 42 }
            ]
        );
    }

    #[test]
    fn session_reuse_preserves_manual_fields_and_equal_timestamps_get_unique_ids() {
        let session = Mutex::new(VecDeque::new());
        let mut context = context(&session);
        let store = Store::default();
        for _ in 0..3 {
            assert!(matches!(
                CaptureEngine::capture_frame(
                    &context,
                    frame(CaptureData::Text("same".into())),
                    &store,
                    &mut ()
                ),
                CaptureOutcome::Stored(_)
            ));
        }
        let ids: std::collections::HashSet<i64> = session
            .lock()
            .unwrap()
            .iter()
            .map(|entry| entry.id)
            .collect();
        assert_eq!(ids.len(), 3);
        assert!(ids.iter().all(|id| *id < 0));
        {
            let mut entries = session.lock().unwrap();
            let entry = entries.back_mut().unwrap();
            entry.is_pinned = true;
            entry.pinned_order = 9;
            entry.tags = vec!["work".into(), "sensitive".into()];
            entry.use_count = 4;
        }
        context.settings.deduplicate = true;
        let CaptureOutcome::Stored(capture) = CaptureEngine::capture_frame(
            &context,
            frame(CaptureData::Text("same".into())),
            &store,
            &mut (),
        ) else {
            panic!("session capture should store");
        };
        assert!(capture.entry.is_pinned);
        assert_eq!(capture.entry.pinned_order, 9);
        assert_eq!(capture.entry.use_count, 5);
        assert_eq!(capture.entry.tags, vec!["work", "sensitive"]);
        assert_eq!(session.lock().unwrap().len(), 1);
        assert_eq!(store.preparations.get(), 0);
        assert_eq!(store.commits.get(), 0);
    }
}
