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
        let accepted = self.driver.send(inputs);
        for input in inputs.iter().take(accepted.min(inputs.len())) {
            if input.up { self.pressed.retain(|down| down.key != input.key || down.encoding != input.encoding); }
            else if !self.pressed.contains(input) { self.pressed.push(*input); }
        }
        if accepted == inputs.len() { Ok(()) }
        else { Err(PasteError::InputRejected { expected: inputs.len(), accepted }) }
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
        let (down, up, interval) = if length > 800 { (2, 2, 40) } else if length > 200 { (4, 4, 30) } else { (10, 10, 20) };
        for (index, unit) in text.encode_utf16().enumerate() {
            if index % interval == 0 && !self.driver.same_target() { return Err(PasteError::ForegroundChanged); }
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
    let result = attempt.run(method, content, content_type);
    if result.is_err() { attempt.release_owned_keys(); }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    #[derive(Default)]
    struct Fake {
        calls: Vec<Vec<Input>>, pauses: Vec<u64>, fail: Option<(usize, usize)>, held: Vec<u16>,
        checks: Cell<usize>, lose_focus: Option<usize>, unavailable: bool, began: usize, ended: usize,
    }
    impl InputDriver for Fake {
        fn send(&mut self, inputs: &[Input]) -> usize {
            self.calls.push(inputs.to_vec());
            self.fail.filter(|(call, _)| *call == self.calls.len() - 1).map_or(inputs.len(), |(_, count)| count)
        }
        fn pause(&mut self, ms: u64) { self.pauses.push(ms); }
        fn key_is_down(&self, key: u16) -> bool { self.held.contains(&key) }
        fn begin_text(&mut self) -> bool { self.began += 1; !self.unavailable }
        fn same_target(&self) -> bool { let check = self.checks.get(); self.checks.set(check + 1); self.lose_focus != Some(check) }
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
    #[test] fn lost_initial_or_mid_text_focus_is_failure_and_restores_context() {
        for check in [0, 1] {
            let mut fake = Fake { lose_focus: Some(check), ..Default::default() };
            assert_eq!(send_paste(&mut fake, "game_mode", Some(&"a".repeat(45)), Some("text")), Err(PasteError::ForegroundChanged));
            assert_eq!((fake.began, fake.ended), (1, 1));
            assert_eq!(fake.calls.iter().flatten().filter(|event| !event.up).count(), check * 20);
        }
        let mut fake = Fake { unavailable: true, ..Default::default() };
        assert_eq!(send_paste(&mut fake, "game_mode", Some("x"), Some("text")), Err(PasteError::ForegroundChanged));
        assert_eq!(fake.calls.len(), 1);
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
}
