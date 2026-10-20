//! Bounded KDE privacy metadata. The live connection observes selection replacement during reads.
use dzc_core::capture::{CaptureFailure, CaptureStage, PrivacyHintAvailability, PrivacyPreflight};
use dzc_core::ports::PrivacyHint;
use std::cell::Cell;
use std::time::{Duration, Instant};
use x11rb::connection::Connection;
use x11rb::protocol::xfixes::{ConnectionExt as _, SelectionEventMask};
use x11rb::protocol::xproto::{Atom, AtomEnum, ConnectionExt as _, CreateWindowAux, WindowClass};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;

const MAX_HINT_BYTES: usize = 64;
const HINT_TIMEOUT: Duration = Duration::from_millis(200);

fn failure() -> CaptureFailure {
    CaptureFailure {
        stage: CaptureStage::Read,
        code: "PrivacyHintReadUnavailable".into(),
    }
}

fn is_secret(bytes: &[u8]) -> Result<bool, CaptureFailure> {
    if bytes.len() > MAX_HINT_BYTES {
        return Err(failure());
    }
    Ok(bytes.strip_suffix(&[0]).unwrap_or(bytes) == b"secret")
}

pub struct SelectionSnapshot {
    connection: RustConnection,
    clipboard: Atom,
    owner: u32,
    identity: u64,
    watching: bool,
    changed: Cell<bool>,
    report: PrivacyPreflight,
}

impl SelectionSnapshot {
    pub fn preflight() -> Result<Self, CaptureFailure> {
        crate::runtime_display::validate_x11_display().map_err(|_| failure())?;
        let (connection, screen) = x11rb::connect(None).map_err(|_| failure())?;
        let clipboard = connection
            .intern_atom(false, b"CLIPBOARD")
            .map_err(|_| failure())?
            .reply()
            .map_err(|_| failure())?
            .atom;
        let hint = connection
            .intern_atom(false, b"x-kde-passwordManagerHint")
            .map_err(|_| failure())?
            .reply()
            .map_err(|_| failure())?
            .atom;
        let targets = connection
            .intern_atom(false, b"TARGETS")
            .map_err(|_| failure())?
            .reply()
            .map_err(|_| failure())?
            .atom;
        let property = connection
            .intern_atom(false, b"DZC_PRIVACY_HINT")
            .map_err(|_| failure())?
            .reply()
            .map_err(|_| failure())?
            .atom;
        let window = connection.generate_id().map_err(|_| failure())?;
        connection
            .create_window(
                x11rb::COPY_DEPTH_FROM_PARENT,
                window,
                connection.setup().roots[screen].root,
                0,
                0,
                1,
                1,
                0,
                WindowClass::INPUT_OUTPUT,
                x11rb::COPY_FROM_PARENT,
                &CreateWindowAux::new(),
            )
            .map_err(|_| failure())?
            .check()
            .map_err(|_| failure())?;
        let watching = connection
            .xfixes_query_version(5, 0)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .is_some()
            && connection
                .xfixes_select_selection_input(
                    window,
                    clipboard,
                    SelectionEventMask::SET_SELECTION_OWNER
                        | SelectionEventMask::SELECTION_WINDOW_DESTROY
                        | SelectionEventMask::SELECTION_CLIENT_CLOSE,
                )
                .ok()
                .is_some_and(|cookie| cookie.check().is_ok());
        let owner = connection
            .get_selection_owner(clipboard)
            .map_err(|_| failure())?
            .reply()
            .map_err(|_| failure())?
            .owner;
        let identity = u64::from_le_bytes(
            uuid::Uuid::new_v4().as_bytes()[..8]
                .try_into()
                .map_err(|_| failure())?,
        );
        let mut snapshot = Self {
            connection,
            clipboard,
            owner,
            identity,
            watching,
            changed: Cell::new(false),
            report: PrivacyPreflight::default(),
        };
        let mut targets_checked = owner == x11rb::NONE;
        let value = if owner == x11rb::NONE {
            Vec::new()
        } else {
            let advertised = snapshot.read_property(window, targets, property, 4096)?;
            let has_hint = if let Some(reply) = advertised {
                if reply.format != 32 || reply.type_ != u32::from(AtomEnum::ATOM) {
                    return Err(failure());
                }
                targets_checked = true;
                reply
                    .value32()
                    .ok_or_else(failure)?
                    .any(|atom| atom == hint)
            } else {
                true
            };
            if !has_hint {
                Vec::new()
            } else {
                let reply = snapshot
                    .read_property(window, hint, property, MAX_HINT_BYTES)?
                    .ok_or_else(failure)?;
                if reply.format != 8 {
                    return Err(failure());
                }
                reply.value
            }
        };
        if !snapshot.is_current() {
            return Err(failure());
        }
        snapshot.report = PrivacyPreflight {
            hints: if is_secret(&value)? {
                vec![PrivacyHint::Sensitive]
            } else {
                Vec::new()
            },
            availability: if watching && targets_checked {
                PrivacyHintAvailability::Checked
            } else {
                PrivacyHintAvailability::Unavailable
            },
            selection_id: watching.then_some(identity),
        };
        Ok(snapshot)
    }

