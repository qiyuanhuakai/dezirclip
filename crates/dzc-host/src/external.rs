use crate::{
    router::{now_ms, privacy_error},
    Host, Session,
};
use base64::{engine::general_purpose, Engine as _};
use dzc_core::api::{
    CaptureTicket, ClipboardEntry, HistoryService, PrivacyAccessIntent, PrivacyReadPermit,
    SensitiveAccess,
};
use dzc_platform::runtime::PrivateFileExport;
use dzc_protocol::wire::{EventData, EventKind, ExternalOpenParams, WireErrorCode};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Arc, Mutex, Weak,
    },
    time::{Duration, Instant, SystemTime},
};

pub fn edit_watch_ticket(
    history: &dzc_core::api::HistoryService,
    permit: &dzc_core::api::PrivacyReadPermit,
    id: i64,
    content_type: &str,
    now: i64,
) -> Result<Option<dzc_core::api::CaptureTicket>, dzc_core::api::PrivacyError> {
    history.privacy().check_read(permit, now)?;
    if id <= 0 || !matches!(content_type, "image" | "text" | "rich_text" | "code") {
        return Ok(None);
    }
    match history.privacy().capture_ticket(now) {
        Ok(ticket) if ticket.epoch() == permit.epoch() => Ok(Some(ticket)),
        Ok(_) => Err(dzc_core::api::PrivacyError::PrivacyEpochChanged),
        Err(dzc_core::api::PrivacyError::PrivacySessionActive) => Ok(None),
        Err(error) => Err(error),
    }
}

pub fn complete_external_open(
    history: &dzc_core::api::HistoryService,
    id: i64,
    permit: &dzc_core::api::PrivacyReadPermit,
    launched: Result<(), WireErrorCode>,
    now: i64,
) -> Result<(), WireErrorCode> {
    launched?;
    history
        .privacy()
        .check_read(permit, now)
        .map_err(privacy_error)?;
    if id > 0 {
        history
            .increment_use_count(id)
            .map_err(|_| WireErrorCode::Unavailable)?;
    }
    Ok(())
}

fn changed_bytes(bytes: &[u8], content_type: &str) -> Option<(String, String)> {
    let content = if content_type == "image" {
        if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
            format!(
                "data:image/gif;base64,{}",
                general_purpose::STANDARD.encode(bytes)
            )
        } else {
            let png = dzc_platform::image_png::as_png_bytes(bytes).ok()?;
            format!(
                "data:image/png;base64,{}",
                general_purpose::STANDARD.encode(png)
            )
        }
    } else {
        String::from_utf8(bytes.to_vec()).ok()?
    };
    let preview = generate_preview(&content);
    Some((content, preview))
}

fn generate_preview(content: &str) -> String {
    if content.starts_with("data:image") {
        "[New Image]".to_string()
    } else if content.chars().count() > 500 {
        let preview_text: String = content.chars().take(497).collect();
        format!("{}...", preview_text.replace('\n', " "))
    } else {
        content.replace('\n', " ")
    }
}

struct Edit {
    _slot: EditSlot,
    lease: PrivateFileExport,
    kind: String,
    id: i64,
    ticket: CaptureTicket,
    modified: Option<SystemTime>,
    expires: Instant,
}

struct EditSlot(Arc<AtomicUsize>);
impl Drop for EditSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Release);
    }
}

pub(crate) struct ExternalEdits {
    owner: Weak<Host>,
    operation: Mutex<()>,
    sender: Mutex<Option<mpsc::Sender<Edit>>>,
    count: Arc<AtomicUsize>,
}

impl ExternalEdits {
    pub(crate) fn new(owner: Weak<Host>) -> Self {
        Self {
            owner,
            operation: Mutex::new(()),
            sender: Mutex::new(None),
            count: Default::default(),
        }
    }

    fn reserve(&self) -> Result<(EditSlot, mpsc::Sender<Edit>), WireErrorCode> {
        // Only the serialized open operation can reserve, while the watcher releases.
        if self.count.load(Ordering::Acquire) >= 32 {
            return Err(WireErrorCode::Busy);
        }
        self.count.fetch_add(1, Ordering::Relaxed);
        let reservation = EditSlot(self.count.clone());
        let mut slot = self.sender.lock().map_err(|_| WireErrorCode::Unavailable)?;
        if let Some(sender) = &*slot {
            return Ok((reservation, sender.clone()));
        }
        let (sender, receiver) = mpsc::channel();
        let owner = self.owner.clone();
        std::thread::Builder::new()
            .name("content-file-watcher".into())
            .spawn(move || watch_edits(owner, receiver))
            .map_err(|_| WireErrorCode::Unavailable)?;
        *slot = Some(sender.clone());
        Ok((reservation, sender))
    }
}

