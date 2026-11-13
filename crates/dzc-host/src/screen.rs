use crate::{router::{now_ms, privacy_error}, Host, Session};
use base64::{engine::general_purpose::STANDARD, Engine};
use dzc_core::api::{CaptureTicket, PrivacyAccessIntent, PrivacyError, PrivacyReadPermit, SensitiveAccess};
use dzc_platform::screenshot::{freeze_at, FrozenScreenFrame, MonitorRect};
use dzc_protocol::wire::*;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::{sync::{Arc, Mutex}, time::{Duration, Instant}};
use zeroize::Zeroizing;

pub(crate) const SELECTION_TIMEOUT: Duration = Duration::from_secs(120);
const CHUNK: usize = 192 * 1024;
const MAX_PNG: usize = 160 * 1024 * 1024;

pub(crate) fn supported() -> bool {
    if dzc_platform::runtime::is_native_wayland() { return false; }
    #[cfg(target_os = "linux")]
    if dzc_platform::runtime_display::validate_x11_display().is_err() { return false; }
    true
}

struct Selection {
    owner: String,
    token: String,
    deadline: Instant,
    timestamp: i64,
    permit: PrivacyReadPermit,
    ticket: Option<CaptureTicket>,
    rect: MonitorRect,
    frame: Option<Arc<FrozenScreenFrame>>,
    png: Zeroizing<Vec<u8>>,
}

impl Selection {
    fn info(&self) -> ScreenImage {
        ScreenImage { token: self.token.clone(), x: self.rect.x, y: self.rect.y, width: self.rect.width,
            height: self.rect.height, png_bytes: self.png.len() as u32 }
    }
}

#[derive(Default)]
struct Slot {
    current: Option<Selection>,
    timer: bool,
}

#[derive(Default)]
pub(crate) struct Screens(Arc<Mutex<Slot>>, Mutex<()>);

impl Screens {
    pub(crate) fn clear(&self) -> Result<(), WireErrorCode> {
        self.0.lock().map_err(|_| WireErrorCode::Unavailable)?.current = None;
        Ok(())
    }

    pub(crate) fn owns(&self, owner: &str) -> bool {
        self.0.lock().is_ok_and(|slot| slot.current.as_ref().is_some_and(|s| s.owner == owner && s.deadline > Instant::now()))
    }

    pub(crate) fn disconnect(&self, owner: &str) {
        if let Ok(mut slot) = self.0.lock() {
            if slot.current.as_ref().is_some_and(|s| s.owner == owner) { slot.current = None; }
        }
    }

    fn install(&self, selection: Selection) -> Result<ScreenImage, WireErrorCode> {
        let mut slot = self.0.lock().map_err(|_| WireErrorCode::Unavailable)?;
        if slot.current.as_ref().is_some_and(|s| s.deadline > Instant::now()) { return Err(WireErrorCode::Busy); }
        let info = selection.info();
        slot.current = Some(selection);
        if !slot.timer {
            let state = self.0.clone();
            let timer = std::thread::Builder::new().name("dzc-screen-expiry".into()).spawn(move || loop {
                let sleep = {
                    let Ok(mut slot) = state.lock() else { return; };
                    match slot.current.as_ref() {
                        Some(frame) if frame.deadline > Instant::now() => frame.deadline.saturating_duration_since(Instant::now()),
                        _ => { slot.current = None; slot.timer = false; return; }
                    }
                };
                std::thread::sleep(sleep);
            });
            if timer.is_err() { slot.current = None; return Err(WireErrorCode::Unavailable); }
            slot.timer = true;
        }
        Ok(info)
    }
}

fn decode<T: DeserializeOwned>(params: Value) -> Result<T, WireErrorCode> {
    serde_json::from_value(params).map_err(|_| WireErrorCode::InvalidRequest)
}
fn encode(value: impl Serialize) -> Result<Value, WireErrorCode> {
    serde_json::to_value(value).map_err(|_| WireErrorCode::Internal)
}