    fn read_property(
        &self,
        window: u32,
        hint: Atom,
        property: Atom,
        maximum: usize,
    ) -> Result<Option<x11rb::protocol::xproto::GetPropertyReply>, CaptureFailure> {
        self.connection
            .convert_selection(window, self.clipboard, hint, property, x11rb::CURRENT_TIME)
            .map_err(|_| failure())?
            .check()
            .map_err(|_| failure())?;
        self.connection.flush().map_err(|_| failure())?;
        let deadline = Instant::now() + HINT_TIMEOUT;
        while Instant::now() < deadline {
            match self.connection.poll_for_event().map_err(|_| failure())? {
                Some(Event::XfixesSelectionNotify(event)) if event.selection == self.clipboard => {
                    self.changed.set(true);
                    return Err(failure());
                }
                Some(Event::SelectionNotify(event))
                    if event.requestor == window
                        && event.selection == self.clipboard
                        && event.target == hint =>
                {
                    if event.property == x11rb::NONE {
                        return Ok(None);
                    }
                    if event.property != property {
                        return Err(failure());
                    }
                    let reply = self
                        .connection
                        .get_property(
                            true,
                            window,
                            property,
                            AtomEnum::ANY,
                            0,
                            maximum.div_ceil(4) as u32,
                        )
                        .map_err(|_| failure())?
                        .reply()
                        .map_err(|_| failure())?;
                    if reply.bytes_after != 0 || reply.value.len() > maximum {
                        return Err(failure());
                    }
                    return Ok(Some(reply));
                }
                Some(_) => {}
                None => std::thread::sleep(Duration::from_millis(1)),
            }
        }
        Err(failure())
    }

    pub fn report(&self) -> PrivacyPreflight {
        self.report.clone()
    }

    /// XFixes also observes reassertion by the same owner, which owner IDs alone cannot distinguish.
    pub fn is_current(&self) -> bool {
        let owner = self
            .connection
            .get_selection_owner(self.clipboard)
            .ok()
            .and_then(|cookie| cookie.reply().ok());
        if owner.is_none_or(|reply| reply.owner != self.owner) {
            self.changed.set(true);
        }
        loop {
            match self.connection.poll_for_event() {
                Ok(Some(Event::XfixesSelectionNotify(event)))
                    if event.selection == self.clipboard =>
                {
                    self.changed.set(true)
                }
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(_) => {
                    self.changed.set(true);
                    break;
                }
            }
        }
        !self.changed.get()
    }

