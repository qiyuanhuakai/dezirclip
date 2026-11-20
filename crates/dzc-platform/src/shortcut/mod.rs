//! Native registrations belong to one worker, independent of Tauri's global handler.
use dzc_core::ports::{PlatformResult, Shortcut, ShortcutProvider, ShortcutRegistration};
use dzc_protocol::{Capability, CapabilityState, PlatformError, UnsupportedReason};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex, Weak,
};
use std::time::Duration;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "linux")]
mod x11;
#[cfg(target_os = "linux")]
pub use x11::enable_x11_shortcut_events;

fn failed() -> PlatformError {
    PlatformError::failed(Capability::GlobalShortcut)
}
fn unsupported(reason: UnsupportedReason) -> PlatformError {
    PlatformError::unsupported(Capability::GlobalShortcut, reason)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Key {
    name: &'static str,
    windows: u32,
    x11: u32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Chord {
    modifiers: u8,
    key: Key,
}
const CTRL: u8 = 1;
const ALT: u8 = 2;
const SHIFT: u8 = 4;
const SUPER: u8 = 8;

const LETTERS: [&str; 26] = [
    "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S",
    "T", "U", "V", "W", "X", "Y", "Z",
];
const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
const FUNCTIONS: [&str; 24] = [
    "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "F13", "F14", "F15",
    "F16", "F17", "F18", "F19", "F20", "F21", "F22", "F23", "F24",
];
const KEY_RANGES: &[(&[&str], u32, u32)] = &[
    (&LETTERS, 0x41, 0x61),
    (&DIGITS, 0x30, 0x30),
    (&FUNCTIONS, 0x70, 0xffbe),
];

// Win32 virtual keys and X11 keysyms for keys that do not form a contiguous range.
const NAMED_KEYS: &[(&str, u32, u32, &[&str])] = &[
    ("Space", 0x20, 0x20, &[]),
    ("Tab", 0x09, 0xff09, &[]),
    ("Enter", 0x0d, 0xff0d, &["Return"]),
    ("Escape", 0x1b, 0xff1b, &["Esc"]),
    ("Backspace", 0x08, 0xff08, &[]),
    ("Delete", 0x2e, 0xffff, &["Del"]),
    ("Insert", 0x2d, 0xff63, &["Ins"]),
    ("Home", 0x24, 0xff50, &[]),
    ("End", 0x23, 0xff57, &[]),
    ("PageUp", 0x21, 0xff55, &[]),
    ("PageDown", 0x22, 0xff56, &[]),
    ("Left", 0x25, 0xff51, &["ArrowLeft"]),
    ("Right", 0x27, 0xff53, &["ArrowRight"]),
    ("Up", 0x26, 0xff52, &["ArrowUp"]),
    ("Down", 0x28, 0xff54, &["ArrowDown"]),
    ("Backquote", 0xc0, 0x60, &["`"]),
    ("Minus", 0xbd, 0x2d, &["-"]),
    ("Equal", 0xbb, 0x3d, &["=", "Plus"]),
    ("BracketLeft", 0xdb, 0x5b, &["["]),
    ("BracketRight", 0xdd, 0x5d, &["]"]),
    ("Backslash", 0xdc, 0x5c, &["\\"]),
    ("Semicolon", 0xba, 0x3b, &[";"]),
    ("Quote", 0xde, 0x27, &["'"]),
    ("Comma", 0xbc, 0x2c, &[","]),
    ("Period", 0xbe, 0x2e, &["."]),
    ("Slash", 0xbf, 0x2f, &["/"]),
    ("CapsLock", 0x14, 0xffe5, &[]),
    ("NumLock", 0x90, 0xff7f, &[]),
    ("ScrollLock", 0x91, 0xff14, &[]),
    ("PrintScreen", 0x2c, 0xff61, &[]),
    ("Pause", 0x13, 0xff13, &["PauseBreak"]),
    ("ContextMenu", 0x5d, 0xff67, &["Apps"]),
    ("Numpad0", 0x60, 0xffb0, &["Num0"]),
    ("Numpad1", 0x61, 0xffb1, &["Num1"]),
    ("Numpad2", 0x62, 0xffb2, &["Num2"]),
    ("Numpad3", 0x63, 0xffb3, &["Num3"]),
    ("Numpad4", 0x64, 0xffb4, &["Num4"]),
    ("Numpad5", 0x65, 0xffb5, &["Num5"]),
    ("Numpad6", 0x66, 0xffb6, &["Num6"]),
    ("Numpad7", 0x67, 0xffb7, &["Num7"]),
    ("Numpad8", 0x68, 0xffb8, &["Num8"]),
    ("Numpad9", 0x69, 0xffb9, &["Num9"]),
    (
        "NumpadAdd",
        0x6b,
        0xffab,
        &["NumAdd", "NumpadPlus", "NumPlus"],
    ),
    ("NumpadSubtract", 0x6d, 0xffad, &["NumSub", "NumSubtract"]),
    ("NumpadMultiply", 0x6a, 0xffaa, &["NumMult", "NumMultiply"]),
    ("NumpadDivide", 0x6f, 0xffaf, &["NumDiv", "NumDivide"]),
    ("NumpadDecimal", 0x6e, 0xffae, &["NumDec", "NumDecimal"]),
    ("NumpadEnter", 0x00, 0xff8d, &["NumEnter"]),
    ("NumpadEqual", 0x00, 0xffbd, &["NumEqual"]),
    ("VolumeMute", 0xad, 0x1008ff12, &["AudioVolumeMute"]),
    ("VolumeDown", 0xae, 0x1008ff11, &["AudioVolumeDown"]),
    ("VolumeUp", 0xaf, 0x1008ff13, &["AudioVolumeUp"]),
    ("MediaTrackNext", 0xb0, 0x1008ff17, &["MediaNextTrack"]),
    (
        "MediaTrackPrevious",
        0xb1,
        0x1008ff16,
        &["MediaPreviousTrack", "MediaTrackPrev"],
    ),
    ("MediaStop", 0xb2, 0x1008ff15, &[]),
    ("MediaPlayPause", 0xb3, 0x1008ff14, &[]),
];

fn key(value: &str) -> Option<Key> {
    for &(names, windows, x11) in KEY_RANGES {
        if let Some(index) = names
            .iter()
            .position(|name| name.eq_ignore_ascii_case(value))
        {
            return Some(Key {
                name: names[index],
                windows: windows + index as u32,
                x11: x11 + index as u32,
            });
        }
    }
    NAMED_KEYS
        .iter()
        .find_map(|&(name, windows, x11, aliases)| {
            let matches = name.eq_ignore_ascii_case(value)
                || aliases
                    .iter()
                    .any(|alias| alias.eq_ignore_ascii_case(value));
            matches.then_some(Key { name, windows, x11 })
        })
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn native_key_name(code: u32, platform_code: impl Fn(u32, u32) -> u32) -> Option<&'static str> {
    for &(names, windows, x11) in KEY_RANGES {
        if let Some(index) = code.checked_sub(platform_code(windows, x11)) {
            if let Some(name) = names.get(index as usize) {
                return Some(name);
            }
        }
    }
    NAMED_KEYS
        .iter()
        .find_map(|&(name, windows, x11, _)| (platform_code(windows, x11) == code).then_some(name))
}

#[cfg(target_os = "windows")]
pub fn windows_key_name(code: u32) -> Option<&'static str> {
    (code != 0).then_some(())?;
    native_key_name(code, |windows, _| windows)
}

#[cfg(target_os = "linux")]
pub fn x11_key_name(symbols: &[u32]) -> Option<&'static str> {
    let base = *symbols.first()?;
    if (0x41..=0x5a).contains(&base) {
        return native_key_name(base + 0x20, |_, x11| x11);
    }
    // An X11 keypad uses the navigation symbol at level one and the number at level two.
    if let Some(number) = symbols.get(1).filter(|&&key| x11_keypad_number(key)) {
        return native_key_name(*number, |_, x11| x11);
    }
    native_key_name(base, |_, x11| x11)
}