impl Host {
    pub(crate) fn screen_request(&self, session: &Session, method: Method, params: Value) -> Result<Value, WireErrorCode> {
        let instance = params["service_instance_id"].as_str().ok_or(WireErrorCode::InvalidRequest)?;
        let epoch = params["expected_epoch"].as_u64().ok_or(WireErrorCode::InvalidRequest)?;
        self.check_library_state(session, instance, epoch)?;
        if !supported() { return Err(WireErrorCode::UnsupportedCapability); }
        if method == Method::DesktopScreenBegin {
            let request: ScreenBeginParams = decode(params)?;
            return self.begin_screen(session, epoch, || {
                freeze_at(request.cursor.map(|p| (p.x, p.y))).map_err(|error| {
                    eprintln!("Screen capture failed: {error}");
                    WireErrorCode::Unavailable
                })
            }).and_then(encode);
        }
        let token = params["token"].as_str().ok_or(WireErrorCode::InvalidRequest)?.to_owned();
        let mut slot = self.screen.0.lock().map_err(|_| WireErrorCode::Unavailable)?;
        let selection = slot.current.as_mut().filter(|s| s.owner == session.id() && s.token == token && s.deadline > Instant::now())
            .ok_or(WireErrorCode::NotFound)?;
        self.history.privacy().check_read(&selection.permit, now_ms()?).map_err(privacy_error)?;
        match method {
            Method::DesktopScreenRead => {
                let request: ScreenReadParams = decode(params)?;
                let offset = request.offset as usize;
                if offset >= selection.png.len() { return Err(WireErrorCode::InvalidRequest); }
                let end = (offset + CHUNK).min(selection.png.len());
                let data_base64 = STANDARD.encode(&selection.png[offset..end]);
                self.history.privacy().check_read(&selection.permit, now_ms()?).map_err(privacy_error)?;
                encode(ScreenChunk { data_base64, next_offset: end as u32, finished: end == selection.png.len() })
            }
            Method::DesktopScreenCrop => {
                let p: ScreenCropParams = decode(params)?;
                let frame = selection.frame.as_ref().ok_or(WireErrorCode::InvalidRequest)?.clone();
                let permit = selection.permit.clone();
                drop(slot);
                // Encoding is CPU work, not ownership state. Privacy revocation,
                // disconnect and other clients' idle reads must not wait for it.
                let crop = frame.crop(p.x, p.y, p.width, p.height)
                    .map_err(|_| WireErrorCode::InvalidRequest)?;
                let png = Zeroizing::new(crop.png_bytes);
                if png.len() > MAX_PNG { return Err(WireErrorCode::FrameTooLarge); }
                self.history.privacy().check_read(&permit, now_ms()?).map_err(privacy_error)?;
                let mut slot = self.screen.0.lock().map_err(|_| WireErrorCode::Unavailable)?;
                let selection = slot.current.as_mut().filter(|s| s.owner == session.id() && s.token == token && s.deadline > Instant::now())
                    .ok_or(WireErrorCode::NotFound)?;
                selection.rect = MonitorRect { x: p.x, y: p.y, width: p.width, height: p.height, is_primary: selection.rect.is_primary };
                selection.frame = None;
                selection.png = png;
                selection.token = uuid::Uuid::new_v4().to_string();
                encode(selection.info())
            }
            Method::DesktopScreenCancel => {
                slot.current = None;
                encode(OperationResult { performed: true })
            }
            Method::DesktopScreenFinish => {
                if !self.actions.can_copy() { return Err(WireErrorCode::UnsupportedCapability); }
                let selection = slot.current.take().ok_or(WireErrorCode::NotFound)?;
                drop(slot);
                let body = GeneratedBody { kind: "image".into(), text: format!("data:image/png;base64,{}", STANDARD.encode(&*selection.png)), html: None };
                let recorded = self.commit_generated_copy(body, "DezirClip Screenshot", selection.timestamp, selection.ticket, &selection.permit)?;
                encode(ScreenFinishResult { copied: true, recorded })
            }
            _ => Err(WireErrorCode::InvalidRequest),
        }
    }

    fn begin_screen(&self, session: &Session, epoch: u64, freeze: impl FnOnce() -> Result<FrozenScreenFrame, WireErrorCode>) -> Result<ScreenImage, WireErrorCode> {
        // Reserve the expensive capture without blocking ownership or status
        // reads. Concurrent clients must not allocate multiple monitor frames.
        let _capture = self.screen.1.try_lock().map_err(|error| match error {
            std::sync::TryLockError::WouldBlock => WireErrorCode::Busy,
            std::sync::TryLockError::Poisoned(_) => WireErrorCode::Unavailable,
        })?;
        let timestamp = now_ms()?;
        let privacy = self.history.privacy();
        let permit = privacy.authorize_read(PrivacyAccessIntent::GeneratedCopy, SensitiveAccess::Denied, Some(epoch), timestamp).map_err(privacy_error)?;
        let ticket = match privacy.capture_ticket(timestamp) {
            Ok(ticket) if ticket.epoch() == epoch => Some(ticket),
            Ok(_) => return Err(WireErrorCode::PrivacyEpochChanged),
            Err(PrivacyError::PrivacySessionActive) => None,
            Err(error) => return Err(privacy_error(error)),
        };
        if self.screen.0.lock().map_err(|_| WireErrorCode::Unavailable)?.current.as_ref().is_some_and(|s| s.deadline > Instant::now()) {
            return Err(WireErrorCode::Busy);
        }
        let frame = freeze()?;
        let png = Zeroizing::new(frame.png_bytes().map_err(|_| WireErrorCode::Unavailable)?);
        if png.len() > MAX_PNG { return Err(WireErrorCode::FrameTooLarge); }
        privacy.check_read(&permit, now_ms()?).map_err(privacy_error)?;
        self.screen.install(Selection { owner: session.id().into(), token: uuid::Uuid::new_v4().to_string(),
            deadline: Instant::now() + SELECTION_TIMEOUT, timestamp, permit, ticket, rect: frame.bounds(), frame: Some(Arc::new(frame)), png })
    }
}
