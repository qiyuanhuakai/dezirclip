use super::*;
use std::io::Write;
use std::os::{fd::AsRawFd, unix::net::UnixStream};
use x11rb::{
    connection::Connection,
    protocol::{
        xkb::{self, ConnectionExt as _},
        xproto::{ConnectionExt as _, GrabMode, ModMask},
        Event,
    },
    rust_connection::RustConnection,
};

fn connect() -> PlatformResult<(RustConnection, usize)> {
    crate::runtime_display::validate_x11_display().map_err(|error| {
        unsupported(match error {
            crate::runtime_display::DisplayUnavailable::NotSet => {
                UnsupportedReason::NoGraphicalSession
            }
            crate::runtime_display::DisplayUnavailable::InvalidDisplay => {
                UnsupportedReason::ConnectionUnavailable
            }
        })
    })?;
    x11rb::connect(None).map_err(|_| unsupported(UnsupportedReason::ConnectionUnavailable))
}
fn repeat(conn: &RustConnection) -> PlatformResult<()> {
    let extension = conn
        .xkb_use_extension(1, 0)
        .map_err(|_| failed())?
        .reply()
        .map_err(|_| failed())?;
    if !extension.supported {
        return Err(unsupported(UnsupportedReason::ExtensionUnavailable));
    }
    let flags =
        xkb::PerClientFlag::DETECTABLE_AUTO_REPEAT | xkb::PerClientFlag::GRABS_USE_XKB_STATE;
    let result = conn
        .xkb_per_client_flags(
            xkb::ID::USE_CORE_KBD.into(),
            flags,
            flags,
            0u32.into(),
            0u32.into(),
            0u32.into(),
        )
        .map_err(|_| failed())?
        .reply()
        .map_err(|_| failed())?;
    if u32::from(result.supported) & u32::from(flags) != u32::from(flags)
        || u32::from(result.value) & u32::from(flags) != u32::from(flags)
    {
        return Err(unsupported(UnsupportedReason::ExtensionUnavailable));
    }
    Ok(())
}
pub(super) fn probe(active: bool) -> CapabilityState {
    let result = connect().and_then(|(conn, _)| {
        repeat(&conn)?;
        group_one(&conn)
    });
    match result {
        Ok(()) => CapabilityState::available("x11-grab-key", active),
        Err(error) => CapabilityState::unsupported(
            "x11-grab-key",
            error
                .reason
                .unwrap_or(UnsupportedReason::ConnectionUnavailable),
        ),
    }
    .with_probe("xkb_detectable_repeat_roundtrip")
}

fn group_one(conn: &RustConnection) -> PlatformResult<()> {
    let state = conn
        .xkb_get_state(xkb::ID::USE_CORE_KBD.into())
        .map_err(|_| failed())?
        .reply()
        .map_err(|_| failed())?;
    if state.group != xkb::Group::M1
        || state.locked_group != xkb::Group::M1
        || state.base_group != 0
        || state.latched_group != 0
    {
        return Err(unsupported(UnsupportedReason::UnsupportedRepresentation));
    }
    Ok(())
}

