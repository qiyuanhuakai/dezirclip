use crate::{
    router::{now_ms, privacy_error},
    Host, Session,
};
use dzc_core::api::SensitiveAccess;
use dzc_core::ports::ClipboardPayload;
use dzc_protocol::wire::*;
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

const LIMIT: usize = 32768;

#[derive(Default)]
struct PasteQueueState {
    revision: uuid::Uuid,
    pub items: VecDeque<i64>,
    pub last_action_was_paste: bool,
    pub last_pasted_content: Option<String>,
    pub version: std::sync::Arc<()>,
    pub in_flight: Option<std::sync::Arc<()>>,
}
impl PasteQueueState {
    pub fn reset_tracking(&mut self) {
        self.revision = uuid::Uuid::new_v4();
        self.last_action_was_paste = false;
        self.last_pasted_content = None;
        self.version = std::sync::Arc::new(());
    }
}

#[derive(Default)]
pub(crate) struct Queue {
    state: Arc<Mutex<PasteQueueState>>,
}

fn reset_paste_tracking(queue: &mut PasteQueueState) {
    queue.reset_tracking();
}

#[derive(Clone)]
struct QueueStep {
    id: i64,
    version: std::sync::Arc<()>,
    token: std::sync::Arc<()>,
}
fn reserve_step(queue: &mut PasteQueueState) -> Result<Option<QueueStep>, &'static str> {
    if queue.in_flight.is_some() {
        return Err("PasteQueueBusy");
    }
    let Some(id) = queue.items.front().copied() else {
        return Ok(None);
    };
    let token = std::sync::Arc::new(());
    queue.in_flight = Some(token.clone());
    Ok(Some(QueueStep {
        id,
        version: queue.version.clone(),
        token,
    }))
}
fn current_step(queue: &PasteQueueState, step: &QueueStep) -> bool {
    std::sync::Arc::ptr_eq(&queue.version, &step.version)
        && queue
            .in_flight
            .as_ref()
            .is_some_and(|token| std::sync::Arc::ptr_eq(token, &step.token))
        && queue.items.front() == Some(&step.id)
}
fn consume_step(queue: &mut PasteQueueState, step: &QueueStep, missing: bool) -> Option<bool> {
    if !current_step(queue, step) {
        return None;
    }
    queue.items.pop_front();
    queue.revision = uuid::Uuid::new_v4();
    if missing || queue.items.is_empty() {
        reset_paste_tracking(queue);
    }
    Some(queue.items.is_empty())
}
fn release_step(queue: &mut PasteQueueState, step: &QueueStep) {
    if queue
        .in_flight
        .as_ref()
        .is_some_and(|token| std::sync::Arc::ptr_eq(token, &step.token))
    {
        queue.in_flight = None;
    }
}
struct QueueStepGuard {
    queue: std::sync::Arc<std::sync::Mutex<PasteQueueState>>,
    step: QueueStep,
}
impl QueueStepGuard {
    fn skip_missing(&self) {
        if let Ok(mut queue) = self.queue.lock() {
            skip_missing_step(&mut queue, &self.step);
        }
    }
    fn current(&self) -> bool {
        self.queue
            .lock()
            .map(|queue| current_step(&queue, &self.step))
            .unwrap_or(false)
    }
    fn consume(&self, missing: bool) -> Option<bool> {
        self.queue
            .lock()
            .ok()
            .and_then(|mut queue| consume_step(&mut queue, &self.step, missing))
    }
    fn accept_keystroke<E>(&self, result: Result<(), E>) -> Result<Option<bool>, E> {
        result.map(|_| self.consume(false))
    }
    fn track(&self, content: Option<String>) {
        if let Ok(mut queue) = self.queue.lock() {
            if current_step(&queue, &self.step) {
                queue.last_action_was_paste = true;
                queue.last_pasted_content = content;
            }
        }
    }
}
fn skip_missing_step(queue: &mut PasteQueueState, step: &QueueStep) -> Option<usize> {
    consume_step(queue, step, true).map(|_| queue.items.len())
}
impl Drop for QueueStepGuard {
    fn drop(&mut self) {
        if let Ok(mut queue) = self.queue.lock() {
            release_step(&mut queue, &self.step);
        }
    }
}

