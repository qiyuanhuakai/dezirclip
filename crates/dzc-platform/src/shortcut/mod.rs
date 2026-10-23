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

fn key(value: &str) -> Option<Key> {
    const LETTERS: [&str; 26] = [
        "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R",
        "S", "T", "U", "V", "W", "X", "Y", "Z",
    ];
    const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
    const FUNCTIONS: [&str; 24] = [
        "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "F13", "F14",
        "F15", "F16", "F17", "F18", "F19", "F20", "F21", "F22", "F23", "F24",
    ];
    let value = value.to_ascii_uppercase();
    if let Some(index) = LETTERS.iter().position(|entry| *entry == value) {
        return Some(Key {
            name: LETTERS[index],
            windows: 0x41 + index as u32,
            x11: 0x61 + index as u32,
        });
    }
    if let Some(index) = DIGITS.iter().position(|entry| *entry == value) {
        return Some(Key {
            name: DIGITS[index],
            windows: 0x30 + index as u32,
            x11: 0x30 + index as u32,
        });
    }
    if let Some(index) = FUNCTIONS.iter().position(|entry| *entry == value) {
        return Some(Key {
            name: FUNCTIONS[index],
            windows: 0x70 + index as u32,
            x11: 0xffbe + index as u32,
        });
    }
    let (name, windows, x11) = match value.as_str() {
        "SPACE" => ("Space", 0x20, 0x20),
        "TAB" => ("Tab", 0x09, 0xff09),
        "ENTER" | "RETURN" => ("Enter", 0x0d, 0xff0d),
        "ESC" | "ESCAPE" => ("Escape", 0x1b, 0xff1b),
        "BACKSPACE" => ("Backspace", 0x08, 0xff08),
        "DELETE" | "DEL" => ("Delete", 0x2e, 0xffff),
        "INSERT" | "INS" => ("Insert", 0x2d, 0xff63),
        "HOME" => ("Home", 0x24, 0xff50),
        "END" => ("End", 0x23, 0xff57),
        "PAGEUP" => ("PageUp", 0x21, 0xff55),
        "PAGEDOWN" => ("PageDown", 0x22, 0xff56),
        "LEFT" | "ARROWLEFT" => ("Left", 0x25, 0xff51),
        "RIGHT" | "ARROWRIGHT" => ("Right", 0x27, 0xff53),
        "UP" | "ARROWUP" => ("Up", 0x26, 0xff52),
        "DOWN" | "ARROWDOWN" => ("Down", 0x28, 0xff54),
        _ => return None,
    };
    Some(Key { name, windows, x11 })
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