#[derive(Clone, PartialEq, Eq)]
struct Mapping {
    first: u8,
    width: usize,
    symbols: Vec<u32>,
    modifiers: Vec<u8>,
}
impl Mapping {
    fn read(conn: &RustConnection) -> PlatformResult<Self> {
        let first = conn.setup().min_keycode;
        let count = conn
            .setup()
            .max_keycode
            .checked_sub(first)
            .and_then(|n| n.checked_add(1))
            .ok_or_else(failed)?;
        let keys = conn
            .get_keyboard_mapping(first, count)
            .map_err(|_| failed())?
            .reply()
            .map_err(|_| failed())?;
        if keys.keysyms_per_keycode == 0 {
            return Err(unsupported(UnsupportedReason::UnsupportedRepresentation));
        }
        let modifiers = conn
            .get_modifier_mapping()
            .map_err(|_| failed())?
            .reply()
            .map_err(|_| failed())?;
        Ok(Self {
            first,
            width: keys.keysyms_per_keycode as usize,
            symbols: keys.keysyms,
            modifiers: modifiers.keycodes,
        })
    }
    fn base(&self, code: u8) -> Option<u32> {
        self.symbols
            .get(code.checked_sub(self.first)? as usize * self.width)
            .copied()
    }
    fn modifier(&self, symbols: &[u32]) -> PlatformResult<u16> {
        if self.modifiers.len() % 8 != 0 {
            return Err(failed());
        }
        let width = self.modifiers.len() / 8;
        if width == 0 {
            return Err(unsupported(UnsupportedReason::UnsupportedRepresentation));
        }
        let mut found: u16 = 0;
        for (index, group) in self.modifiers.chunks(width).enumerate() {
            if group.iter().any(|code| {
                *code != 0
                    && self
                        .base(*code)
                        .is_some_and(|symbol| symbols.contains(&symbol))
            }) {
                found |= 1 << index;
            }
        }
        if found.count_ones() != 1 {
            return Err(unsupported(UnsupportedReason::UnsupportedRepresentation));
        }
        Ok(found)
    }
    fn binding(&self, chord: &Chord) -> PlatformResult<(u8, Vec<u16>)> {
        let mut mask = 0;
        for (flag, symbols) in [
            (CTRL, &[0xffe3, 0xffe4][..]),
            (ALT, &[0xffe9, 0xffea][..]),
            (SHIFT, &[0xffe1, 0xffe2][..]),
            (SUPER, &[0xffeb, 0xffec][..]),
        ] {
            if chord.modifiers & flag != 0 {
                let native = self.modifier(symbols)?;
                if mask & native != 0 {
                    return Err(unsupported(UnsupportedReason::UnsupportedRepresentation));
                }
                mask |= native;
            }
        }
        let code = self
            .symbols
            .chunks(self.width)
            .enumerate()
            .find_map(|(index, row)| {
                let code = self.first.checked_add(u8::try_from(index).ok()?)?;
                (row.first() == Some(&chord.key.x11) && !self.modifiers.contains(&code))
                    .then_some(code)
            })
            .ok_or_else(|| unsupported(UnsupportedReason::UnsupportedRepresentation))?;
        let mut ignored = 0;
        for lock in [0xffe5, 0xff7f, 0xff14] {
            if let Ok(native) = self.modifier(&[lock]) {
                ignored |= native;
            }
        }
        if mask & ignored != 0 {
            return Err(unsupported(UnsupportedReason::UnsupportedRepresentation));
        }
        let mut masks = vec![mask];
        for bit in 0..8 {
            let native = 1 << bit;
            if ignored & native != 0 {
                let extra: Vec<_> = masks.iter().map(|mask| mask | native).collect();
                masks.extend(extra);
            }
        }
        Ok((code, masks))
    }
}

