//! Focus can be returned to the original application only while the requesting
//! desktop client, or the original target itself, still owns keyboard focus.
use super::*;
use crate::ipc::{PeerIdentity, ProcessExit};
use dzc_protocol::wire::QueueTarget;

fn failed() -> PlatformError {
    PlatformError::failed(Capability::AutoPaste)
}

pub struct RememberedFocus {
    peer_pid: u32,
    peer: ProcessExit,
    target: QueueTarget,
    #[cfg(target_os = "windows")]
    original: WindowsBoundPaste,
    #[cfg(target_os = "linux")]
    original: linux::Original,
}

impl RememberedFocus {
    pub fn capture(peer: &PeerIdentity) -> PlatformResult<Self> {
        let peer_pid = peer.pid().ok_or_else(failed)?;
        let peer = ProcessExit::open(peer).map_err(|_| failed())?;
        let target = QueueInputTarget::foreground()?;
        #[cfg(target_os = "windows")]
        let original = {
            let descriptor = target.windows.as_ref().ok_or_else(failed)?;
            if descriptor.pid == peer_pid || peer.shares_executable(descriptor.pid).map_err(|_| failed())? {
                return Err(failed());
            }
            WindowsBoundPaste::prepare(descriptor)?
        };
        #[cfg(target_os = "linux")]
        let original = linux::Original::capture(target.x11.ok_or_else(failed)?, peer_pid, &peer)?;
        Ok(Self {
            peer_pid,
            peer,
            original,
            target,
        })
    }