impl Queue {
    pub(crate) fn invalidate(&self) -> Result<(), WireErrorCode> {
        self.state
            .lock()
            .map_err(|_| WireErrorCode::Unavailable)?
            .reset_tracking();
        Ok(())
    }
    pub(crate) fn echo(&self) -> Option<String> {
        let q = self.state.lock().ok()?;
        q.last_action_was_paste
            .then(|| q.last_pasted_content.clone())
            .flatten()
    }
    pub(crate) fn capture(
        &self,
        history: &dzc_core::api::HistoryService,
        id: i64,
        ticket: &dzc_core::api::CaptureTicket,
    ) -> Result<bool, WireErrorCode> {
        let mut q = self.state.lock().map_err(|_| WireErrorCode::Unavailable)?;
        if !history
            .queue_preferences()
            .map_err(|_| WireErrorCode::Unavailable)?
            .0
        {
            return Ok(false);
        }
        history
            .privacy()
            .check_capture(ticket, now_ms()?)
            .map_err(privacy_error)?;
        if q.last_action_was_paste {
            q.items.clear();
            q.reset_tracking();
        }
        if q.items.len() >= LIMIT {
            return Err(WireErrorCode::Busy);
        }
        q.items.push_back(id);
        q.revision = uuid::Uuid::new_v4();
        Ok(true)
    }
}

fn queue_text(payload: &ClipboardPayload) -> Option<&str> {
    match payload {
        ClipboardPayload::Text(text) | ClipboardPayload::Html { text, .. } => Some(text),
        ClipboardPayload::Private { payload, .. } => queue_text(payload),
        _ => None,
    }
}

fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, WireErrorCode> {
    serde_json::from_value(value).map_err(|_| WireErrorCode::InvalidRequest)
}
fn encode(value: impl serde::Serialize) -> Result<Value, WireErrorCode> {
    serde_json::to_value(value).map_err(|_| WireErrorCode::Internal)
}

impl Host {
    pub fn queue_length(&self) -> Result<usize, WireErrorCode> {
        Ok(self
            .queue
            .state
            .lock()
            .map_err(|_| WireErrorCode::Unavailable)?
            .items
            .len())
    }

    fn queue_snapshot(&self) -> Result<QueueState, WireErrorCode> {
        let (sequential, delete_after_paste, paste_method) = self
            .history
            .queue_preferences()
            .map_err(|_| WireErrorCode::Unavailable)?;
        let q = self
            .queue
            .state
            .lock()
            .map_err(|_| WireErrorCode::Unavailable)?;
        Ok(QueueState {
            ids: q.items.iter().copied().collect(),
            revision: q.revision.to_string(),
            preferences: QueuePreferences {
                sequential,
                delete_after_paste,
                paste_method,
            },
        })
    }

    pub(crate) fn queue_changed(&self) -> Result<(), WireErrorCode> {
        let count = self.queue_length()?;
        self.actions.queue_changed(count);
        self.publish(
            EventKind::ServiceStateChanged,
            EventData {
                entry_id: None,
                status: Some(format!("paste_queue_changed:{count}")),
            },
        )
    }

    pub(crate) fn queue_request(
        &self,
        session: &Session,
        method: Method,
        params: Value,
    ) -> Result<Value, WireErrorCode> {
        let instance = params["service_instance_id"]
            .as_str()
            .ok_or(WireErrorCode::InvalidRequest)?;
        let epoch = params["expected_epoch"]
            .as_u64()
            .ok_or(WireErrorCode::InvalidRequest)?;
        self.check_library_state(session, instance, epoch)?;
        match method {
            Method::DesktopPasteTarget => encode(
                dzc_platform::runtime::QueueInputTarget::foreground()
                    .map_err(|_| WireErrorCode::Unavailable)?,
            ),
            Method::DesktopQueueGet => encode(self.queue_snapshot()?),
            Method::DesktopQueueSet => {
                let p: QueueReplaceParams = decode(params)?;
                let mut q = self
                    .queue
                    .state
                    .lock()
                    .map_err(|_| WireErrorCode::Unavailable)?;
                q.items = p.ids.into();
                q.reset_tracking();
                drop(q);
                self.queue_changed()?;
                Ok(json!({"performed":true}))
            }
            Method::DesktopQueueConfigure => {
                let p: QueueSettingParams = decode(params)?;
                self.history
                    .update_queue_setting(&p.key, &p.value, epoch)
                    .map_err(|e| match e.code {
                        dzc_core::api::CoreErrorCode::Validation => WireErrorCode::InvalidRequest,
                        dzc_core::api::CoreErrorCode::PrivacyEpochChanged => {
                            WireErrorCode::PrivacyEpochChanged
                        }
                        _ => WireErrorCode::Unavailable,
                    })?;
                if p.key == "app.sequential_mode" && p.value == "false" {
                    let mut q = self
                        .queue
                        .state
                        .lock()
                        .map_err(|_| WireErrorCode::Unavailable)?;
                    q.items.clear();
                    q.reset_tracking();
                }
                self.queue_changed()?;
                Ok(json!({"performed":true}))
            }
            Method::DesktopQueueNext => {
                session.require(Permission::ClipboardPaste)?;
                if !self.actions.can_copy() || !self.actions.can_paste() {
                    return Err(WireErrorCode::UnsupportedCapability);
                }
                encode(self.queue_next(decode(params)?)?)
            }
            _ => Err(WireErrorCode::InvalidRequest),
        }
    }

