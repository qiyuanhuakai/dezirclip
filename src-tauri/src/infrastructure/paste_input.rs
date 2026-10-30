//! Windows paste sequences. Acceptance means input insertion, not application acknowledgement.
use std::fmt;

pub(crate) const CONTROL: u16 = 0x11;
pub(crate) const SHIFT: u16 = 0x10;
const ALT: u16 = 0x12;
const LEFT_WIN: u16 = 0x5b;
const RIGHT_WIN: u16 = 0x5c;
const V: u16 = 0x56;
const INSERT: u16 = 0x2d;
const ENTER: u16 = 0x0d;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Encoding { Virtual, Scan, ScanWithVirtual, ExtendedScan, Unicode }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Input { pub key: u16, pub encoding: Encoding, pub up: bool }
impl Input {
    fn new(key: u16, encoding: Encoding, up: bool) -> Self { Self { key, encoding, up } }
    fn released(self) -> Self { Self { up: true, ..self } }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PasteError { InputRejected { expected: usize, accepted: usize }, ForegroundChanged }
impl fmt::Display for PasteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputRejected { expected, accepted } => write!(f, "Paste input rejected: accepted {accepted} of {expected} events"),
            Self::ForegroundChanged => f.write_str("Paste input aborted: foreground window changed or unavailable"),
        }
    }
}
pub(crate) trait InputDriver {
    fn send(&mut self, inputs: &[Input]) -> usize;
    fn pause(&mut self, millis: u64);
    fn key_is_down(&self, key: u16) -> bool;
    fn begin_text(&mut self) -> bool;
    fn same_target(&self) -> bool;
    fn end_text(&mut self);
}

