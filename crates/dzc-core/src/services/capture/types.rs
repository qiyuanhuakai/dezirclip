use crate::domain::models::ClipboardEntry;
use std::collections::VecDeque;
use std::sync::Mutex;

/// A payload is read only after the source and capture controls permit it.
#[derive(Debug)]
pub enum CaptureData {
    Text(String),
    RichText {
        text: String,
        html: String,
    },
    Image {
        data_url: String,
    },
    EncodedImage {
        bytes: Vec<u8>,
        mime: String,
    },
    RgbaImage {
        width: u32,
        height: u32,
        bytes: Vec<u8>,
    },
    Files(Vec<String>),
}

#[derive(Debug, Clone, Default)]
pub struct CaptureSource {
    pub app_name: String,
    pub process_path: Option<String>,
}

/// A consistent per-capture snapshot, independent of a desktop settings store.
#[derive(Debug, Clone, Default)]
pub struct CaptureSettings {
    pub persistent: bool,
    pub deduplicate: bool,
    pub capture_files: bool,
    pub capture_rich_text: bool,
    pub privacy_protection: bool,
    pub privacy_kinds: Vec<String>,
    pub privacy_custom_rules: Vec<String>,
    pub excluded_source_apps: Vec<String>,
    pub ocr_enabled: bool,
}

pub struct CaptureContext<'a> {
    pub source: CaptureSource,
    pub timestamp: i64,
    pub settings: CaptureSettings,
    pub session: &'a Mutex<VecDeque<ClipboardEntry>>,
    pub paused: bool,
    pub sequential_echo: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct CaptureReadRequest {
    pub files: bool,
    pub rich_text: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct CaptureReadMetadata {
    /// Issued before reading the payload; the repository rechecks it inside commit.
    pub privacy_ticket: Option<crate::api::CaptureTicket>,
    pub privacy_hints_checked: bool,
    pub fingerprint: Option<u64>,
    pub echo_hash: Option<u64>,
    pub image: bool,
    pub files: bool,
    /// Durable dedup identity for a decoded image whose clipboard source was a file.
    pub image_content_hash: Option<i64>,
}

#[derive(Debug)]
pub struct CaptureFrame {
    pub data: CaptureData,
    pub metadata: CaptureReadMetadata,
}

/// Native adapters choose formats and decode them; Core requests only enabled data.
pub trait CaptureReader {
    fn preflight_privacy(&mut self) -> Result<PrivacyPreflight, CaptureFailure> {
        Ok(PrivacyPreflight::default())
    }
    fn selection_id(&self) -> Option<u64> {
        None
    }
    fn read(&mut self, request: CaptureReadRequest) -> Result<Option<CaptureData>, CaptureFailure>;

    fn metadata(&self) -> CaptureReadMetadata {
        CaptureReadMetadata::default()
    }

    fn empty_reason(&self) -> CaptureSkip {
        CaptureSkip::Empty
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PrivacyHintAvailability {
    Checked,
    #[default]
    Unavailable,
}

#[derive(Debug, Clone, Default)]
pub struct PrivacyPreflight {
    pub hints: Vec<crate::ports::PrivacyHint>,
    pub availability: PrivacyHintAvailability,
    pub selection_id: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureSkip {
    PrivacySession,
    ExplicitPrivacyHint,
    PrivacyEpochChanged,
    Paused,
    ExcludedSource,
    Empty,
    FilesDisabled,
    SequentialEcho,
    DuplicateNotification,
    OwnClipboard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureRejection {
    Oversized,
    SensitivePersistenceRejected,
    ProtectedRecordUnavailable,
    AttachmentBudgetExceeded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureStage {
    Read,
    Prepare,
    Deduplicate,
    Persist,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureFailure {
    pub stage: CaptureStage,
    pub code: String,
}

#[derive(Debug)]
pub struct StoredCapture {
    pub entry: ClipboardEntry,
    pub persistent: bool,
    pub removed_ids: Vec<i64>,
    pub ocr_target: Option<i64>,
    pub privacy: crate::privacy::PrivacyDecision,
}

#[derive(Debug)]
pub enum CaptureOutcome {
    Stored(StoredCapture),
    Skipped(CaptureSkip),
    Rejected(CaptureRejection),
    Failed(CaptureFailure),
}

/// Events contain metadata only. A trusted caller receives the stored entry in the result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureEvent {
    Stored { id: i64, persistent: bool },
    Removed { id: i64 },
    OcrRequested { id: i64 },
    Skipped(CaptureSkip),
    Rejected(CaptureRejection),
    Failed(CaptureFailure),
}

pub trait EventSink {
    fn emit(&mut self, event: CaptureEvent);
}

impl EventSink for () {
    fn emit(&mut self, _: CaptureEvent) {}
}