    fn queue_next(&self, p: QueueNextParams) -> Result<QueueStepResult, WireErrorCode> {
        if self.is_stopping() {
            return Err(WireErrorCode::Busy);
        }
        let (step, guard) = {
            let mut q = self
                .queue
                .state
                .lock()
                .map_err(|_| WireErrorCode::Unavailable)?;
            if q.revision.to_string() != p.expected_revision {
                return Err(WireErrorCode::HistoryRevisionChanged);
            }
            let Some(step) = reserve_step(&mut q).map_err(|_| WireErrorCode::Busy)? else {
                return Ok(QueueStepResult {
                    status: "empty".into(),
                    id: None,
                    remaining: 0,
                });
            };
            (
                step.id,
                QueueStepGuard {
                    queue: self.queue.state.clone(),
                    step,
                },
            )
        };
        let outcome = self.deliver_queue_step(&guard, &p);
        if outcome.is_err() {
            let mut q = self
                .queue
                .state
                .lock()
                .map_err(|_| WireErrorCode::Unavailable)?;
            if current_step(&q, &guard.step) {
                q.reset_tracking();
            }
        }
        self.queue_changed()?;
        let remaining = self.queue_length()? as u32;
        outcome.map(|status| QueueStepResult {
            status: status.into(),
            id: Some(step),
            remaining,
        })
    }

    fn deliver_queue_step(
        &self,
        guard: &QueueStepGuard,
        p: &QueueNextParams,
    ) -> Result<&'static str, WireErrorCode> {
        let id = guard.step.id;
        let now = now_ms()?;
        let (kind, permit, payload) = if id > 0 {
            let Some(prepared) = self
                .history
                .prepare_queue_entry(id, Some(p.expected_epoch), now)
                .map_err(privacy_error)?
            else {
                guard.skip_missing();
                return Ok("missing");
            };
            if prepared.permit.epoch() != p.expected_epoch {
                return Err(WireErrorCode::PrivacyEpochChanged);
            }
            let payload = self
                .history
                .clipboard_payload_authorized(id, &prepared.permit, now)
                .map_err(privacy_error)?;
            (prepared.entry.content_type, prepared.permit, payload)
        } else {
            let permit = self
                .history
                .privacy()
                .authorize_selected_copy(
                    id,
                    Some(&p.service_instance_id),
                    SensitiveAccess::Denied,
                    Some(p.expected_epoch),
                    now,
                )
                .map_err(privacy_error)?;
            let row = self
                .session
                .lock()
                .map_err(|_| WireErrorCode::Unavailable)?
                .iter()
                .find(|r| r.id == id)
                .cloned();
            let Some(row) = row else {
                guard.skip_missing();
                return Ok("missing");
            };
            let payload = self
                .history
                .session_clipboard_payload_for_host_authorized(
                    &row,
                    &p.service_instance_id,
                    &permit,
                    now,
                )
                .map_err(privacy_error)?;
            (row.content_type, permit, payload)
        };
        let (_, delete, method) = self
            .history
            .queue_preferences()
            .map_err(|_| WireErrorCode::Unavailable)?;
        let target = dzc_platform::runtime::QueueInputTarget::prepare(&p.target)
            .map_err(|_| WireErrorCode::Unavailable)?;
        let text = queue_text(&payload).map(str::to_owned);
        guard.track(text.clone());
        let validate = || {
            if !guard.current() {
                return Err(WireErrorCode::HistoryRevisionChanged);
            }
            self.history
                .privacy()
                .check_read(&permit, now_ms()?)
                .map_err(privacy_error)
        };
        validate()?;
        self.copy_payload(if kind == "rich_text" {
            payload
        } else {
            crate::desktop_actions::plain(payload)
        })?;
        validate()?;
        guard.accept_keystroke(
            target
                .send(&method, text.as_deref(), &kind, || validate().is_ok())
                .map_err(|_| WireErrorCode::Unavailable),
        )?;
        let complete = if !delete && id < 0 {
            self.history
                .privacy()
                .check_read(&permit, now_ms()?)
                .map_err(privacy_error)
                .and_then(|()| {
                    let mut rows = self
                        .session
                        .lock()
                        .map_err(|_| WireErrorCode::Unavailable)?;
                    let row = rows
                        .iter_mut()
                        .find(|row| row.id == id)
                        .ok_or(WireErrorCode::NotFound)?;
                    row.use_count = row.use_count.saturating_add(1);
                    drop(rows);
                    self.publish(
                        EventKind::EntryUpdated,
                        EventData {
                            entry_id: Some(id),
                            status: None,
                        },
                    )
                })
        } else {
            self.mutate_inner(DesktopMutationParams {
                id,
                service_instance_id: p.service_instance_id.clone(),
                expected_epoch: p.expected_epoch,
                reveal: false,
                action: if delete { "delete" } else { "used" }.into(),
                content: None,
                pinned: None,
                tags: None,
                confirm_persistence: None,
            })
            .map(|_| ())
        };
        Ok(if complete.is_ok() {
            "pasted"
        } else {
            "metadata_failed"
        })
    }
}