#[cfg(target_os = "linux")]
fn x11_keypad_number(keysym: u32) -> bool {
    keysym == 0xffae || (0xffb0..=0xffb9).contains(&keysym)
}

fn parse(value: &str) -> PlatformResult<Chord> {
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        return Err(unsupported(UnsupportedReason::UnsupportedRepresentation));
    }
    let mut modifiers = 0;
    let mut selected = None;
    for part in value.split('+').map(str::trim) {
        let modifier = match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => CTRL,
            "alt" => ALT,
            "shift" => SHIFT,
            "super" | "meta" | "win" | "windows" | "cmd" | "command" => SUPER,
            _ => 0,
        };
        if modifier != 0 {
            if modifiers & modifier != 0 {
                return Err(unsupported(UnsupportedReason::UnsupportedRepresentation));
            }
            modifiers |= modifier;
        } else if selected.is_none() {
            selected = key(part);
            if selected.is_none() {
                return Err(unsupported(UnsupportedReason::UnsupportedRepresentation));
            }
        } else {
            return Err(unsupported(UnsupportedReason::UnsupportedRepresentation));
        }
    }
    Ok(Chord {
        modifiers,
        key: selected.ok_or_else(|| unsupported(UnsupportedReason::UnsupportedRepresentation))?,
    })
}

