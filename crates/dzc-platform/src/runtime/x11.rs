use super::*;
use x11rb::{
    connection::Connection,
    protocol::{
        xproto::{AtomEnum, ConnectionExt},
        xtest::ConnectionExt as _,
    },
};
fn connection() -> Result<(x11rb::rust_connection::RustConnection, usize), UnsupportedReason> {
    crate::runtime_display::validate_x11_display().map_err(|error| match error {
        crate::runtime_display::DisplayUnavailable::NotSet
            if std::env::var_os("WAYLAND_DISPLAY").is_some() =>
        {
            UnsupportedReason::NativeWaylandUnavailable
        }
        crate::runtime_display::DisplayUnavailable::NotSet => UnsupportedReason::NoGraphicalSession,
        crate::runtime_display::DisplayUnavailable::InvalidDisplay => {
            UnsupportedReason::ConnectionUnavailable
        }
    })?;
    x11rb::connect(None).map_err(|_| UnsupportedReason::ConnectionUnavailable)
}
pub fn clipboard_probe() -> CapabilityState {
    let result = connection().and_then(|(conn, _)| {
        conn.get_input_focus()
            .map_err(|_| UnsupportedReason::ConnectionUnavailable)?
            .reply()
            .map(|_| ())
            .map_err(|_| UnsupportedReason::ConnectionUnavailable)
    });
    match result {
        Ok(()) => CapabilityState::available("x11", false),
        Err(reason) => CapabilityState::unsupported("x11", reason),
    }
    .with_probe("x11_roundtrip")
}
fn active(conn: &x11rb::rust_connection::RustConnection, screen: usize) -> Result<Option<u32>, ()> {
    let atom = conn
        .intern_atom(true, b"_NET_ACTIVE_WINDOW")
        .map_err(|_| ())?
        .reply()
        .map_err(|_| ())?
        .atom;
    if atom == 0 {
        return Ok(None);
    }
    Ok(conn
        .get_property(
            false,
            conn.setup().roots[screen].root,
            atom,
            AtomEnum::WINDOW,
            0,
            1,
        )
        .map_err(|_| ())?
        .reply()
        .map_err(|_| ())?
        .value32()
        .and_then(|mut values| values.next())
        .filter(|id| *id != 0))
}
pub fn source_probe() -> CapabilityState {
    let result = connection().and_then(|(conn, screen)| {
        active(&conn, screen)
            .map_err(|_| UnsupportedReason::ConnectionUnavailable)?
            .ok_or(UnsupportedReason::ExtensionUnavailable)
    });
    match result {
        Ok(_) => CapabilityState::available("x11-ewmh-active-window", false),
        Err(reason) => CapabilityState::unsupported("x11-ewmh-active-window", reason),
    }
    .with_probe("ewmh_active_window")
}
pub fn source() -> PlatformResult<SourceIdentity> {
    let (conn, screen) = connection()
        .map_err(|reason| PlatformError::unsupported(Capability::ResolveSource, reason))?;
    let window = active(&conn, screen)
        .map_err(|_| PlatformError::failed(Capability::ResolveSource))?
        .ok_or_else(|| {
            PlatformError::unsupported(
                Capability::ResolveSource,
                UnsupportedReason::ExtensionUnavailable,
            )
        })?;
    let mut identity = SourceIdentity {
        resolution: SourceResolution::ActiveWindow,
        ..Default::default()
    };
    let atom = conn
        .intern_atom(true, b"_NET_WM_PID")
        .map_err(|_| PlatformError::failed(Capability::ResolveSource))?
        .reply()
        .map_err(|_| PlatformError::failed(Capability::ResolveSource))?
        .atom;
    if atom != 0 {
        let reply = conn
            .get_property(false, window, atom, AtomEnum::CARDINAL, 0, 1)
            .map_err(|_| PlatformError::failed(Capability::ResolveSource))?
            .reply()
            .map_err(|_| PlatformError::failed(Capability::ResolveSource))?;
        if let Some(pid) = reply
            .value32()
            .and_then(|mut values| values.next())
            .filter(|pid| *pid > 0)
        {
            identity.process_path = std::fs::read_link(format!("/proc/{pid}/exe"))
                .ok()
                .map(|path| path.to_string_lossy().into_owned());
            identity.app_name = std::fs::read_to_string(format!("/proc/{pid}/comm"))
                .ok()
                .map(|name| name.trim_end().to_owned());
        }
    }
    Ok(identity)
}
fn paste_keys(conn: &x11rb::rust_connection::RustConnection) -> Result<(u8, u8), ()> {
    conn.xtest_get_version(2, 2)
        .map_err(|_| ())?
        .reply()
        .map_err(|_| ())?;
    let setup = conn.setup();
    let min = setup.min_keycode;
    let count = setup.max_keycode - min + 1;
    let mapping = conn
        .get_keyboard_mapping(min, count)
        .map_err(|_| ())?
        .reply()
        .map_err(|_| ())?;
    let find = |sym| {
        mapping
            .keysyms
            .chunks(mapping.keysyms_per_keycode as usize)
            .position(|symbols| symbols.contains(&sym))
            .map(|index| min + index as u8)
            .ok_or(())
    };
    Ok((find(0xffe3)?, find(0x76)?))
}
pub fn paste_probe() -> CapabilityState {
    let result = connection().and_then(|(conn, _)| {
        paste_keys(&conn).map_err(|_| UnsupportedReason::ExtensionUnavailable)
    });
    match result {
        Ok(_) => CapabilityState::available("x11-xtest", false),
        Err(reason) => CapabilityState::unsupported("x11-xtest", reason),
    }
    .with_probe("xtest_version_and_keymap")
}
pub fn paste() -> PlatformResult<()> {
    let (conn, _) =
        connection().map_err(|reason| PlatformError::unsupported(Capability::AutoPaste, reason))?;
    let (control, v) = paste_keys(&conn).map_err(|_| {
        PlatformError::unsupported(
            Capability::AutoPaste,
            UnsupportedReason::ExtensionUnavailable,
        )
    })?;
    for (kind, key) in [(2, control), (2, v), (3, v), (3, control)] {
        conn.xtest_fake_input(kind, key, 0, 0, 0, 0, 0)
            .map_err(|_| PlatformError::failed(Capability::AutoPaste))?
            .check()
            .map_err(|_| PlatformError::failed(Capability::AutoPaste))?;
    }
    conn.flush()
        .map_err(|_| PlatformError::failed(Capability::AutoPaste))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn isolated_x11_ports_probe_source_and_explicit_paste() {
        if std::env::var("DEZIRCLIP_RUN_LINUX_CLIPBOARD_SMOKE_TESTS").as_deref() != Ok("1") {
            return;
        }
        use x11rb::{
            protocol::{
                xproto::{CreateWindowAux, EventMask, InputFocus, PropMode, WindowClass},
                Event,
            },
            wrapper::ConnectionExt as _,
        };
        let (conn, screen) = connection().unwrap();
        let root = conn.setup().roots[screen].root;
        let window = conn.generate_id().unwrap();
        conn.create_window(
            0,
            window,
            root,
            0,
            0,
            80,
            80,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new().event_mask(EventMask::KEY_PRESS | EventMask::KEY_RELEASE),
        )
        .unwrap()
        .check()
        .unwrap();
        let active = conn
            .intern_atom(false, b"_NET_ACTIVE_WINDOW")
            .unwrap()
            .reply()
            .unwrap()
            .atom;
        let pid = conn
            .intern_atom(false, b"_NET_WM_PID")
            .unwrap()
            .reply()
            .unwrap()
            .atom;
        conn.change_property32(PropMode::REPLACE, root, active, AtomEnum::WINDOW, &[window])
            .unwrap()
            .check()
            .unwrap();
        conn.change_property32(
            PropMode::REPLACE,
            window,
            pid,
            AtomEnum::CARDINAL,
            &[std::process::id()],
        )
        .unwrap()
        .check()
        .unwrap();
        conn.map_window(window).unwrap().check().unwrap();
        conn.set_input_focus(InputFocus::PARENT, window, 0u32)
            .unwrap()
            .check()
            .unwrap();
        assert!(clipboard_probe().is_available());
        assert!(source_probe().is_available());
        assert!(paste_probe().is_available());
        let identity = source().unwrap();
        assert_eq!(identity.resolution, SourceResolution::ActiveWindow);
        assert!(identity.process_path.is_some());
        let (control, v) = paste_keys(&conn).unwrap();
        paste().unwrap();
        let mut received = Vec::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while received.len() < 4 && std::time::Instant::now() < deadline {
            match conn.poll_for_event().unwrap() {
                Some(Event::KeyPress(event)) => received.push((2, event.detail)),
                Some(Event::KeyRelease(event)) => received.push((3, event.detail)),
                _ => std::thread::sleep(std::time::Duration::from_millis(5)),
            }
        }
        conn.delete_property(root, active).unwrap().check().unwrap();
        conn.destroy_window(window).unwrap().check().unwrap();
        assert_eq!(received, vec![(2, control), (2, v), (3, v), (3, control)]);
    }
}