    pub fn selection_id(&self) -> Option<u64> {
        (self.watching && self.is_current()).then_some(self.identity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_explicit_kde_secret_value_skips_history() {
        assert!(is_secret(b"secret").unwrap());
        assert!(is_secret(b"secret\0").unwrap());
        for value in [
            b"".as_slice(),
            b"public",
            b"SECRET",
            b" secret",
            b"secret\n",
        ] {
            assert!(!is_secret(value).unwrap());
        }
        assert_eq!(
            is_secret(&[0; 65]).unwrap_err().code,
            "PrivacyHintReadUnavailable"
        );
    }
    #[test]
    fn isolated_x11_metadata_is_bounded_and_same_owner_replacement_revokes_the_snapshot() {
        if std::env::var("DEZIRCLIP_RUN_LINUX_CLIPBOARD_SMOKE_TESTS").as_deref() != Ok("1") {
            return;
        }
        use std::sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc, Mutex,
        };
        use x11rb::protocol::xproto::{
            EventMask, PropMode, SelectionNotifyEvent, SELECTION_NOTIFY_EVENT,
        };
        use x11rb::wrapper::ConnectionExt as _;
        let (owner, screen) = x11rb::connect(None).unwrap();
        let clipboard = owner
            .intern_atom(false, b"CLIPBOARD")
            .unwrap()
            .reply()
            .unwrap()
            .atom;
        let targets = owner
            .intern_atom(false, b"TARGETS")
            .unwrap()
            .reply()
            .unwrap()
            .atom;
        let hint = owner
            .intern_atom(false, b"x-kde-passwordManagerHint")
            .unwrap()
            .reply()
            .unwrap()
            .atom;
        let window = owner.generate_id().unwrap();
        owner
            .create_window(
                x11rb::COPY_DEPTH_FROM_PARENT,
                window,
                owner.setup().roots[screen].root,
                0,
                0,
                1,
                1,
                0,
                WindowClass::INPUT_OUTPUT,
                x11rb::COPY_FROM_PARENT,
                &CreateWindowAux::new(),
            )
            .unwrap()
            .check()
            .unwrap();
        owner
            .set_selection_owner(window, clipboard, x11rb::CURRENT_TIME)
            .unwrap()
            .check()
            .unwrap();
        let owner = Arc::new(owner);
        let payload = Arc::new(Mutex::new(Some(b"secret".to_vec())));
        let stop = Arc::new(AtomicBool::new(false));
        let silent = Arc::new(AtomicBool::new(false));
        let body_requests = Arc::new(AtomicUsize::new(0));
        let server = owner.clone();
        let server_payload = payload.clone();
        let server_stop = stop.clone();
        let server_silent = silent.clone();
        let server_body = body_requests.clone();
        let worker = std::thread::spawn(move || {
            while !server_stop.load(Ordering::Relaxed) {
                match server.poll_for_event().unwrap() {
                    Some(Event::SelectionRequest(request)) => {
                        if server_silent.load(Ordering::Relaxed) {
                            continue;
                        }
                        if request.target == targets {
                            let formats = if server_payload.lock().unwrap().is_some() {
                                vec![hint, targets]
                            } else {
                                vec![targets]
                            };
                            server
                                .change_property32(
                                    PropMode::REPLACE,
                                    request.requestor,
                                    request.property,
                                    AtomEnum::ATOM,
                                    &formats,
                                )
                                .unwrap();
                            server
                                .send_event(
                                    false,
                                    request.requestor,
                                    EventMask::NO_EVENT,
                                    SelectionNotifyEvent {
                                        response_type: SELECTION_NOTIFY_EVENT,
                                        sequence: 0,
                                        time: request.time,
                                        requestor: request.requestor,
                                        selection: request.selection,
                                        target: request.target,
                                        property: request.property,
                                    },
                                )
                                .unwrap();
                            server.flush().unwrap();
                            continue;
                        }
                        let value = if request.target == hint {
                            server_payload.lock().unwrap().clone()
                        } else {
                            server_body.fetch_add(1, Ordering::Relaxed);
                            None
                        };
                        let property = if let Some(value) = value {
                            server
                                .change_property8(
                                    PropMode::REPLACE,
                                    request.requestor,
                                    request.property,
                                    hint,
                                    &value,
                                )
                                .unwrap();
                            request.property
                        } else {
                            x11rb::NONE
                        };
                        server
                            .send_event(
                                false,
                                request.requestor,
                                EventMask::NO_EVENT,
                                SelectionNotifyEvent {
                                    response_type: SELECTION_NOTIFY_EVENT,
                                    sequence: 0,
                                    time: request.time,
                                    requestor: request.requestor,
                                    selection: request.selection,
                                    target: request.target,
                                    property,
                                },
                            )
                            .unwrap();
                        server.flush().unwrap();
                    }
                    Some(_) => {}
                    None => std::thread::sleep(Duration::from_millis(1)),
                }
            }
        });
        let snapshot = SelectionSnapshot::preflight().unwrap();
        assert_eq!(
            snapshot.report().availability,
            PrivacyHintAvailability::Checked
        );
        assert_eq!(snapshot.report().hints, vec![PrivacyHint::Sensitive]);
        assert!(snapshot.selection_id().is_some());
        *payload.lock().unwrap() = Some(b"public".to_vec());
        owner
            .set_selection_owner(window, clipboard, x11rb::CURRENT_TIME)
            .unwrap()
            .check()
            .unwrap();
        assert_eq!(snapshot.selection_id(), None);
        assert!(SelectionSnapshot::preflight()
            .unwrap()
            .report()
            .hints
            .is_empty());
        *payload.lock().unwrap() = Some(vec![7; 65]);
        assert!(SelectionSnapshot::preflight().is_err());
        *payload.lock().unwrap() = None;
        assert!(SelectionSnapshot::preflight()
            .unwrap()
            .report()
            .hints
            .is_empty());
        silent.store(true, Ordering::Relaxed);
        let start = Instant::now();
        assert!(SelectionSnapshot::preflight().is_err());
        assert!(start.elapsed() < Duration::from_secs(5));
        assert_eq!(body_requests.load(Ordering::Relaxed), 0);
        stop.store(true, Ordering::Relaxed);
        worker.join().unwrap();
    }
}