/// Canonicalization validates a supported chord; it does not register or reserve it.
pub fn normalize_main_shortcut(value: &str) -> PlatformResult<String> {
    if value.eq_ignore_ascii_case("MouseMiddle") || value.eq_ignore_ascii_case("MButton") {
        return Ok("MouseMiddle".into());
    }
    normalize_shortcut(value)
}

pub fn normalize_shortcut(value: &str) -> PlatformResult<String> {
    let chord = parse(value)?;
    let mut parts = Vec::new();
    for (mask, name) in [
        (CTRL, "Ctrl"),
        (ALT, "Alt"),
        (SHIFT, "Shift"),
        (SUPER, "Super"),
    ] {
        if chord.modifiers & mask != 0 {
            parts.push(name);
        }
    }
    parts.push(chord.key.name);
    Ok(parts.join("+"))
}

type Wake = Box<dyn Fn() -> PlatformResult<()> + Send + Sync>;
#[derive(Default)]
struct Control {
    cancelled: AtomicBool,
    armed: AtomicBool,
    running: AtomicBool,
    wake: Mutex<Option<Wake>>,
}
struct ClearOnExit(Arc<Control>);
impl Drop for ClearOnExit {
    fn drop(&mut self) {
        self.0.clear();
    }
}
impl Control {
    fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
    fn install(&self, wake: Wake) -> PlatformResult<()> {
        let mut slot = self.wake.lock().map_err(|_| failed())?;
        *slot = Some(wake);
        if self.cancelled() {
            if let Some(wake) = &*slot {
                wake()?;
            }
        }
        Ok(())
    }
    fn clear(&self) {
        self.armed.store(false, Ordering::Release);
        self.running.store(false, Ordering::Release);
        if let Ok(mut slot) = self.wake.lock() {
            *slot = None;
        }
    }
    fn cancel(&self) -> PlatformResult<()> {
        self.armed.store(false, Ordering::Release);
        self.cancelled.store(true, Ordering::Release);
        if let Some(wake) = &*self.wake.lock().map_err(|_| failed())? {
            wake()?;
        }
        Ok(())
    }
    fn pressed(&self, callback: &mut dyn FnMut()) {
        if self.armed.load(Ordering::Acquire) && !self.cancelled() {
            callback();
        }
    }
}

struct Registration {
    control: Arc<Control>,
    done: mpsc::Receiver<()>,
    worker: Option<std::thread::JoinHandle<PlatformResult<()>>>,
}
impl Registration {
    fn stop_with_timeout(&mut self, timeout: Duration) -> PlatformResult<()> {
        if self.worker.is_none() {
            return Ok(());
        }
        // The armed gate closes even if the native wake fails or a callback is slow.
        let _wake = self.control.cancel();
        if self.done.recv_timeout(timeout).is_err()
            && !self
                .worker
                .as_ref()
                .is_some_and(|worker| worker.is_finished())
        {
            return Err(failed());
        }
        self.worker
            .take()
            .expect("worker remains owned until join")
            .join()
            .map_err(|_| failed())?
    }
}
impl ShortcutRegistration for Registration {
    fn unregister(&mut self) -> PlatformResult<()> {
        self.stop_with_timeout(Duration::from_secs(5))
    }
}
impl Drop for Registration {
    fn drop(&mut self) {
        let _ = self.unregister();
    }
}

#[derive(Default)]
struct State {
    registrations: Mutex<Vec<Weak<Control>>>,
    // Failed startup does not discard a worker whose stop has not been confirmed.
    pending: Mutex<Vec<Registration>>,
}
#[derive(Default, Clone)]
pub struct NativeShortcutProvider {
    state: Arc<State>,
}