#[cfg(test)]
mod reservation_tests {
    use super::*;
    use dzc_platform::paste_input::send_queue_input;
    use std::sync::{Arc, Mutex};
    #[test]
    fn rejected_input_never_restores_alt_or_consumes_and_success_restores_once() {
        use std::cell::{Cell, RefCell};
        let held = Cell::new(true);
        let order = RefCell::new(Vec::new());
        let original = "PasteKeystrokeFailed:accepted 1 of 2 events".to_owned();
        let result = send_queue_input(
            || {
                order.borrow_mut().push("validate");
                Ok(())
            },
            || {
                order.borrow_mut().push("send");
                held.set(false);
                Err(original.clone())
            },
            || {
                order.borrow_mut().push("restore");
                held.set(true);
                Ok(())
            },
        );
        assert_eq!(result, Err(original));
        assert!(!held.get());
        assert_eq!(*order.borrow(), vec!["validate", "send"]);
        let shared = Arc::new(Mutex::new(PasteQueueState::default()));
        shared.lock().unwrap().items.push_back(17);
        let step = reserve_step(&mut shared.lock().unwrap()).unwrap().unwrap();
        let guard = QueueStepGuard {
            queue: shared.clone(),
            step,
        };
        assert!(guard.accept_keystroke(result).is_err());
        assert_eq!(shared.lock().unwrap().items.front(), Some(&17));
        drop(guard);
        assert!(shared.lock().unwrap().in_flight.is_none());
        order.borrow_mut().clear();
        let denied = send_queue_input(
            || Err("PrivacyEpochChanged".into()),
            || {
                order.borrow_mut().push("send");
                Ok(())
            },
            || {
                order.borrow_mut().push("restore");
                Ok(())
            },
        );
        assert_eq!(denied, Err("PrivacyEpochChanged".into()));
        assert!(
            order.borrow().is_empty(),
            "validation failure must not inject or restore keys"
        );
        assert_eq!(
            send_queue_input(
                || Ok(()),
                || Ok(()),
                || {
                    order.borrow_mut().push("restore");
                    Ok(())
                }
            ),
            Ok(())
        );
        assert_eq!(*order.borrow(), vec!["restore"]);
    }
    #[test]
    fn rejected_windows_input_keeps_the_queue_and_drop_releases_the_original_reservation() {
        use dzc_platform::paste_input::{self, Input, InputDriver};
        struct Rejected {
            call: usize,
            fail: usize,
            accepted: usize,
        }
        impl InputDriver for Rejected {
            fn send(&mut self, inputs: &[Input]) -> usize {
                let call = self.call;
                self.call += 1;
                if call == self.fail {
                    self.accepted
                } else {
                    inputs.len()
                }
            }
            fn pause(&mut self, _: u64) {}
            fn key_is_down(&self, _: u16) -> bool {
                false
            }
            fn begin_text(&mut self) -> bool {
                true
            }
            fn same_target(&self) -> bool {
                true
            }
            fn end_text(&mut self) {}
        }
        for (method, text, fail, accepted) in [
            ("ctrl_v", None, 0, 4),
            ("ctrl_v", None, 1, 0),
            ("ctrl_v", None, 1, 1),
            ("ctrl_v", None, 2, 1),
            ("shift_insert", None, 2, 0),
            ("game_mode", None, 2, 0),
            ("game_mode", Some("中😀\r\n"), 7, 0),
        ] {
            let shared = Arc::new(Mutex::new(PasteQueueState::default()));
            shared.lock().unwrap().items.extend([17, 18]);
            let step = reserve_step(&mut shared.lock().unwrap()).unwrap().unwrap();
            {
                let guard = QueueStepGuard {
                    queue: shared.clone(),
                    step,
                };
                let result = paste_input::send_paste(
                    &mut Rejected {
                        call: 0,
                        fail,
                        accepted,
                    },
                    method,
                    text,
                    Some("text"),
                )
                .map_err(|error| error.to_string());
                let mut completions = 0;
                let accepted = guard.accept_keystroke(result).map(|_| {
                    completions += 1;
                });
                assert!(
                    accepted.is_err(),
                    "{method} failed input must not acknowledge a queue item"
                );
                assert_eq!(
                    completions, 0,
                    "neither service nor embedded completion may delete/count after failed input"
                );
                assert!(guard.current());
                assert_eq!(
                    shared
                        .lock()
                        .unwrap()
                        .items
                        .iter()
                        .copied()
                        .collect::<Vec<_>>(),
                    vec![17, 18]
                );
            }
            assert!(shared.lock().unwrap().in_flight.is_none());
            assert_eq!(
                reserve_step(&mut shared.lock().unwrap())
                    .unwrap()
                    .unwrap()
                    .id,
                17
            );
        }
    }
    #[test]
    fn accepted_input_with_lost_target_does_not_restore_alt_complete_or_consume() {
        use dzc_platform::paste_input::{self, Input, InputDriver};
        use std::cell::Cell;
        struct Changed {
            checks: Cell<usize>,
            lost_at: usize,
            sends: usize,
        }
        impl InputDriver for Changed {
            fn send(&mut self, inputs: &[Input]) -> usize {
                self.sends += 1;
                inputs.len()
            }
            fn pause(&mut self, _: u64) {}
            fn key_is_down(&self, _: u16) -> bool {
                false
            }
            fn begin_text(&mut self) -> bool {
                true
            }
            fn same_target(&self) -> bool {
                let check = self.checks.get();
                self.checks.set(check + 1);
                check != self.lost_at
            }
            fn end_text(&mut self) {}
        }
        for lost_at in [0, 3, 6] {
            let shared = Arc::new(Mutex::new(PasteQueueState::default()));
            shared.lock().unwrap().items.extend([17, 18]);
            let step = reserve_step(&mut shared.lock().unwrap()).unwrap().unwrap();
            let guard = QueueStepGuard {
                queue: shared.clone(),
                step,
            };
            let restores = Cell::new(0);
            let mut driver = Changed {
                checks: Cell::new(0),
                lost_at,
                sends: 0,
            };
            let result = send_queue_input(
                || Ok(()),
                || {
                    paste_input::send_paste(&mut driver, "ctrl_v", None, Some("text"))
                        .map_err(|error| error.to_string())
                },
                || {
                    restores.set(restores.get() + 1);
                    Ok(())
                },
            );
            assert!(result.as_ref().unwrap_err().contains("foreground"));
            assert_eq!(restores.get(), 0);
            assert!(guard.accept_keystroke(result).is_err());
            assert_eq!(
                shared
                    .lock()
                    .unwrap()
                    .items
                    .iter()
                    .copied()
                    .collect::<Vec<_>>(),
                vec![17, 18]
            );
            if lost_at == 0 {
                assert_eq!(driver.sends, 0);
            }
            if lost_at == 6 {
                assert_eq!(
                    driver.sends, 3,
                    "even fully accepted batches cannot acknowledge a failed final target check"
                );
            }
            drop(guard);
            assert!(shared.lock().unwrap().in_flight.is_none());
        }
        let result = send_queue_input(
            || Ok(()),
            || Ok(()),
            || Err("ForegroundChangedDuringAltRestore".into()),
        );
        assert_eq!(result, Err("ForegroundChangedDuringAltRestore".into()));
        let shared = Arc::new(Mutex::new(PasteQueueState::default()));
        shared.lock().unwrap().items.push_back(17);
        let step = reserve_step(&mut shared.lock().unwrap()).unwrap().unwrap();
        let guard = QueueStepGuard {
            queue: shared.clone(),
            step,
        };
        assert!(guard.accept_keystroke(result).is_err());
        assert_eq!(shared.lock().unwrap().items.front(), Some(&17));
    }
    #[test]
    fn dropping_a_cancelled_async_step_releases_its_reservation_without_consuming() {
        let shared = Arc::new(Mutex::new(PasteQueueState::default()));
        shared.lock().unwrap().items.push_back(1);
        let step = reserve_step(&mut shared.lock().unwrap()).unwrap().unwrap();
        {
            let pending = QueueStepGuard {
                queue: shared.clone(),
                step,
            };
            assert!(reserve_step(&mut shared.lock().unwrap()).is_err());
            reset_paste_tracking(&mut shared.lock().unwrap());
            assert!(!pending.current());
        }
        let mut queue = shared.lock().unwrap();
        assert_eq!(queue.items.front(), Some(&1));
        assert!(queue.in_flight.is_none());
        let next = reserve_step(&mut queue).unwrap().unwrap();
        drop(queue);
        let pending = QueueStepGuard {
            queue: shared.clone(),
            step: next.clone(),
        };
        release_step(&mut shared.lock().unwrap(), &next);
        let replacement = reserve_step(&mut shared.lock().unwrap()).unwrap().unwrap();
        drop(pending);
        assert!(
            current_step(&shared.lock().unwrap(), &replacement),
            "a stale Drop must not unlock a newer step"
        );
        release_step(&mut shared.lock().unwrap(), &replacement);
    }
    #[test]
    fn service_step_reservation_blocks_duplicate_work_and_failure_keeps_the_front() {
        let mut queue = PasteQueueState::default();
        queue.items.extend([1, 2]);
        let first = reserve_step(&mut queue).unwrap().unwrap();
        assert!(reserve_step(&mut queue).is_err());
        release_step(&mut queue, &first);
        assert_eq!(queue.items.iter().copied().collect::<Vec<_>>(), vec![1, 2]);
        let retry = reserve_step(&mut queue).unwrap().unwrap();
        assert!(!std::sync::Arc::ptr_eq(&first.token, &retry.token));
        release_step(&mut queue, &first);
        assert!(
            reserve_step(&mut queue).is_err(),
            "old cleanup must not release the current reservation"
        );
        assert_eq!(consume_step(&mut queue, &retry, false), Some(false));
        assert_eq!(
            consume_step(&mut queue, &retry, false),
            None,
            "a pasted item is consumed only once"
        );
        release_step(&mut queue, &retry);
        assert_eq!(reserve_step(&mut queue).unwrap().unwrap().id, 2);
    }
    #[test]
    fn replaced_or_cleared_queue_rejects_old_completion_even_for_the_same_id() {
        let mut queue = PasteQueueState::default();
        queue.items.push_back(1);
        let old = reserve_step(&mut queue).unwrap().unwrap();
        queue.items.clear();
        queue.items.push_back(1);
        reset_paste_tracking(&mut queue);
        assert!(!current_step(&queue, &old));
        assert_eq!(consume_step(&mut queue, &old, false), None);
        assert!(
            reserve_step(&mut queue).is_err(),
            "replacement must not overlap the pending cleanup"
        );
        release_step(&mut queue, &old);
        let current = reserve_step(&mut queue).unwrap().unwrap();
        queue.items.clear();
        reset_paste_tracking(&mut queue);
        assert_eq!(consume_step(&mut queue, &current, false), None);
        release_step(&mut queue, &current);
        assert!(reserve_step(&mut queue).unwrap().is_none());
    }
    #[test]
    fn missing_step_can_be_skipped_without_releasing_another_pending_operation() {
        let mut queue = PasteQueueState::default();
        queue.items.extend([1, 2]);
        let step = reserve_step(&mut queue).unwrap().unwrap();
        assert_eq!(skip_missing_step(&mut queue, &step), Some(1));
        assert_eq!(
            skip_missing_step(&mut queue, &step),
            None,
            "stale skips cannot emit a replacement length"
        );
        assert_eq!(queue.items.front(), Some(&2));
        assert!(!queue.last_action_was_paste);
        assert!(reserve_step(&mut queue).is_err());
        release_step(&mut queue, &step);
        let next = reserve_step(&mut queue).unwrap().unwrap();
        assert_eq!(next.id, 2);
        assert_eq!(skip_missing_step(&mut queue, &next), Some(0));
    }
}