pub(super) fn run(
    chord: Chord,
    control: Arc<Control>,
    ready: mpsc::SyncSender<PlatformResult<()>>,
    callback: &mut dyn FnMut(),
) -> PlatformResult<()> {
    let (conn, screen) = connect()?;
    repeat(&conn)?;
    group_one(&conn)?;
    let mapping = Mapping::read(&conn)?;
    let (keycode, masks) = mapping.binding(&chord)?;
    let held = conn
        .query_keymap()
        .map_err(|_| failed())?
        .reply()
        .map_err(|_| failed())?;
    if held.keys[usize::from(keycode) / 8] & (1 << (keycode % 8)) != 0 {
        return Err(unsupported(UnsupportedReason::UnsupportedRepresentation));
    }
    let root = conn.setup().roots[screen].root;
    let (cancel, wake) = UnixStream::pair().map_err(|_| failed())?;
    wake.set_nonblocking(true).map_err(|_| failed())?;
    control.install(Box::new(move || match (&wake).write(&[1]) {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(()),
        Err(_) => Err(failed()),
    }))?;
    if control.cancelled() {
        return Ok(());
    }
    // Each checked request is part of one connection: any failed partial set is
    // removed when that connection is dropped, before registration returns Err.
    for mask in &masks {
        if control.cancelled() {
            return Ok(());
        }
        conn.grab_key(
            false,
            root,
            ModMask::from(*mask),
            keycode,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
        )
        .map_err(|_| failed())?
        .check()
        .map_err(|_| unsupported(UnsupportedReason::PermissionDenied))?;
    }
    conn.flush().map_err(|_| failed())?;
    control.running.store(true, Ordering::Release);
    let result = (|| {
        if control.cancelled() {
            return Ok(());
        }
        ready.send(Ok(())).map_err(|_| failed())?;
        let mut held = false;
        loop {
            if control.cancelled() {
                return Ok(());
            }
            while let Some(event) = conn.poll_for_event().map_err(|_| failed())? {
                match event {
                    Event::KeyPress(event)
                        if event.detail == keycode && event.response_type & 0x80 == 0 =>
                    {
                        // XKB puts the event's effective group in bits13/14.
                        // This adapter supports group-one base-symbol shortcuts.
                        if u16::from(event.state) & 0x6000 == 0 && !held {
                            if Mapping::read(&conn)? != mapping {
                                return Err(unsupported(
                                    UnsupportedReason::UnsupportedRepresentation,
                                ));
                            }
                            held = true;
                            control.pressed(callback);
                        }
                    }
                    Event::KeyRelease(event)
                        if event.detail == keycode && event.response_type & 0x80 == 0 =>
                    {
                        held = false
                    }
                    Event::MappingNotify(_) => {
                        return Err(unsupported(UnsupportedReason::UnsupportedRepresentation))
                    }
                    _ => (),
                }
            }
            match crate::wayland::io::wait(
                conn.stream().as_raw_fd(),
                libc::POLLIN,
                Some(&cancel),
                std::time::Instant::now() + Duration::from_secs(24 * 60 * 60),
            ) {
                Ok(()) | Err(crate::wayland::WaylandError::TimedOut) => (),
                Err(crate::wayland::WaylandError::Cancelled) => return Ok(()),
                Err(_) => return Err(failed()),
            }
        }
    })();
    control.clear();
    let mut cleanup = Ok(());
    for mask in masks {
        if conn
            .ungrab_key(keycode, root, ModMask::from(mask))
            .map_err(|_| failed())
            .and_then(|cookie| cookie.check().map_err(|_| failed()))
            .is_err()
        {
            cleanup = Err(failed());
        }
    }
    let _ = conn.flush();
    result.and(cleanup)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mapping_uses_base_symbol_and_real_modifier_map_not_arbitrary_slots() {
        let mapping = Mapping {
            first: 8,
            width: 2,
            symbols: vec![0xffe3, 0, 0xffe9, 0, 0x70, 0x50, 0x78, 0x70, 0xffe5, 0],
            modifiers: vec![0, 12, 8, 9, 0, 0, 0, 0],
        };
        let (code, masks) = mapping.binding(&parse("Ctrl+Alt+P").unwrap()).unwrap();
        assert_eq!(code, 10);
        assert_eq!(masks, vec![12, 14]);
        let mut remapped = mapping;
        remapped.symbols[4] = 0x78;
        assert!(remapped.binding(&parse("Ctrl+Alt+P").unwrap()).is_err());
    }
    #[test]
    fn ambiguous_modifier_mapping_is_rejected() {
        let mapping = Mapping {
            first: 8,
            width: 1,
            symbols: vec![0xffe3, 0x70],
            modifiers: vec![8, 0, 8, 0, 0, 0, 0, 0],
        };
        assert!(mapping.binding(&parse("Ctrl+P").unwrap()).is_err());
    }

    #[test]
    fn isolated_disabled_shortcuts_never_connect_or_register() {
        if std::env::var("DEZIRCLIP_RUN_LINUX_SHORTCUT_SMOKE_TESTS").as_deref() != Ok("1")
            || !crate::wayland::service_disabled("global_shortcut")
        {
            return;
        }
        let runtime = std::path::PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap());
        assert!(runtime.join("dezirclip-isolated-wayland-fixture").is_file());
        assert_eq!(std::env::var("DISPLAY").unwrap(), ":65530");
        let provider = NativeShortcutProvider::default();
        let result = provider.register(
            &Shortcut {
                id: "synthetic".into(),
                accelerator: "Ctrl+Alt+P".into(),
            },
            Box::new(|| panic!("disabled callback cannot arm")),
        );
        assert_eq!(
            result.err(),
            Some(unsupported(UnsupportedReason::DisabledByConfiguration))
        );
        assert!(!provider.capability().is_available());
        assert!(!provider.capability().active);
    }

    #[test]
    fn isolated_wayland_shortcuts_are_unsupported_before_x11_access() {
        if std::env::var("DEZIRCLIP_RUN_LINUX_SHORTCUT_SMOKE_TESTS").as_deref() != Ok("1")
            || std::env::var("XDG_SESSION_TYPE").as_deref() != Ok("wayland")
        {
            return;
        }
        let runtime = std::path::PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap());
        assert!(runtime.join("dezirclip-isolated-wayland-fixture").is_file());
        assert_eq!(std::env::var("DISPLAY").unwrap(), ":65530");
        let provider = NativeShortcutProvider::default();
        let expected = unsupported(UnsupportedReason::AdapterUnavailable);
        let result = provider.register(
            &Shortcut {
                id: "synthetic".into(),
                accelerator: "Ctrl+Alt+P".into(),
            },
            Box::new(|| panic!("Wayland callback cannot arm")),
        );
        assert_eq!(result.err(), Some(expected));
        assert!(!provider.capability().is_available());
        assert!(!provider.capability().active);
    }

    #[test]
    fn isolated_x11_shortcuts_conflict_repeat_release_and_re_register() {
        if std::env::var("DEZIRCLIP_RUN_LINUX_SHORTCUT_SMOKE_TESTS").as_deref() != Ok("1") {
            return;
        }
        let runtime = std::path::PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap());
        assert!(runtime.join("dezirclip-isolated-wayland-fixture").is_file());
        assert!(std::env::var_os("WAYLAND_DISPLAY").is_none());
        use x11rb::protocol::{
            xproto::{KEY_PRESS_EVENT, KEY_RELEASE_EVENT},
            xtest::ConnectionExt as _,
        };
        let (conn, screen) = connect().unwrap();
        let mapping = Mapping::read(&conn).unwrap();
        let lookup = |symbol| {
            mapping
                .symbols
                .chunks(mapping.width)
                .enumerate()
                .find_map(|(index, row)| {
                    (row.first() == Some(&symbol)).then_some(mapping.first + index as u8)
                })
                .unwrap()
        };
        let control = lookup(0xffe3);
        let alt = lookup(0xffe9);
        let p = lookup(0x70);
        let root = conn.setup().roots[screen].root;
        let inject = |kind, code| {
            conn.xtest_fake_input(kind, code, x11rb::CURRENT_TIME, root, 0, 0, 0)
                .unwrap()
                .check()
                .unwrap();
            conn.flush().unwrap();
        };
        let press = || {
            inject(KEY_PRESS_EVENT, control);
            inject(KEY_PRESS_EVENT, alt);
            inject(KEY_PRESS_EVENT, p);
        };
        let release = || {
            inject(KEY_RELEASE_EVENT, p);
            inject(KEY_RELEASE_EVENT, alt);
            inject(KEY_RELEASE_EVENT, control);
        };
        let provider = NativeShortcutProvider::default();
        assert!(provider.capability().is_available());
        let chord = Shortcut {
            id: "synthetic-show".into(),
            accelerator: "Ctrl+Alt+P".into(),
        };
        let (tx, rx) = mpsc::channel();
        let mut registration = provider
            .register(
                &chord,
                Box::new(move || {
                    tx.send(()).unwrap();
                }),
            )
            .unwrap();
        assert!(provider.capability().active);
        let second = NativeShortcutProvider::default();
        assert!(second
            .register(
                &chord,
                Box::new(|| panic!("conflicting callback must never arm"))
            )
            .is_err());
        second.drain_failed_registrations().unwrap();
        press();
        rx.recv_timeout(Duration::from_secs(2)).unwrap();
        inject(KEY_PRESS_EVENT, p);
        inject(KEY_PRESS_EVENT, p);
        std::thread::sleep(Duration::from_millis(850));
        assert!(
            rx.try_recv().is_err(),
            "held key autorepeat must not trigger again"
        );
        release();
        assert!(
            rx.recv_timeout(Duration::from_millis(80)).is_err(),
            "key release is not activation"
        );
        press();
        rx.recv_timeout(Duration::from_secs(2)).unwrap();
        release();
        registration.unregister().unwrap();
        assert!(!provider.capability().active);
        registration.unregister().unwrap();
        press();
        release();
        assert!(rx.recv_timeout(Duration::from_millis(80)).is_err());
        let (tx, rx) = mpsc::channel();
        let mut next = second
            .register(
                &chord,
                Box::new(move || {
                    tx.send(()).unwrap();
                }),
            )
            .unwrap();
        press();
        rx.recv_timeout(Duration::from_secs(2)).unwrap();
        release();
        next.unregister().unwrap();
        assert!(!second.capability().active);
        println!("actual X11 GrabKey conflict, native press/repeat/release, unregister and reacquisition passed");
    }
}