struct Attempt<'a, D> { driver: &'a mut D, pressed: Vec<Input>, initially_held: Vec<u16> }
impl<D: InputDriver> Attempt<'_, D> {
    fn send(&mut self, inputs: &[Input]) -> Result<(), PasteError> {
        if !self.driver.same_target() { return Err(PasteError::ForegroundChanged); }
        let accepted = self.driver.send(inputs);
        for input in inputs.iter().take(accepted.min(inputs.len())) {
            if input.up { self.pressed.retain(|down| down.key != input.key || down.encoding != input.encoding); }
            else if !self.pressed.contains(input) { self.pressed.push(*input); }
        }
        let same_target = self.driver.same_target();
        if accepted != inputs.len() { return Err(PasteError::InputRejected { expected: inputs.len(), accepted }); }
        if !same_target { return Err(PasteError::ForegroundChanged); }
        Ok(())
    }
    fn key(&mut self, key: u16, encoding: Encoding, up: bool) -> Result<(), PasteError> {
        self.send(&[Input::new(key, encoding, up)])
    }
    fn pair(&mut self, key: u16, encoding: Encoding, down_ms: u64, up_ms: u64) -> Result<(), PasteError> {
        self.key(key, encoding, false)?;
        self.driver.pause(down_ms);
        self.key(key, encoding, true)?;
        self.driver.pause(up_ms);
        Ok(())
    }
    fn text(&mut self, text: &str) -> Result<(), PasteError> {
        let length = text.chars().count();
        let (down, up) = if length > 800 { (2, 2) } else if length > 200 { (4, 4) } else { (10, 10) };
        for unit in text.encode_utf16() {
            match unit {
                13 => (),
                10 => self.pair(ENTER, Encoding::ScanWithVirtual, down, up)?,
                _ => self.pair(unit, Encoding::Unicode, down, up)?,
            }
        }
        Ok(())
    }
    fn run(&mut self, method: &str, content: Option<&str>, content_type: Option<&str>) -> Result<(), PasteError> {
        self.send(&[LEFT_WIN, RIGHT_WIN, ALT, SHIFT, CONTROL].map(|key| Input::new(key, Encoding::Virtual, true)))?;
        self.driver.pause(50);
        let method = if method == "game_mode" && !matches!(content_type, Some("text" | "code" | "url" | "rich_text")) { "ctrl_v" } else { method };
        if method == "ctrl_v" {
            self.send(&[Input::new(CONTROL, Encoding::Scan, false), Input::new(V, Encoding::Scan, false)])?;
            self.driver.pause(50);
            self.send(&[Input::new(V, Encoding::Scan, true), Input::new(CONTROL, Encoding::Scan, true)])
        } else if method == "game_mode" {
            self.driver.pause(250);
            if let Some(text) = content {
                if !self.driver.same_target() { return Err(PasteError::ForegroundChanged); }
                if !self.driver.begin_text() { return Err(PasteError::ForegroundChanged); }
                let result = self.text(text);
                self.driver.end_text();
                result
            } else {
                self.key(CONTROL, Encoding::Scan, false)?;
                self.driver.pause(80);
                self.key(V, Encoding::Scan, false)?;
                self.driver.pause(120);
                self.key(V, Encoding::Scan, true)?;
                self.driver.pause(80);
                self.key(CONTROL, Encoding::Scan, true)
            }
        } else {
            self.key(SHIFT, Encoding::ScanWithVirtual, false)?;
            self.driver.pause(10);
            self.key(INSERT, Encoding::ExtendedScan, false)?;
            self.driver.pause(10);
            self.key(INSERT, Encoding::ExtendedScan, true)?;
            self.driver.pause(10);
            self.key(SHIFT, Encoding::ScanWithVirtual, true)
        }
    }
    fn release_owned_keys(&mut self) {
        // Only release this attempt's inserted downs; never retry a paste or mask its error.
        let releases: Vec<_> = self.pressed.iter().rev()
            .filter(|input| input.encoding == Encoding::Unicode || !self.initially_held.contains(&input.key))
            .map(|input| input.released()).collect();
        if !releases.is_empty() { let _ = self.driver.send(&releases); }
    }
}
pub(crate) fn send_paste(driver: &mut impl InputDriver, method: &str, content: Option<&str>, content_type: Option<&str>) -> Result<(), PasteError> {
    let initially_held = [CONTROL, SHIFT, V, INSERT, ENTER].into_iter().filter(|key| driver.key_is_down(*key)).collect();
    let mut attempt = Attempt { driver, pressed: Vec::new(), initially_held };
    let result = attempt.run(method, content, content_type).and_then(|()| {
        if attempt.driver.same_target() { Ok(()) } else { Err(PasteError::ForegroundChanged) }
    });
    if result.is_err() { attempt.release_owned_keys(); }
    result
}
pub(crate) fn release_window_keys(driver: &mut impl InputDriver) -> Result<(), PasteError> {
    let mut attempt = Attempt { driver, pressed: Vec::new(), initially_held: Vec::new() };
    attempt.send(&[LEFT_WIN, RIGHT_WIN].map(|key| Input::new(key, Encoding::Virtual, true)))
}
pub(crate) fn restore_borrowed_ime<C: Copy + Eq>(original: C,
    mut same_window: impl FnMut() -> bool, borrow_current: impl FnOnce() -> Option<C>,
    restore_mode: impl FnOnce(C), mut release: impl FnMut(C)) {
    if same_window() {
        if let Some(current) = borrow_current() {
            if current == original && same_window() { restore_mode(original); }
            release(current);
        }
    }
    release(original);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    #[derive(Default)]
    struct Fake {
        calls: Vec<Vec<Input>>, pauses: Vec<u64>, fail: Option<(usize, usize)>, held: Vec<u16>,
        checks: Cell<usize>, lose_focus: Option<usize>, unavailable: bool, began: usize, ended: usize,
        lost_at_call: Cell<Option<usize>>,
    }
    impl InputDriver for Fake {
        fn send(&mut self, inputs: &[Input]) -> usize {
            self.calls.push(inputs.to_vec());
            self.fail.filter(|(call, _)| *call == self.calls.len() - 1).map_or(inputs.len(), |(_, count)| count)
        }
        fn pause(&mut self, ms: u64) { self.pauses.push(ms); }
        fn key_is_down(&self, key: u16) -> bool { self.held.contains(&key) }
        fn begin_text(&mut self) -> bool { self.began += 1; !self.unavailable }
        fn same_target(&self) -> bool {
            let check = self.checks.get(); self.checks.set(check + 1);
            let same = self.lose_focus != Some(check);
            if !same { self.lost_at_call.set(Some(self.calls.len())); }
            same
        }
        fn end_text(&mut self) { self.ended += 1; }
    }
    fn fault_matrix(method: &str, text: Option<&str>, kind: Option<&str>) {
        let mut full = Fake::default();
        assert_eq!(send_paste(&mut full, method, text, kind), Ok(()));
        for (call, events) in full.calls.iter().enumerate() {
            for accepted in 0..events.len() {
                let mut fake = Fake { fail: Some((call, accepted)), ..Default::default() };
                assert_eq!(send_paste(&mut fake, method, text, kind), Err(PasteError::InputRejected { expected: events.len(), accepted }), "{method} call {call}");
                assert_eq!(&fake.calls[..=call], &full.calls[..=call]);
                assert!(fake.calls[call + 1..].iter().flatten().all(|event| event.up), "a failure may only release keys, never type or retry");
                assert_eq!(fake.began, fake.ended, "restore the text context after every begun attempt");
            }
        }
    }
    #[test] fn ctrl_v_zero_partial_and_keyup_failures_propagate() { fault_matrix("ctrl_v", None, Some("text")); }
    #[test] fn shift_insert_and_unknown_method_failures_propagate() {
        fault_matrix("shift_insert", None, Some("files")); fault_matrix("unknown", None, None);
    }
    #[test] fn game_fallback_and_nontext_route_failures_propagate() {
        fault_matrix("game_mode", None, Some("text")); fault_matrix("game_mode", Some("not typed"), Some("image"));
    }
    #[test] fn every_unicode_surrogate_and_crlf_enter_failure_propagates() {
        fault_matrix("game_mode", Some("中😀\r\nZ"), Some("text"));
        let mut fake = Fake::default(); send_paste(&mut fake, "game_mode", Some("中😀\r\nZ"), Some("text")).unwrap();
        let typed: Vec<_> = fake.calls.iter().skip(1).flatten().filter(|event| !event.up).copied().collect();
        assert_eq!(typed.iter().filter(|event| event.encoding == Encoding::Unicode).map(|event| event.key).collect::<Vec<_>>(), "中😀Z".encode_utf16().collect::<Vec<_>>());
        assert_eq!(typed.iter().filter(|event| event.key == ENTER && event.encoding == Encoding::ScanWithVirtual).count(), 1);
    }
    #[test] fn every_batch_and_final_focus_check_rejects_loss_without_retrying_input() {
        for (method, content, kind) in [
            ("ctrl_v", None, Some("text")), ("shift_insert", None, Some("text")),
            ("unknown", None, None), ("game_mode", None, Some("text")),
            ("game_mode", Some("中😀\r\nZ"), Some("text")),
            ("game_mode", Some("not typed"), Some("image")),
        ] {
            let mut full = Fake::default();
            send_paste(&mut full, method, content, kind).unwrap();
            for check in 0..full.checks.get() {
                let mut fake = Fake { lose_focus: Some(check), ..Default::default() };
                assert_eq!(send_paste(&mut fake, method, content, kind), Err(PasteError::ForegroundChanged), "{method} check {check}");
                assert_eq!(fake.began, fake.ended);
                let stopped = fake.lost_at_call.get().unwrap();
                assert!(fake.calls[stopped..].iter().flatten().all(|event| event.up), "after target loss only release this attempt's keys");
                if check == 0 { assert!(fake.calls.is_empty(), "wrong initial target receives no modifier release or paste"); }
            }
        }
    }
    #[test] fn preparatory_window_key_releases_obey_the_same_target_and_count_guards() {
        let mut full = Fake::default();
        release_window_keys(&mut full).unwrap();
        assert_eq!(full.calls, vec![vec![Input::new(LEFT_WIN, Encoding::Virtual, true), Input::new(RIGHT_WIN, Encoding::Virtual, true)]]);
        for check in [0, 1] {
            let mut changed = Fake { lose_focus: Some(check), ..Default::default() };
            assert_eq!(release_window_keys(&mut changed), Err(PasteError::ForegroundChanged));
            assert_eq!(changed.calls.len(), check);
        }
        let mut partial = Fake { fail: Some((0, 1)), ..Default::default() };
        assert_eq!(release_window_keys(&mut partial), Err(PasteError::InputRejected { expected: 2, accepted: 1 }));
        assert_eq!(partial.calls.len(), 1);
    }
    #[test] fn unavailable_text_context_aborts_before_typing() {
        let mut fake = Fake { unavailable: true, ..Default::default() };
        assert_eq!(send_paste(&mut fake, "game_mode", Some("x"), Some("text")), Err(PasteError::ForegroundChanged));
        assert_eq!(fake.calls.len(), 1);
    }
    #[test] fn target_loss_after_ctrl_down_only_releases_that_attempt_and_keeps_partial_error() {
        let mut lost = Fake { lose_focus: Some(3), ..Default::default() };
        assert_eq!(send_paste(&mut lost, "ctrl_v", None, Some("text")), Err(PasteError::ForegroundChanged));
        assert_eq!(lost.calls[2], vec![Input::new(V, Encoding::Scan, true), Input::new(CONTROL, Encoding::Scan, true)]);
        let mut partial = Fake { lose_focus: Some(3), fail: Some((1, 1)), ..Default::default() };
        assert_eq!(send_paste(&mut partial, "ctrl_v", None, Some("text")), Err(PasteError::InputRejected { expected: 2, accepted: 1 }));
        assert_eq!(partial.calls[2], vec![Input::new(CONTROL, Encoding::Scan, true)]);
        let mut held = Fake { lose_focus: Some(3), fail: Some((1, 1)), held: vec![CONTROL], ..Default::default() };
        assert!(send_paste(&mut held, "ctrl_v", None, Some("text")).is_err());
        assert_eq!(held.calls.len(), 2);
    }
    #[test] fn partial_ctrl_down_only_releases_inserted_non_user_keys_and_keeps_original_error() {
        let mut fake = Fake { fail: Some((1, 1)), ..Default::default() };
        assert!(send_paste(&mut fake, "ctrl_v", None, Some("text")).is_err());
        assert_eq!(fake.calls[2], vec![Input::new(CONTROL, Encoding::Scan, true)]);
        let mut held = Fake { fail: Some((1, 1)), held: vec![CONTROL], ..Default::default() };
        assert!(send_paste(&mut held, "ctrl_v", None, Some("text")).is_err());
        assert_eq!(held.calls.len(), 2, "cleanup must not release a key originally held by the user");
    }
    #[test] fn complete_sequences_preserve_delays_and_routing() {
        for (method, content, kind, pauses) in [
            ("ctrl_v", None, Some("text"), vec![50, 50]),
            ("shift_insert", None, Some("text"), vec![50, 10, 10, 10]),
            ("game_mode", None, Some("text"), vec![50, 250, 80, 120, 80]),
            ("game_mode", Some("x"), Some("text"), vec![50, 250, 10, 10]),
            ("game_mode", Some("x"), Some("files"), vec![50, 50]),
        ] {
            let mut fake = Fake::default(); send_paste(&mut fake, method, content, kind).unwrap();
            assert_eq!(fake.pauses, pauses);
        }
    }
    #[test] fn ime_cleanup_rejects_replaced_windows_or_contexts_and_pairs_every_borrow() {
        use std::cell::RefCell;
        for (checks, current, expected) in [
            (vec![false], Some(7), vec![("release", 7)]),
            (vec![true], None, vec![("borrow", 0), ("release", 7)]),
            (vec![true], Some(9), vec![("borrow", 9), ("release", 9), ("release", 7)]),
            (vec![true, false], Some(7), vec![("borrow", 7), ("release", 7), ("release", 7)]),
            (vec![true, true], Some(7), vec![("borrow", 7), ("restore", 7), ("release", 7), ("release", 7)]),
        ] {
            let mut checks = checks.into_iter();
            let events = RefCell::new(Vec::new());
            restore_borrowed_ime(7, || checks.next().unwrap(),
                || { events.borrow_mut().push(("borrow", current.unwrap_or(0))); current },
                |context| events.borrow_mut().push(("restore", context)),
                |context| events.borrow_mut().push(("release", context)));
            assert_eq!(*events.borrow(), expected);
        }
    }
}
