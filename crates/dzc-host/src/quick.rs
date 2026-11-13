use crate::{Host, Session};
use dzc_platform::runtime::{QueueInputTarget, RememberedFocus};
use dzc_protocol::wire::*;
use serde_json::Value;
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

pub(super) const TIMEOUT: Duration = Duration::from_secs(120);
struct FocusTicket {
    token: String,
    state: RuntimeState,
    target: RememberedFocus,
    expires: Instant,
}
#[derive(Default)]
pub(super) struct FocusSlot(Mutex<Option<FocusTicket>>);
impl FocusSlot {
    pub(super) fn active(&self) -> bool {
        self.0.lock().is_ok_and(|mut slot| {
            if slot
                .as_ref()
                .is_some_and(|item| item.expires <= Instant::now())
            {
                *slot = None;
            }
            slot.is_some()
        })
    }
}

impl Host {
    pub(crate) fn quick_request(
        &self,
        session: &Session,
        method: Method,
        params: Value,
    ) -> Result<Value, WireErrorCode> {
        let invalid = |_| WireErrorCode::InvalidRequest;
        let instance = params["service_instance_id"]
            .as_str()
            .ok_or(WireErrorCode::InvalidRequest)?;
        let epoch = params["expected_epoch"]
            .as_u64()
            .ok_or(WireErrorCode::InvalidRequest)?;
        self.check_library_state(session, instance, epoch)?;
        session.require(Permission::ClipboardPaste)?;
        let mut slot = session
            .quick
            .0
            .lock()
            .map_err(|_| WireErrorCode::Unavailable)?;
        if slot
            .as_ref()
            .is_some_and(|t| t.expires <= Instant::now() || t.state.privacy_epoch != epoch)
        {
            *slot = None;
        }
        if method == Method::DesktopQuickBegin {
            if slot.is_some() {
                return Err(WireErrorCode::Busy);
            }
            let state = self.snapshot()?;
            let target = RememberedFocus::capture(
                session
                    .peer
                    .as_ref()
                    .ok_or(WireErrorCode::PermissionDenied)?,
            )
            .map_err(|_| WireErrorCode::Unavailable)?;
            if !crate::same_surface_state(&state, &self.snapshot()?) {
                return Err(WireErrorCode::HistoryRevisionChanged);
            }
            let token = uuid::Uuid::new_v4().to_string();
            *slot = Some(FocusTicket {
                token: token.clone(),
                state: state.clone(),
                target,
                expires: Instant::now() + TIMEOUT,
            });
            return Ok(serde_json::json!(QuickTicket { token, state }));
        }
        if slot.as_ref().map(|t| t.token.as_str()) != params["token"].as_str() {
            return Err(WireErrorCode::NotFound);
        }
        let ticket = slot.take().ok_or(WireErrorCode::NotFound)?;
        drop(slot);
        match method {
            Method::DesktopQuickDismiss => {
                let p: QuickDismissParams = serde_json::from_value(params).map_err(invalid)?;
                if p.restore {
                    ticket
                        .target
                        .restore()
                        .map_err(|_| WireErrorCode::Unavailable)?;
                }
                Ok(serde_json::json!(OperationResult { performed: true }))
            }
            Method::DesktopQuickPaste => {
                let p: QuickPasteParams = serde_json::from_value(params).map_err(invalid)?;
                let current = self.snapshot()?;
                if current.privacy_epoch != epoch {
                    return Err(WireErrorCode::PrivacyEpochChanged);
                }
                if current.history_revision != p.expected_history_revision {
                    return Err(WireErrorCode::HistoryRevisionChanged);
                }
                let target = ticket
                    .target
                    .restore()
                    .map_err(|_| WireErrorCode::Unavailable)?;
                let bound =
                    QueueInputTarget::prepare(target).map_err(|_| WireErrorCode::Unavailable)?;
                if !crate::same_surface_state(&current, &self.snapshot()?) {
                    return Err(WireErrorCode::HistoryRevisionChanged);
                }
                self.desktop_copy_bound(
                    session,
                    DesktopCopyParams {
                        id: p.id,
                        service_instance_id: p.service_instance_id,
                        expected_epoch: epoch,
                        reveal: false,
                        paste: true,
                        with_format: None,
                        derived_text: None,
                        image_as_base64: None,
                        // Quick paste matches the Tauri panel: selecting a row
                        // preserves history even when main-window paste consumes it.
                        delete_after_use: Some(false),
                        move_to_top: Some(true),
                        expected_paste_target: None,
                    },
                    Some((bound, current)),
                )
            }
            _ => Err(WireErrorCode::InvalidRequest),
        }
    }
}
