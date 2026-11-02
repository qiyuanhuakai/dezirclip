use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;
use tauri::Emitter;

use crate::global_state::{GLOBAL_APP_HANDLE, IS_RECORDING};

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt, EventMask, GrabMode, GrabStatus, ModMask};

const XK_ESCAPE: u32 = 0xFF1B;
const XK_SPACE: u32 = 0x0020;
const XK_RETURN: u32 = 0xFF0D;
const XK_TAB: u32 = 0xFF09;
const XK_BACKSPACE: u32 = 0xFF08;
const XK_DELETE: u32 = 0xFFFF;
const XK_INSERT: u32 = 0xFF63;
const XK_PAGE_UP: u32 = 0xFF55;
const XK_PAGE_DOWN: u32 = 0xFF56;
const XK_HOME: u32 = 0xFF50;
const XK_END: u32 = 0xFF57;
const XK_LEFT: u32 = 0xFF51;
const XK_UP: u32 = 0xFF52;
const XK_RIGHT: u32 = 0xFF53;
const XK_DOWN: u32 = 0xFF54;
const XK_F1: u32 = 0xFFBE;
const XK_F12: u32 = 0xFFC9;
const XK_PLUS: u32 = 0x002B;
const XK_COMMA: u32 = 0x002C;
const XK_MINUS: u32 = 0x002D;
const XK_PERIOD: u32 = 0x002E;
const XK_SLASH: u32 = 0x002F;
const XK_GRAVE: u32 = 0x0060;
const XK_SEMICOLON: u32 = 0x003B;
const XK_BRACKETLEFT: u32 = 0x005B;
const XK_BACKSLASH: u32 = 0x005C;
const XK_BRACKETRIGHT: u32 = 0x005D;
const XK_APOSTROPHE: u32 = 0x0027;

struct GrabGuard<'a> {
    conn: &'a x11rb::rust_connection::RustConnection,
}

impl<'a> Drop for GrabGuard<'a> {
    fn drop(&mut self) {
        let _ = self.conn.ungrab_keyboard(x11rb::CURRENT_TIME);
        let _ = self.conn.ungrab_pointer(x11rb::CURRENT_TIME);
        let _ = self.conn.flush();
    }
}

#[derive(Debug, PartialEq)]
enum RecordedInput { Chord(String), Cancelled }

pub fn start_recording_grab() {
    thread::spawn(|| match recording_thread(|| {}) {
        Ok(Some(input)) => {
            IS_RECORDING.store(false, Ordering::SeqCst);
            if let Some(handle) = GLOBAL_APP_HANDLE.get() {
                match input {
                    RecordedInput::Chord(chord) => { let _ = handle.emit("hotkey-recorded", chord); }
                    RecordedInput::Cancelled => { let _ = handle.emit("recording-cancelled", ()); }
                }
            }
        }
        Ok(None) => {}
        Err(error) => eprintln!("input_grab error: {error}"),
    });
}

fn recording_thread(on_grabbed: impl FnOnce()) -> Result<Option<RecordedInput>, Box<dyn std::error::Error>> {
    let (conn, screen_num) = match x11rb::connect(None) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Failed to connect to X11: {}", e);
            return Ok(None);
        }
    };

    // XKB detectable repeat is per connection. On legacy servers the release path also
    // consults the live keymap before accepting a core autorepeat release/press pair.
    let _ = dzc_platform::shortcut::enable_x11_shortcut_events(&conn);
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;

    let kb_cookie = conn.grab_keyboard(
        false,
        root,
        x11rb::CURRENT_TIME,
        GrabMode::ASYNC,
        GrabMode::ASYNC,
    )?;
    let kb_reply = kb_cookie.reply()?;
    if kb_reply.status != GrabStatus::SUCCESS {
        eprintln!("Failed to grab keyboard: {:?}", kb_reply.status);
        return Ok(None);
    }

    let ptr_cookie = conn.grab_pointer(
        false,
        root,
        EventMask::BUTTON_PRESS,
        GrabMode::ASYNC,
        GrabMode::ASYNC,
        x11rb::NONE,
        x11rb::NONE,
        x11rb::CURRENT_TIME,
    )?;
    let ptr_reply = ptr_cookie.reply()?;
    if ptr_reply.status != GrabStatus::SUCCESS {
        eprintln!("Failed to grab pointer: {:?}", ptr_reply.status);
        let _ = conn.ungrab_keyboard(x11rb::CURRENT_TIME);
        let _ = conn.flush();
        return Ok(None);
    }

    let _guard = GrabGuard { conn: &conn };
    conn.flush()?;
    on_grabbed();

    let mut recorded = None;
    while IS_RECORDING.load(Ordering::SeqCst) {
        match conn.poll_for_event()? {
            Some(event) => match event {
                x11rb::protocol::Event::KeyPress(event) => {
                    let ctrl = (u16::from(event.state) & u16::from(ModMask::CONTROL)) != 0;
                    let shift = (u16::from(event.state) & u16::from(ModMask::SHIFT)) != 0;
                    let alt = (u16::from(event.state) & u16::from(ModMask::M1)) != 0;
                    let win = (u16::from(event.state) & u16::from(ModMask::M4)) != 0;

                    let reply = conn.get_keyboard_mapping(event.detail, 1)?.reply()?;
                    let keysym = reply.keysyms.first().copied().unwrap_or(0);

                    if is_modifier_key(keysym) {
                        continue;
                    }

                    if keysym == XK_ESCAPE {
                        return Ok(Some(RecordedInput::Cancelled));
                    }

                    let key_name = keysym_to_name(keysym);
                    let hotkey = build_hotkey_string(ctrl, shift, alt, win, &key_name);

                    recorded.get_or_insert((event.detail, hotkey));
                }
                x11rb::protocol::Event::KeyRelease(event) => {
                    if !recorded.as_ref().is_some_and(|(key, _)| *key == event.detail) { continue; }
                    let held = conn.query_keymap()?.reply()?;
                    let key = usize::from(event.detail);
                    if held.keys[key / 8] & (1 << (key % 8)) != 0 { continue; }
                    return Ok(recorded.take().map(|(_, chord)| RecordedInput::Chord(chord)));
                }
                x11rb::protocol::Event::ButtonPress(event) => {
                    if event.detail == 2 {
                        return Ok(Some(RecordedInput::Chord("MouseMiddle".into())));
                    }
                }
                _ => {}
            },
            None => {
                thread::sleep(Duration::from_millis(10));
            }
        }
    }

    Ok(None)
}