    pub fn restore(&self) -> PlatformResult<&QueueTarget> {
        if !self.peer.is_running().map_err(|_| failed())? {
            return Err(failed());
        }
        self.original.restore_from(self.peer_pid)?;
        Ok(&self.target)
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use x11rb::{
        connection::Connection,
        protocol::{
            res::{ClientIdMask, ClientIdSpec, ConnectionExt as _},
            xproto::{
                AtomEnum, ChangeWindowAttributesAux, ClientMessageEvent, ConnectionExt, EventMask,
                InputFocus, MapState,
            },
            Event,
        },
        rust_connection::RustConnection,
    };

    pub(super) struct Original {
        conn: RustConnection,
        root: u32,
        client: u32,
        focus: u32,
        pid: u32,
    }

    fn owner(conn: &RustConnection, window: u32) -> PlatformResult<u32> {
        // XRes identifies the local client that allocated the XID. Window
        // properties such as _NET_WM_PID are writable by other X11 clients.
        let replies = conn
            .res_query_client_ids(&[ClientIdSpec {
                client: window,
                mask: ClientIdMask::LOCAL_CLIENT_PID,
            }])
            .map_err(|_| failed())?
            .reply()
            .map_err(|_| failed())?;
        replies
            .ids
            .iter()
            .find_map(|value| match value.value.as_slice() {
                [pid] if value.spec.mask == ClientIdMask::LOCAL_CLIENT_PID && *pid > 0 => {
                    Some(*pid)
                }
                _ => None,
            })
            .ok_or_else(failed)
    }

    impl Original {
        pub(super) fn capture(focus: u32, peer: u32, identity: &ProcessExit) -> PlatformResult<Self> {
            let (conn, _) = super::super::x11::connection().map_err(|_| failed())?;
            let pid = owner(&conn, focus)?;
            if pid == peer || pid == std::process::id() || identity.shares_executable(pid).map_err(|_| failed())? {
                return Err(failed());
            }
            let mut client = focus;
            let mut root = 0;
            for _ in 0..64 {
                let tree = conn
                    .query_tree(client)
                    .map_err(|_| failed())?
                    .reply()
                    .map_err(|_| failed())?;
                root = tree.root;
                if tree.parent == root || owner(&conn, tree.parent).ok() != Some(pid) {
                    break;
                }
                client = tree.parent;
            }
            if root == 0 || client == root {
                return Err(failed());
            }
            for window in [client, focus] {
                conn.change_window_attributes(
                    window,
                    &ChangeWindowAttributesAux::new().event_mask(EventMask::STRUCTURE_NOTIFY),
                )
                .map_err(|_| failed())?
                .check()
                .map_err(|_| failed())?;
            }
            if super::super::x11::checked_focus(&conn)? != focus || owner(&conn, focus)? != pid {
                return Err(failed());
            }
            Ok(Self {
                conn,
                root,
                client,
                focus,
                pid,
            })
        }

        pub(super) fn restore_from(&self, peer: u32) -> PlatformResult<()> {
            let conn = &self.conn;
            conn.grab_server()
                .map_err(|_| failed())?
                .check()
                .map_err(|_| failed())?;
            let mut wait_for_manager = None;
            let result = (|| {
                let focused = super::super::x11::checked_focus(conn)?;
                while let Some(event) = conn.poll_for_event().map_err(|_| failed())? {
                    if matches!(event, Event::DestroyNotify(e) if e.window == self.client || e.window == self.focus)
                    {
                        return Err(failed());
                    }
                }
                if owner(conn, self.focus)? != self.pid || owner(conn, self.client)? != self.pid {
                    return Err(failed());
                }
                if focused == self.focus {
                    return Ok(());
                }
                if owner(conn, focused)? != peer {
                    return Err(failed());
                }
                let attrs = conn
                    .get_window_attributes(self.focus)
                    .map_err(|_| failed())?
                    .reply()
                    .map_err(|_| failed())?;
                if attrs.map_state != MapState::VIEWABLE {
                    return Err(failed());
                }
                let atom = conn
                    .intern_atom(true, b"_NET_ACTIVE_WINDOW")
                    .map_err(|_| failed())?
                    .reply()
                    .map_err(|_| failed())?
                    .atom;
                if atom != 0 {
                    let active = conn
                        .get_property(false, self.root, atom, AtomEnum::WINDOW, 0, 1)
                        .map_err(|_| failed())?
                        .reply()
                        .map_err(|_| failed())?;
                    if let Some(window) = active
                        .value32()
                        .and_then(|mut ids| ids.next())
                        .filter(|id| *id != 0)
                    {
                        if window != self.client && owner(conn, window)? != peer {
                            return Err(failed());
                        }
                        if window != self.client {
                            wait_for_manager = Some(atom);
                        }
                    }
                    let message = ClientMessageEvent::new(
                        32,
                        self.client,
                        atom,
                        [1, x11rb::CURRENT_TIME, focused, 0, 0],
                    );
                    conn.send_event(
                        false,
                        self.root,
                        EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                        message,
                    )
                    .map_err(|_| failed())?
                    .check()
                    .map_err(|_| failed())?;
                }
                if wait_for_manager.is_some() {
                    return Ok(());
                }
                conn.set_input_focus(InputFocus::PARENT, self.focus, x11rb::CURRENT_TIME)
                    .map_err(|_| failed())?
                    .check()
                    .map_err(|_| failed())?;
                if super::super::x11::checked_focus(conn)? != self.focus {
                    return Err(failed());
                }
                Ok(())
            })();
            conn.ungrab_server()
                .map_err(|_| failed())?
                .check()
                .map_err(|_| failed())?;
            result?;
            if let Some(atom) = wait_for_manager {
                self.settle_focus(atom, peer)?;
            }
            Ok(())
        }

        // A reparenting window manager may focus the application's toplevel
        // after processing _NET_ACTIVE_WINDOW. Restore the exact child only
        // when that toplevel is active and currently owns the keyboard.
        fn settle_focus(&self, atom: u32, peer: u32) -> PlatformResult<()> {
            let conn = &self.conn;
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(250);
            loop {
                conn.grab_server().map_err(|_| failed())?.check().map_err(|_| failed())?;
                let result = (|| {
                    let active = conn.get_property(false, self.root, atom, AtomEnum::WINDOW, 0, 1)
                        .map_err(|_| failed())?.reply().map_err(|_| failed())?
                        .value32().and_then(|mut ids| ids.next()).ok_or_else(failed)?;
                    let focus = super::super::x11::checked_focus(conn)?;
                    if active != self.client {
                        if owner(conn, active)? != peer || (focus != self.focus && focus != self.client && owner(conn, focus)? != peer) {
                            return Err(failed());
                        }
                        return Ok(false);
                    }
                    if focus == self.focus { return Ok(true); }
                    if owner(conn, focus)? == peer { return Ok(false); }
                    if focus != self.client || owner(conn, focus)? != self.pid || owner(conn, self.focus)? != self.pid {
                        return Err(failed());
                    }
                    conn.set_input_focus(InputFocus::PARENT, self.focus, x11rb::CURRENT_TIME)
                        .map_err(|_| failed())?.check().map_err(|_| failed())?;
                    Ok(super::super::x11::checked_focus(conn)? == self.focus)
                })();
                conn.ungrab_server().map_err(|_| failed())?.check().map_err(|_| failed())?;
                if result? { return Ok(()); }
                if std::time::Instant::now() >= deadline { return Err(failed()); }
                std::thread::sleep(std::time::Duration::from_millis(4));
            }
        }
    }
}