impl NativeShortcutProvider {
    fn active(&self) -> bool {
        self.state
            .registrations
            .lock()
            .map(|mut entries| {
                entries.retain(|entry| entry.strong_count() > 0);
                entries
                    .iter()
                    .filter_map(Weak::upgrade)
                    .any(|control| control.running.load(Ordering::Acquire))
            })
            .unwrap_or(false)
    }
    /// Drains only failed-startup workers; successful registrations are caller owned.
    pub fn drain_failed_registrations(&self) -> PlatformResult<()> {
        let mut pending = self.state.pending.lock().map_err(|_| failed())?;
        let mut result = Ok(());
        let mut index = 0;
        while index < pending.len() {
            if let Err(error) = pending[index].unregister() {
                result = Err(error);
                index += 1;
            } else {
                pending.remove(index);
            }
        }
        result
    }
    fn start(
        &self,
        run: impl FnOnce(Arc<Control>, mpsc::SyncSender<PlatformResult<()>>) -> PlatformResult<()>
            + Send
            + 'static,
    ) -> PlatformResult<Box<dyn ShortcutRegistration>> {
        let control = Arc::new(Control::default());
        self.state
            .registrations
            .lock()
            .map_err(|_| failed())?
            .push(Arc::downgrade(&control));
        let worker_control = control.clone();
        let (ready_tx, ready) = mpsc::sync_channel(1);
        let (done_tx, done) = mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("native-shortcut".into())
            .spawn(move || {
                let _cleanup = ClearOnExit(worker_control.clone());
                let result = run(worker_control.clone(), ready_tx.clone());
                worker_control.clear();
                let _ = ready_tx.try_send(Err(result.clone().err().unwrap_or_else(failed)));
                let _ = done_tx.send(());
                result
            })
            .map_err(|_| failed())?;
        let mut registration = Registration {
            control: control.clone(),
            done,
            worker: Some(worker),
        };
        match ready.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(()))
                if !control.cancelled()
                    && !registration
                        .worker
                        .as_ref()
                        .is_some_and(|worker| worker.is_finished()) =>
            {
                control.armed.store(true, Ordering::Release);
                Ok(Box::new(registration))
            }
            result => {
                let error = match result {
                    Ok(Err(error)) => error,
                    _ => failed(),
                };
                if registration.unregister().is_err() {
                    self.state
                        .pending
                        .lock()
                        .map_err(|_| failed())?
                        .push(registration);
                }
                Err(error)
            }
        }
    }
}
impl ShortcutProvider for NativeShortcutProvider {
    fn capability(&self) -> CapabilityState {
        #[cfg(target_os = "windows")]
        let state = CapabilityState::available("win32-register-hotkey", self.active())
            .with_probe("native_registration_required");
        #[cfg(target_os = "linux")]
        let state = if crate::wayland::service_disabled("global_shortcut") {
            CapabilityState::unsupported(
                "linux-shortcut-disabled",
                UnsupportedReason::DisabledByConfiguration,
            )
        } else if crate::wayland::requested() {
            CapabilityState::unsupported(
                "wayland-global-shortcut-not-attached",
                UnsupportedReason::AdapterUnavailable,
            )
            .with_probe("no_authorized_portal_session")
        } else {
            x11::probe(self.active())
        };
        state
    }
    fn register(
        &self,
        shortcut: &Shortcut,
        mut pressed: Box<dyn FnMut() + Send>,
    ) -> PlatformResult<Box<dyn ShortcutRegistration>> {
        let chord = parse(&shortcut.accelerator)?;
        #[cfg(target_os = "linux")]
        if crate::wayland::service_disabled("global_shortcut") {
            return Err(unsupported(UnsupportedReason::DisabledByConfiguration));
        }
        #[cfg(target_os = "linux")]
        if crate::wayland::requested() {
            return Err(unsupported(UnsupportedReason::AdapterUnavailable));
        }
        self.start(move |control, ready| {
            #[cfg(target_os = "windows")]
            {
                windows::run(chord, control, ready, &mut pressed)
            }
            #[cfg(target_os = "linux")]
            {
                x11::run(chord, control, ready, &mut pressed)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn middle_button_aliases_are_only_valid_for_main_shortcuts() {
        for value in ["MouseMiddle", "MButton", "mousemiddle", "mbutton"] {
            assert_eq!(normalize_main_shortcut(value).unwrap(), "MouseMiddle");
            assert!(normalize_shortcut(value).is_err());
        }
        assert_eq!(normalize_main_shortcut("Alt+C").unwrap(), "Alt+C");
        for value in ["Ctrl+MouseMiddle", "MouseMiddle\n", "MouseLeft"] {
            assert!(normalize_main_shortcut(value).is_err());
        }
    }
    #[test]
    fn canonical_shortcuts_are_strict_and_do_not_register() {
        assert_eq!(
            normalize_shortcut(" alt + Control + p ").unwrap(),
            "Ctrl+Alt+P"
        );
        assert_eq!(
            normalize_shortcut("Win+Shift+ArrowLeft").unwrap(),
            "Shift+Super+Left"
        );
        assert_eq!(normalize_shortcut("F24").unwrap(), "F24");
        for value in [
            "",
            "Ctrl",
            "Ctrl+Ctrl+P",
            "Ctrl++P",
            "A+B",
            "Ctrl+P\n",
            "AltGr+P",
            "Ctrl+🧪",
        ] {
            assert!(normalize_shortcut(value).is_err(), "{value:?}");
        }
    }
    #[test]
    fn extended_keys_round_trip_through_native_names() {
        for chord in [
            "Ctrl+Semicolon",
            "Alt+Numpad5",
            "Shift+NumpadAdd",
            "Ctrl+Pause",
            "Ctrl+CapsLock",
            "Ctrl+NumLock",
            "Ctrl+ContextMenu",
            "Ctrl+PrintScreen",
            "Alt+MediaTrackNext",
        ] {
            assert_eq!(normalize_shortcut(chord).unwrap(), chord);
        }
        for (raw, expected) in [
            ("Ctrl+;", "Ctrl+Semicolon"),
            ("Win+NumDec", "Super+NumpadDecimal"),
            ("Ctrl+1", "Ctrl+1"),
            ("Alt+NumEnter", "Alt+NumpadEnter"),
            ("Shift+Plus", "Shift+Equal"),
        ] {
            assert_eq!(normalize_shortcut(raw).unwrap(), expected);
        }
        #[cfg(target_os = "windows")]
        for (code, expected) in [
            (0x61, "Numpad1"),
            (0x31, "1"),
            (0xbb, "Equal"),
            (0x5d, "ContextMenu"),
        ] {
            assert_eq!(windows_key_name(code), Some(expected));
        }
        #[cfg(target_os = "linux")]
        for (symbols, expected) in [
            (&[0xff9c, 0xffb1][..], "Numpad1"),
            (&[0x31, 0x21], "1"),
            (&[0x3d, 0x2b], "Equal"),
            (&[0xff7f, 0], "NumLock"),
            (&[0xff67, 0], "ContextMenu"),
        ] {
            assert_eq!(x11_key_name(symbols), Some(expected));
        }
    }
    #[test]
    fn cancellation_closes_callback_gate_even_when_wake_fails() {
        let control = Control::default();
        control.armed.store(true, Ordering::Release);
        control.install(Box::new(|| Err(failed()))).unwrap();
        let mut count = 0;
        control.pressed(&mut || count += 1);
        assert!(control.cancel().is_err());
        control.pressed(&mut || count += 1);
        assert_eq!(count, 1);
    }
    #[test]
    fn stop_timeout_preserves_worker_until_retry_confirms_join() {
        let control = Arc::new(Control::default());
        let (done_tx, done) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            release_rx.recv().unwrap();
            let _ = done_tx.send(());
            Ok(())
        });
        let mut registration = Registration {
            control,
            done,
            worker: Some(worker),
        };
        assert!(registration.stop_with_timeout(Duration::ZERO).is_err());
        assert!(registration.worker.is_some());
        release_tx.send(()).unwrap();
        registration.unregister().unwrap();
        assert!(registration.worker.is_none());
        registration.unregister().unwrap();
    }
    #[test]
    fn startup_failure_never_arms_callback_and_joins_worker() {
        let provider = NativeShortcutProvider::default();
        assert!(provider
            .start(|control, _| {
                assert!(!control.armed.load(Ordering::Acquire));
                Err(failed())
            })
            .is_err());
        provider.drain_failed_registrations().unwrap();
        assert!(!provider.active());
    }
}