fn is_modifier_key(keysym: u32) -> bool {
    matches!(
        keysym,
        0xFFE1
            | 0xFFE2
            | 0xFFE3
            | 0xFFE4
            | 0xFFE9
            | 0xFFEA
            | 0xFFE7
            | 0xFFE8
            | 0xFFEB
            | 0xFFEC
            | 0xFFED
            | 0xFFEE
            | 0xFE03
            | 0xFF7E
    )
}

fn keysym_to_name(keysym: u32) -> String {
    match keysym {
        XK_SPACE => "Space".to_string(),
        XK_RETURN => "Enter".to_string(),
        XK_TAB => "Tab".to_string(),
        XK_BACKSPACE => "Backspace".to_string(),
        XK_DELETE => "Delete".to_string(),
        XK_INSERT => "Insert".to_string(),
        XK_PAGE_UP => "PageUp".to_string(),
        XK_PAGE_DOWN => "PageDown".to_string(),
        XK_HOME => "Home".to_string(),
        XK_END => "End".to_string(),
        XK_LEFT => "Left".to_string(),
        XK_UP => "Up".to_string(),
        XK_RIGHT => "Right".to_string(),
        XK_DOWN => "Down".to_string(),
        XK_PLUS => "Plus".to_string(),
        XK_COMMA => "Comma".to_string(),
        XK_MINUS => "Minus".to_string(),
        XK_PERIOD => "Period".to_string(),
        XK_SLASH => "/".to_string(),
        XK_GRAVE => "`".to_string(),
        XK_SEMICOLON => ";".to_string(),
        XK_BRACKETLEFT => "[".to_string(),
        XK_BACKSLASH => "\\".to_string(),
        XK_BRACKETRIGHT => "]".to_string(),
        XK_APOSTROPHE => "'".to_string(),
        k if k >= XK_F1 && k <= XK_F12 => format!("F{}", k - XK_F1 + 1),
        k if k >= 0x30 && k <= 0x39 => format!("{}", char::from_u32(k).unwrap()),
        k if k >= 0x41 && k <= 0x5A => format!("{}", char::from_u32(k).unwrap()),
        k if k >= 0x61 && k <= 0x7A => {
            format!("{}", char::from_u32(k).unwrap().to_ascii_uppercase())
        }
        _ => format!("Key_{}", keysym),
    }
}