fn watch_edits(owner: Weak<Host>, receiver: mpsc::Receiver<Edit>) {
    let mut edits = Vec::new();
    loop {
        if edits.is_empty() {
            match receiver.recv() {
                Ok(edit) => edits.push(edit),
                Err(_) => return,
            }
        }
        match receiver.recv_timeout(Duration::from_secs(2)) {
            Ok(edit) => edits.push(edit),
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let Some(host) = owner.upgrade() else { return };
        if host.is_stopping() {
            return;
        }
        edits.retain_mut(|edit| {
            if Instant::now() >= edit.expires || !edit.lease.is_current() {
                return false;
            }
            let modified = edit.lease.modified(0).ok();
            if modified != edit.modified {
                edit.modified = modified;
                if let Some((content, preview)) = edit
                    .lease
                    .read(0)
                    .ok()
                    .and_then(|bytes| changed_bytes(&bytes, &edit.kind))
                {
                    let Ok(now) = now_ms() else { return false };
                    if host
                        .history
                        .update_content_authorized(edit.id, &content, &preview, &edit.ticket, now)
                        .is_ok()
                    {
                        let _ = host.publish(
                            EventKind::EntryUpdated,
                            EventData {
                                entry_id: Some(edit.id),
                                status: None,
                            },
                        );
                    }
                }
            }
            true
        });
    }
}

struct OpenedFile {
    lease: PrivateFileExport,
    modified_before_launch: Option<SystemTime>,
}

fn launch_entry(
    history: &HistoryService,
    row: &ClipboardEntry,
    permit: &PrivacyReadPermit,
    authorize: impl Fn() -> Result<(), WireErrorCode>,
) -> Result<Option<OpenedFile>, WireErrorCode> {
    let kind = row.content_type.as_str();
    let selected = history.default_application(kind).map_err(|_| WireErrorCode::Unavailable)?;
    let handler = dzc_platform::apps::resolve_handler(selected.as_deref());
    let check = || {
        authorize()?;
        history
            .privacy()
            .check_read(permit, now_ms()?)
            .map_err(privacy_error)
    };
    if matches!(kind, "link" | "url") {
        check()?;
        dzc_platform::apps::open_url(&handler, &row.content)
            .map_err(|_| WireErrorCode::Unavailable)?;
        return Ok(None);
    }
    let export = if row.id == 0 {
        history.generated_external_files_authorized(row, permit, now_ms()?)
    } else {
        history.external_files_authorized(row.id, permit, now_ms()?)
    }
    .map_err(privacy_error)?;
    let lease = dzc_platform::runtime::private_file_export(export)
        .map_err(|_| WireErrorCode::Unavailable)?;
    let path = lease
        .paths()
        .map_err(|_| WireErrorCode::Unavailable)?
        .into_iter()
        .next()
        .ok_or(WireErrorCode::Unavailable)?;
    let modified_before_launch = lease.modified(0).ok();
    check()?;
    dzc_platform::apps::open_file(&handler, &path).map_err(|_| WireErrorCode::Unavailable)?;
    Ok(Some(OpenedFile {
        lease,
        modified_before_launch,
    }))
}

impl Host {
    pub(crate) fn open_history_item(
        &self,
        session: &Session,
        p: ExternalOpenParams,
    ) -> Result<(), WireErrorCode> {
        self.check_library_state(session, &p.service_instance_id, p.expected_epoch)?;
        let _operation = self
            .external
            .operation
            .try_lock()
            .map_err(|_| WireErrorCode::Busy)?;
        let sensitive = if p.reveal {
            SensitiveAccess::ExplicitUserAction
        } else {
            SensitiveAccess::Denied
        };
        let permit = self
            .history
            .privacy()
            .authorize_selected_copy(p.id, None, sensitive, Some(p.expected_epoch), now_ms()?)
            .map_err(privacy_error)?;
        let row = self
            .history
            .get_entry_authorized(p.id, &permit, now_ms()?)
            .map_err(privacy_error)?
            .ok_or(WireErrorCode::SensitiveAccessDenied)?;
        let monitor = edit_watch_ticket(&self.history, &permit, p.id, &row.content_type, now_ms()?)
            .map_err(privacy_error)?
            .map(|ticket| self.external.reserve().map(|watch| (ticket, watch)))
            .transpose()?;
        let authorize = || {
            if self.is_stopping() {
                return Err(WireErrorCode::Busy);
            }
            if !self.grants.still_valid(session) {
                return Err(WireErrorCode::PermissionDenied);
            }
            Ok(())
        };
        let opened = launch_entry(&self.history, &row, &permit, authorize)?;
        authorize()?;
        complete_external_open(&self.history, p.id, &permit, Ok(()), now_ms()?)?;
        if let Some(file) = opened {
            if let Some((ticket, (slot, sender))) = monitor {
                sender
                    .send(Edit {
                        _slot: slot,
                        lease: file.lease,
                        modified: file.modified_before_launch,
                        id: p.id,
                        kind: row.content_type,
                        ticket,
                        expires: Instant::now() + Duration::from_secs(300),
                    })
                    .map_err(|_| WireErrorCode::Unavailable)?;
            } else {
                file.lease.retain();
            }
        }
        self.publish(
            EventKind::EntryUpdated,
            EventData {
                entry_id: Some(p.id),
                status: None,
            },
        )
    }
}

// Legacy caller-created previews have no saved row to watch or increment.
pub fn open_generated(
    history: &HistoryService,
    content: &str,
    kind: &str,
    epoch: Option<u64>,
) -> Result<(), WireErrorCode> {
    let permit = history
        .privacy()
        .authorize_read(
            PrivacyAccessIntent::GeneratedCopy,
            SensitiveAccess::Denied,
            epoch,
            now_ms()?,
        )
        .map_err(privacy_error)?;
    let row = serde_json::from_value(serde_json::json!({ "id":0,"content_type":kind,"content":content,"preview":"","source_app":"Manual","timestamp":now_ms()?,"is_pinned":false }))
        .map_err(|_| WireErrorCode::InvalidRequest)?;
    let opened = launch_entry(history, &row, &permit, || Ok(()))?;
    complete_external_open(history, 0, &permit, Ok(()), now_ms()?)?;
    if let Some(file) = opened {
        file.lease.retain();
    }
    Ok(())
}
