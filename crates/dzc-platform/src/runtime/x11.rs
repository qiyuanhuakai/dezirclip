use super::*;
use x11rb::{
    connection::Connection,
    protocol::{
        xkb::{self, ConnectionExt as _},
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
fn idle_keyboard(conn: &x11rb::rust_connection::RustConnection) -> Result<(), ()> {
    let state = conn
        .xkb_get_state(xkb::ID::USE_CORE_KBD.into())
        .map_err(|_| ())?
        .reply()
        .map_err(|_| ())?;
    if u16::from(state.mods) != 0
        || u16::from(state.base_mods) != 0
        || u16::from(state.latched_mods) != 0
        || u16::from(state.locked_mods) != 0
        || u16::from(state.lookup_mods) != 0
        || u16::from(state.compat_state) != 0
        || state.group != xkb::Group::M1
        || state.locked_group != xkb::Group::M1
        || state.base_group != 0
        || state.latched_group != 0
        || u16::from(state.ptr_btn_state) != 0
    {
        return Err(());
    }
    let keys = conn
        .query_keymap()
        .map_err(|_| ())?
        .reply()
        .map_err(|_| ())?;
    if keys.keys.iter().any(|byte| *byte != 0) {
        return Err(());
    }
    Ok(())
}

// This subset has an exact XKB translation: group one, no existing modifiers,
// and a key type that cannot consume Control or select another level at zero.
// Other groups, latches, held keys and stateful actions are unsupported.
fn group_one_level_zero(map: &xkb::GetMapReply, key: &xkb::KeySymMap) -> Option<u32> {
    if key.group_info & 0x0f == 0 || key.width == 0 {
        return None;
    }
    let index = key.kt_index[0].checked_sub(map.first_type)? as usize;
    let kind = map.map.types_rtrn.as_ref()?.get(index)?;
    if kind.num_levels == 0
        || kind.num_levels > key.width
        || u16::from(kind.mods_vmods) != 0
        || u16::from(kind.mods_mask) & u16::from(x11rb::protocol::xproto::ModMask::CONTROL) != 0
        || kind
            .map
            .iter()
            .any(|entry| entry.active && u16::from(entry.mods_mask) == 0 && entry.level != 0)
    {
        return None;
    }
    key.syms.first().copied()
}

fn key_actions(map: &xkb::GetMapReply, keycode: u8) -> Option<&[xkb::Action]> {
    let actions = map.map.key_actions.as_ref()?;
    let index = keycode.checked_sub(map.first_key_action)? as usize;
    let count = *actions.acts_rtrn_count.get(index)? as usize;
    let start: usize = actions.acts_rtrn_count[..index]
        .iter()
        .map(|count| *count as usize)
        .sum();
    actions.acts_rtrn_acts.get(start..start + count)
}

fn mapped_modifiers(map: &xkb::GetMapReply, keycode: u8) -> Option<u16> {
    Some(
        map.map
            .modmap_rtrn
            .as_ref()?
            .iter()
            .filter(|entry| entry.keycode == keycode)
            .fold(0, |mask, entry| mask | u16::from(entry.mods)),
    )
}

fn default_behavior(map: &xkb::GetMapReply, keycode: u8) -> bool {
    map.map.behaviors_rtrn.as_ref().is_some_and(|entries| {
        entries
            .iter()
            .all(|entry| entry.keycode != keycode || entry.behavior.as_common().type_ == 0)
    })
}

fn select_paste_keys(map: &xkb::GetMapReply) -> Option<(u8, u8)> {
    use x11rb::protocol::xproto::ModMask;
    let control_mask = u16::from(ModMask::CONTROL);
    let mut control = None;
    let mut v = None;
    for (index, key) in map.map.syms_rtrn.as_ref()?.iter().enumerate() {
        let keycode = map.first_key_sym.checked_add(u8::try_from(index).ok()?)?;
        if !default_behavior(map, keycode) {
            continue;
        }
        let Some(symbol) = group_one_level_zero(map, key) else {
            continue;
        };
        let Some(actions) = key_actions(map, keycode) else {
            continue;
        };
        if symbol == 0x76
            && mapped_modifiers(map, keycode) == Some(0)
            && actions
                .iter()
                .all(|action| action.as_noaction().type_ == xkb::SAType::NO_ACTION)
        {
            v.get_or_insert(keycode);
        }
        if matches!(symbol, 0xffe3 | 0xffe4)
            && mapped_modifiers(map, keycode) == Some(control_mask)
            && !actions.is_empty()
            && actions.iter().all(|action| {
                if action.as_noaction().type_ != xkb::SAType::SET_MODS {
                    return false;
                }
                let action = action.as_setmods();
                let flags = u8::from(action.flags);
                u16::from(action.mask) == control_mask
                    && u8::from(action.vmods_high) == 0
                    && u8::from(action.vmods_low) == 0
                    && flags & !u8::from(xkb::SA::CLEAR_LOCKS | xkb::SA::USE_MOD_MAP_MODS) == 0
                    && (u16::from(action.real_mods) == control_mask
                        || flags & u8::from(xkb::SA::USE_MOD_MAP_MODS) != 0)
            })
        {
            control.get_or_insert(keycode);
        }
    }
    Some((control?, v?))
}

fn paste_keys(
    conn: &x11rb::rust_connection::RustConnection,
) -> Result<(u8, u8), UnsupportedReason> {
    let missing = UnsupportedReason::ExtensionUnavailable;
    let unsafe_state = UnsupportedReason::BackendUnavailable;
    conn.xtest_get_version(2, 2)
        .map_err(|_| missing)?
        .reply()
        .map_err(|_| missing)?;
    if !conn
        .xkb_use_extension(1, 0)
        .map_err(|_| missing)?
        .reply()
        .map_err(|_| missing)?
        .supported
    {
        return Err(missing);
    }
    idle_keyboard(conn).map_err(|_| unsafe_state)?;
    let controls = conn
        .xkb_get_controls(xkb::ID::USE_CORE_KBD.into())
        .map_err(|_| missing)?
        .reply()
        .map_err(|_| missing)?;
    // Acceleration has no effect while MouseKeys is off; the timeout may only
    // keep or disable this supported subset, never enable input transformations.
    let allowed = xkb::BoolCtrl::REPEAT_KEYS
        | xkb::BoolCtrl::AUDIBLE_BELL_MASK
        | xkb::BoolCtrl::ACCESS_X_FEEDBACK_MASK
        | xkb::BoolCtrl::IGNORE_GROUP_LOCK_MASK
        | xkb::BoolCtrl::MOUSE_KEYS_ACCEL
        | xkb::BoolCtrl::ACCESS_X_TIMEOUT_MASK;
    if u32::from(controls.enabled_controls) & !u32::from(allowed) != 0
        || u16::from(controls.internal_mods_mask) != 0
        || (controls.access_x_timeout != 0
            && u32::from(controls.access_x_timeout_mask)
                & u32::from(controls.access_x_timeout_values)
                & !u32::from(allowed)
                != 0)
    {
        return Err(unsafe_state);
    }
    let parts = xkb::MapPart::KEY_TYPES
        | xkb::MapPart::KEY_SYMS
        | xkb::MapPart::KEY_ACTIONS
        | xkb::MapPart::KEY_BEHAVIORS
        | xkb::MapPart::MODIFIER_MAP;
    let map = conn
        .xkb_get_map(
            xkb::ID::USE_CORE_KBD.into(),
            parts,
            xkb::MapPart::default(),
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            xkb::VMod::default(),
            0,
            0,
            0,
            0,
            0,
            0,
        )
        .map_err(|_| missing)?
        .reply()
        .map_err(|_| missing)?;
    let keys = select_paste_keys(&map).ok_or(unsafe_state)?;
    idle_keyboard(conn).map_err(|_| unsafe_state)?;
    Ok(keys)
}
pub fn paste_probe() -> CapabilityState {
    let result = connection().and_then(|(conn, _)| paste_keys(&conn));
    match result {
        Ok(_) => CapabilityState::available("x11-xtest", false),
        Err(reason) => CapabilityState::unsupported("x11-xtest", reason),
    }
    .with_probe("xtest_xkb_idle_group_one_and_key_types")
}
pub fn paste() -> PlatformResult<()> {
    let (conn, _) =
        connection().map_err(|reason| PlatformError::unsupported(Capability::AutoPaste, reason))?;
    conn.grab_server()
        .map_err(|_| PlatformError::failed(Capability::AutoPaste))?
        .check()
        .map_err(|_| PlatformError::failed(Capability::AutoPaste))?;
    let result = (|| {
        let (control, v) = paste_keys(&conn)
            .map_err(|reason| PlatformError::unsupported(Capability::AutoPaste, reason))?;
        let mut requests = Vec::new();
        let mut failed = false;
        for (kind, key) in [(2, control), (2, v), (3, v), (3, control)] {
            match conn.xtest_fake_input(kind, key, 0, 0, 0, 0, 0) {
                Ok(request) => requests.push((kind, key, request)),
                Err(_) => {
                    failed = true;
                    break;
                }
            }
        }
        // Queue both releases before the first roundtrip, including error replies.
        let mut owned = Vec::new();
        for (kind, key, request) in requests {
            match request.check() {
                Ok(()) if kind == 2 => owned.push(key),
                Ok(()) => owned.retain(|pressed| *pressed != key),
                Err(_) => failed = true,
            }
        }
        if failed {
            for key in owned.into_iter().rev() {
                if let Ok(request) = conn.xtest_fake_input(3, key, 0, 0, 0, 0, 0) {
                    let _ = request.check();
                }
            }
            let _ = conn.flush();
            return Err(PlatformError::failed(Capability::AutoPaste));
        }
        Ok(())
    })();
    let ungrab = conn
        .ungrab_server()
        .map_err(|_| PlatformError::failed(Capability::AutoPaste))?
        .check()
        .map_err(|_| PlatformError::failed(Capability::AutoPaste));
    result.and(ungrab)
}

#[cfg(test)]
mod tests {
    use super::*;
    use x11rb::protocol::{xproto::ModMask, Event};

    fn lookup_symbol(key: u8, state: u16) -> u64 {
        // An independent Xlib oracle is loaded only by this opt-in native test.
        // No test or production binary gains a mandatory libX11 dependency.
        unsafe {
            type Open = unsafe extern "C" fn(*const std::ffi::c_char) -> *mut std::ffi::c_void;
            type Close = unsafe extern "C" fn(*mut std::ffi::c_void) -> std::ffi::c_int;
            type Lookup = unsafe extern "C" fn(
                *mut std::ffi::c_void,
                u8,
                u32,
                *mut u32,
                *mut std::ffi::c_ulong,
            ) -> std::ffi::c_int;
            let library = libc::dlopen(c"libX11.so.6".as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
            assert!(!library.is_null());
            let open = libc::dlsym(library, c"XOpenDisplay".as_ptr());
            let close = libc::dlsym(library, c"XCloseDisplay".as_ptr());
            let lookup = libc::dlsym(library, c"XkbLookupKeySym".as_ptr());
            if open.is_null() || close.is_null() || lookup.is_null() {
                libc::dlclose(library);
                panic!("Native test requires the Xlib XKB lookup functions");
            }
            let open: Open = std::mem::transmute(open);
            let close: Close = std::mem::transmute(close);
            let lookup: Lookup = std::mem::transmute(lookup);
            let display = open(std::ptr::null());
            if display.is_null() {
                libc::dlclose(library);
                panic!("Native test Xlib could not connect to its private display");
            }
            let (mut modifiers, mut symbol) = (0, 0);
            let found = lookup(display, key, state.into(), &mut modifiers, &mut symbol);
            close(display);
            libc::dlclose(library);
            assert_ne!(found, 0);
            symbol as u64
        }
    }

    fn receive_keys(
        conn: &x11rb::rust_connection::RustConnection,
        count: usize,
    ) -> Vec<(u8, u8, u16)> {
        let mut received = Vec::new();
        let deadline = std::time::Instant::now()
            + std::time::Duration::from_millis(if count == 0 { 50 } else { 2000 });
        while std::time::Instant::now() < deadline && (count == 0 || received.len() < count) {
            match conn.poll_for_event().unwrap() {
                Some(Event::KeyPress(event)) => {
                    received.push((2, event.detail, event.state.into()))
                }
                Some(Event::KeyRelease(event)) => {
                    received.push((3, event.detail, event.state.into()))
                }
                _ => std::thread::sleep(std::time::Duration::from_millis(2)),
            }
        }
        received
    }

    fn assert_actual_control_v(conn: &x11rb::rust_connection::RustConnection) {
        assert!(receive_keys(conn, 0).is_empty());
        assert!(paste_probe().is_available());
        paste().unwrap();
        let received = receive_keys(conn, 4);
        assert_eq!(received.len(), 4);
        assert_eq!(
            received.iter().map(|event| event.0).collect::<Vec<_>>(),
            [2, 2, 3, 3]
        );
        for event in [received[0], received[3]] {
            assert!(matches!(lookup_symbol(event.1, event.2), 0xffe3 | 0xffe4));
        }
        for event in [received[1], received[2]] {
            assert_eq!(
                lookup_symbol(event.1, event.2),
                0x76,
                "actual shortcut must be Ctrl+v, never Ctrl+x"
            );
            assert_eq!(event.2 & 0xff, u16::from(ModMask::CONTROL));
        }
        idle_keyboard(conn).unwrap();
    }

    fn assert_unsupported_without_input(conn: &x11rb::rust_connection::RustConnection) {
        assert!(receive_keys(conn, 0).is_empty());
        let before = conn
            .xkb_get_state(xkb::ID::USE_CORE_KBD.into())
            .unwrap()
            .reply()
            .unwrap();
        let keys = conn.query_keymap().unwrap().reply().unwrap().keys;
        assert!(!paste_probe().is_available());
        assert!(paste().is_err());
        let after = conn
            .xkb_get_state(xkb::ID::USE_CORE_KBD.into())
            .unwrap()
            .reply()
            .unwrap();
        assert_eq!(
            (
                before.group,
                before.mods,
                before.base_mods,
                before.latched_mods,
                before.locked_mods
            ),
            (
                after.group,
                after.mods,
                after.base_mods,
                after.latched_mods,
                after.locked_mods
            )
        );
        assert_eq!(conn.query_keymap().unwrap().reply().unwrap().keys, keys);
        assert!(receive_keys(conn, 0).is_empty());
    }

    fn fixture_keymap(
        conn: &x11rb::rust_connection::RustConnection,
        v_in_group_one: bool,
        control_modifier: bool,
        control_selects_x: bool,
    ) {
        conn.change_keyboard_mapping(1, 24, 4, &[0x78, 0x58, 0x76, 0x56])
            .unwrap()
            .check()
            .unwrap();
        let symbols = if control_selects_x {
            [0x76, 0x78, 0x78, 0x58]
        } else if v_in_group_one {
            [0x76, 0x56, 0x78, 0x58]
        } else {
            [0x78, 0x58, 0x78, 0x58]
        };
        conn.change_keyboard_mapping(1, 55, 4, &symbols)
            .unwrap()
            .check()
            .unwrap();
        let mapping = conn.get_modifier_mapping().unwrap().reply().unwrap();
        let width = mapping.keycodes.len() / 8;
        let mut modifiers = mapping.keycodes;
        for key in &mut modifiers {
            if matches!(*key, 37 | 105) {
                *key = 0;
            }
        }
        modifiers[2 * width..3 * width].fill(0);
        let row = if control_modifier { 2 } else { 3 };
        let start = row * width;
        let slot = modifiers[start..start + width]
            .iter()
            .position(|key| *key == 0)
            .unwrap_or(0);
        modifiers[start + slot] = 37;
        assert_eq!(
            conn.set_modifier_mapping(&modifiers)
                .unwrap()
                .reply()
                .unwrap()
                .status,
            x11rb::protocol::xproto::MappingStatus::SUCCESS
        );
        if control_selects_x {
            let map = conn
                .xkb_get_map(
                    xkb::ID::USE_CORE_KBD.into(),
                    xkb::MapPart::KEY_TYPES,
                    xkb::MapPart::KEY_SYMS,
                    0,
                    0,
                    55,
                    1,
                    0,
                    0,
                    0,
                    0,
                    xkb::VMod::default(),
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                )
                .unwrap()
                .reply()
                .unwrap();
            let kind = map.map.syms_rtrn.as_ref().unwrap()[0].kt_index[0];
            let values = xkb::SetMapAux {
                types: Some(vec![xkb::SetKeyType {
                    mask: ModMask::CONTROL,
                    real_mods: ModMask::CONTROL,
                    virtual_mods: xkb::VMod::default(),
                    num_levels: 2,
                    preserve: false,
                    entries: vec![xkb::KTSetMapEntry {
                        level: 1,
                        real_mods: ModMask::CONTROL,
                        virtual_mods: xkb::VMod::default(),
                    }],
                    preserve_entries: vec![],
                }]),
                ..Default::default()
            };
            conn.xkb_set_map(
                xkb::ID::USE_CORE_KBD.into(),
                xkb::SetMapFlags::default(),
                map.min_key_code,
                map.max_key_code,
                kind,
                1,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                xkb::VMod::default(),
                &values,
            )
            .unwrap()
            .check()
            .unwrap();
        }
    }

    #[test]
    fn isolated_x11_ports_probe_source_and_explicit_paste() {
        if std::env::var("DEZIRCLIP_RUN_LINUX_CLIPBOARD_SMOKE_TESTS").as_deref() != Ok("1") {
            return;
        }
        use x11rb::{
            protocol::xproto::{CreateWindowAux, EventMask, InputFocus, PropMode, WindowClass},
            wrapper::ConnectionExt as _,
        };
        let (conn, screen) = connection().unwrap();
        assert!(
            conn.xkb_use_extension(1, 0)
                .unwrap()
                .reply()
                .unwrap()
                .supported
        );
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
        let controls = conn
            .xkb_get_controls(xkb::ID::USE_CORE_KBD.into())
            .unwrap()
            .reply()
            .unwrap();
        eprintln!(
            "native XKB controls={:#x}, internal={:#x}, timeout={}",
            u32::from(controls.enabled_controls),
            u16::from(controls.internal_mods_mask),
            controls.access_x_timeout
        );
        assert!(paste_probe().is_available());
        let identity = source().unwrap();
        assert_eq!(identity.resolution, SourceResolution::ActiveWindow);
        assert!(identity.process_path.is_some());
        assert_actual_control_v(&conn);
        eprintln!("verified default keymap: actual Control+v and no held keys after delivery");

        // Lower keycode 24 has [x,X,v,V]. Only 55 is v in the active group.
        fixture_keymap(&conn, true, true, false);
        assert_eq!(lookup_symbol(24, u16::from(ModMask::CONTROL)), 0x78);
        assert_eq!(
            lookup_symbol(24, u16::from(ModMask::CONTROL) | (1 << 13)),
            0x76
        );
        assert_eq!(lookup_symbol(55, u16::from(ModMask::CONTROL)), 0x76);
        assert_actual_control_v(&conn);
        eprintln!("verified remapped [x,X,v,V]: independent Xlib oracle observed Control+v, not Control+x");

        conn.xkb_latch_lock_state(
            xkb::ID::USE_CORE_KBD.into(),
            ModMask::default(),
            ModMask::default(),
            true,
            xkb::Group::M2,
            ModMask::default(),
            false,
            0,
        )
        .unwrap()
        .check()
        .unwrap();
        assert_eq!(
            conn.xkb_get_state(xkb::ID::USE_CORE_KBD.into())
                .unwrap()
                .reply()
                .unwrap()
                .group,
            xkb::Group::M2
        );
        assert_unsupported_without_input(&conn);
        eprintln!(
            "verified active group two: Unsupported without changing group or injecting keys"
        );
        conn.xkb_latch_lock_state(
            xkb::ID::USE_CORE_KBD.into(),
            ModMask::default(),
            ModMask::default(),
            true,
            xkb::Group::M1,
            ModMask::default(),
            false,
            0,
        )
        .unwrap()
        .check()
        .unwrap();

        for held in [50, 55] {
            conn.xtest_fake_input(2, held, 0, 0, 0, 0, 0)
                .unwrap()
                .check()
                .unwrap();
            assert_eq!(receive_keys(&conn, 1)[0].1, held);
            assert_unsupported_without_input(&conn);
            eprintln!(
                "verified held fixture key {held}: Unsupported without releasing the held key"
            );
            conn.xtest_fake_input(3, held, 0, 0, 0, 0, 0)
                .unwrap()
                .check()
                .unwrap();
            assert_eq!(receive_keys(&conn, 1)[0].1, held);
        }

        fixture_keymap(&conn, false, true, false);
        assert_unsupported_without_input(&conn);
        eprintln!("verified no v in current group: Unsupported without input");
        fixture_keymap(&conn, true, false, false);
        assert_unsupported_without_input(&conn);
        eprintln!("verified Control key remapped to Mod1: Unsupported without input");
        fixture_keymap(&conn, true, true, true);
        assert_eq!(lookup_symbol(55, u16::from(ModMask::CONTROL)), 0x78);
        assert_unsupported_without_input(&conn);
        eprintln!("verified Control-dependent key type actually translates to x: Unsupported without input");

        conn.delete_property(root, active).unwrap().check().unwrap();
        conn.destroy_window(window).unwrap().check().unwrap();
    }
}