fn build_hotkey_string(ctrl: bool, shift: bool, alt: bool, win: bool, key_name: &str) -> String {
    format!(
        "{}{}{}{}{}",
        if ctrl { "Ctrl+" } else { "" },
        if shift { "Shift+" } else { "" },
        if alt { "Alt+" } else { "" },
        if win { "Win+" } else { "" },
        key_name
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keysym_to_name() {
        assert_eq!(keysym_to_name(XK_ESCAPE), "Key_65307");
        assert_eq!(keysym_to_name(XK_SPACE), "Space");
        assert_eq!(keysym_to_name(XK_F1), "F1");
        assert_eq!(keysym_to_name(0x41), "A");
        assert_eq!(keysym_to_name(0x31), "1");
        assert_eq!(keysym_to_name(XK_PLUS), "Plus");
    }

    #[test]
    fn test_build_hotkey_string() {
        assert_eq!(
            build_hotkey_string(true, false, false, false, "A"),
            "Ctrl+A"
        );
        assert_eq!(
            build_hotkey_string(true, true, true, true, "F1"),
            "Ctrl+Shift+Alt+Win+F1"
        );
        assert_eq!(
            build_hotkey_string(false, false, false, false, "Space"),
            "Space"
        );
        assert_eq!(
            build_hotkey_string(false, true, false, true, "1"),
            "Shift+Win+1"
        );
    }
}

#[cfg(test)]
mod native_repeat_tests {
    use super::*;
    use dzc_core::ports::{Shortcut, ShortcutProvider};
    use dzc_platform::shortcut::NativeShortcutProvider;
    use std::sync::mpsc;
    use x11rb::protocol::{xproto::{KEY_PRESS_EVENT, KEY_RELEASE_EVENT}, xtest::ConnectionExt as _};

    #[test]
    #[ignore = "requires a disposable Xvfb display with keyboard autorepeat enabled"]
    fn held_key_is_saved_only_after_release_and_registers_on_x11() {
        let runtime = std::path::PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap());
        assert!(runtime.join("dezirclip-isolated-x11-recorder").is_file());
        let (injector, screen) = x11rb::connect(None).unwrap();
        let root = injector.setup().roots[screen].root;
        let first = injector.setup().min_keycode;
        let map = injector.get_keyboard_mapping(first, injector.setup().max_keycode - first + 1).unwrap().reply().unwrap();
        let keycode = |symbol| first + map.keysyms.chunks(usize::from(map.keysyms_per_keycode))
            .position(|row| row[0] == symbol).unwrap() as u8;
        let ctrl = keycode(0xffe3);
        let p = keycode(0x70);
        let send = |kind, key| { injector.xtest_fake_input(kind, key, 0, root, 0, 0, 0).unwrap().check().unwrap(); };
        // Prove this server actually produces legacy synthetic releases while the key is held.
        let (legacy, _) = x11rb::connect(None).unwrap();
        assert_eq!(legacy.grab_keyboard(false, root, 0u32, GrabMode::ASYNC, GrabMode::ASYNC).unwrap().reply().unwrap().status, GrabStatus::SUCCESS);
        send(KEY_PRESS_EVENT, p);
        thread::sleep(Duration::from_millis(900));
        assert_ne!(injector.query_keymap().unwrap().reply().unwrap().keys[usize::from(p)/8] & (1 << (p % 8)), 0);
        let mut releases = 0;
        while let Some(event) = legacy.poll_for_event().unwrap() {
            if matches!(event, x11rb::protocol::Event::KeyRelease(key) if key.detail == p) { releases += 1; }
        }
        send(KEY_RELEASE_EVENT, p);
        legacy.ungrab_keyboard(0u32).unwrap().check().unwrap();
        assert!(releases > 0, "the fixture must generate autorepeat");

        let (ready, grabbed) = mpsc::channel();
        let (finished, result) = mpsc::channel();
        IS_RECORDING.store(true, Ordering::SeqCst);
        let worker = thread::spawn(move || {
            let chord = recording_thread(|| ready.send(()).unwrap()).unwrap();
            finished.send(chord).unwrap();
        });
        grabbed.recv_timeout(Duration::from_secs(3)).unwrap();
        send(KEY_PRESS_EVENT, ctrl);
        send(KEY_PRESS_EVENT, p);
        assert_eq!(result.recv_timeout(Duration::from_millis(900)), Err(mpsc::RecvTimeoutError::Timeout));
        send(KEY_RELEASE_EVENT, p);
        let Some(RecordedInput::Chord(chord)) = result.recv_timeout(Duration::from_secs(3)).unwrap() else { panic!("missing chord") };
        assert_eq!(chord, "Ctrl+P");
        IS_RECORDING.store(false, Ordering::SeqCst);
        worker.join().unwrap();
        send(KEY_RELEASE_EVENT, ctrl);
        let provider = NativeShortcutProvider::default();
        let (pressed, press) = mpsc::channel();
        let mut registration = provider.register(&Shortcut { id: "recorded".into(), accelerator: chord }, Box::new(move || {
            let _ = pressed.send(());
        })).unwrap();
        send(KEY_PRESS_EVENT, ctrl); send(KEY_PRESS_EVENT, p);
        press.recv_timeout(Duration::from_secs(3)).unwrap();
        send(KEY_RELEASE_EVENT, p); send(KEY_RELEASE_EVENT, ctrl);
        registration.unregister().unwrap();
    }
}
