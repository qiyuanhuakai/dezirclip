//! Request-bound application activation. The WM may refuse every request.
use std::io::{self, IoSlice};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::time::Instant;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{Atom, AtomEnum, ClientMessageEvent, ConnectionExt, CreateWindowAux, EventMask, MapState, PropMode, Property, Window, WindowClass};
use x11rb::protocol::Event;
use x11rb::reexports::x11rb_protocol::{parse_display::{parse_display, ConnectAddress}, xauth::get_auth};
use x11rb::rust_connection::{DefaultStream, PollMode, RustConnection, Stream};
use x11rb::utils::RawFdContainer;
use x11rb::wrapper::ConnectionExt as _;

type Current = Arc<dyn Fn() -> bool + Send + Sync>;
type XConnection = RustConnection<DeadlineStream>;
const MAX_CLIENTS: u32 = 256;

fn active(deadline: Instant, current: &dyn Fn() -> bool) -> io::Result<()> {
    if Instant::now() >= deadline || !current() || Instant::now() >= deadline {
        Err(io::Error::new(io::ErrorKind::TimedOut, "Surface request expired"))
    } else { Ok(()) }
}

fn poll(fd: i32, mode: PollMode, deadline: Instant, current: &dyn Fn() -> bool) -> io::Result<()> {
    loop {
        active(deadline, current)?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        let milliseconds = remaining.as_millis().saturating_add(1).min(i32::MAX as u128) as i32;
        let mut descriptor = libc::pollfd { fd, events: 0, revents: 0 };
        if mode.readable() { descriptor.events |= libc::POLLIN; }
        if mode.writable() { descriptor.events |= libc::POLLOUT; }
        // The descriptor belongs to this request's nonblocking local X connection.
        let result = unsafe { libc::poll(&mut descriptor, 1, milliseconds) };
        if result < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted { continue; }
            return Err(error);
        }
        active(deadline, current)?;
        if descriptor.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "X connection closed"));
        }
        if result > 0 { return Ok(()); }
    }
}

struct DeadlineStream { inner: DefaultStream, deadline: Instant, current: Current }
impl Stream for DeadlineStream {
    fn poll(&self, mode: PollMode) -> io::Result<()> {
        poll(self.inner.as_raw_fd(), mode, self.deadline, self.current.as_ref())
    }
    fn read(&self, buffer: &mut [u8], fds: &mut Vec<RawFdContainer>) -> io::Result<usize> {
        active(self.deadline, self.current.as_ref())?;
        self.inner.read(buffer, fds)
    }
    fn write(&self, buffer: &[u8], fds: &mut Vec<RawFdContainer>) -> io::Result<usize> {
        active(self.deadline, self.current.as_ref())?;
        self.inner.write(buffer, fds)
    }
    fn write_vectored(&self, buffers: &[IoSlice<'_>], fds: &mut Vec<RawFdContainer>) -> io::Result<usize> {
        active(self.deadline, self.current.as_ref())?;
        self.inner.write_vectored(buffers, fds)
    }
}

fn local_socket(path: &str, abstract_socket: bool, deadline: Instant, current: &dyn Fn() -> bool) -> io::Result<UnixStream> {
    active(deadline, current)?;
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    let bytes = path.as_bytes();
    if bytes.is_empty() || bytes.len() + 1 > address.sun_path.len() || bytes.contains(&0) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Local X socket path rejected"));
    }
    let start = usize::from(abstract_socket);
    for (output, input) in address.sun_path[start..].iter_mut().zip(bytes) { *output = *input as libc::c_char; }
    let length = std::mem::offset_of!(libc::sockaddr_un, sun_path) + bytes.len() + 1;
    // Own the socket immediately, including all failed or timed-out connect paths.
    let raw = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK, 0) };
    if raw < 0 { return Err(io::Error::last_os_error()); }
    let descriptor = unsafe { OwnedFd::from_raw_fd(raw) };
    let result = unsafe { libc::connect(descriptor.as_raw_fd(), (&address as *const libc::sockaddr_un).cast(), length as libc::socklen_t) };
    if result != 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EINPROGRESS) { return Err(error); }
        poll(descriptor.as_raw_fd(), PollMode::Writable, deadline, current)?;
        let mut value: libc::c_int = 0;
        let mut size = std::mem::size_of_val(&value) as libc::socklen_t;
        if unsafe { libc::getsockopt(descriptor.as_raw_fd(), libc::SOL_SOCKET, libc::SO_ERROR, (&mut value as *mut libc::c_int).cast(), &mut size) } != 0 {
            return Err(io::Error::last_os_error());
        }
        if value != 0 { return Err(io::Error::from_raw_os_error(value)); }
    }
    active(deadline, current)?;
    Ok(UnixStream::from(descriptor))
}

fn connection(deadline: Instant, current: Current) -> Result<(XConnection, usize), String> {
    active(deadline, current.as_ref()).map_err(|_| "SurfaceActivationUnavailable")?;
    let display = parse_display(None).map_err(|_| "SurfaceActivationUnavailable")?;
    let screen = usize::from(display.screen);
    for address in display.connect_instruction() {
        let ConnectAddress::Socket(path) = address else { continue; };
        for abstract_socket in [true, false] {
            let socket = match local_socket(&path, abstract_socket, deadline, current.as_ref()) {
                Ok(socket) => socket,
                Err(_) => continue,
            };
            let (stream, (family, address)) = DefaultStream::from_unix_stream(socket).map_err(|_| "SurfaceActivationUnavailable")?;
            // Authentication stays the normal parent's Xauthority lookup. It runs
            // in a bounded caller worker; an OS-blocked file read is not a killable thread.
            let (name, data) = get_auth(family, &address, display.display).map_err(|_| "SurfaceActivationUnavailable")?.unwrap_or_default();
            active(deadline, current.as_ref()).map_err(|_| "SurfaceActivationUnavailable")?;
            let stream = DeadlineStream { inner: stream, deadline, current: current.clone() };
            let connection = RustConnection::connect_to_stream_with_auth_info(stream, screen, name, data).map_err(|_| "SurfaceActivationUnavailable")?;
            return Ok((connection, screen));
        }
    }
    Err("SurfaceActivationUnavailable".into())
}

fn atom(connection: &XConnection, name: &[u8]) -> Result<Atom, String> {
    let value = connection.intern_atom(false, name).map_err(|_| "SurfaceActivationUnavailable")?.reply().map_err(|_| "SurfaceActivationUnavailable")?.atom;
    if value == 0 { return Err("SurfaceActivationUnavailable".into()); }
    Ok(value)
}

fn request_event(event: &x11rb::protocol::xproto::PropertyNotifyEvent, window: Window, property: Atom) -> Option<u32> {
    (event.window == window && event.atom == property && event.state == Property::NEW_VALUE
        && event.response_type & 0x80 == 0 && event.time != 0).then_some(event.time)
}

/// Server-clock sample at explicit request start, not an invented key-event time.
pub(crate) fn request_time(deadline: Instant, current: Current) -> Result<u32, String> {
    let (connection, screen) = connection(deadline, current.clone())?;
    let root = connection.setup().roots[screen].root;
    let window = connection.generate_id().map_err(|_| "SurfaceActivationUnavailable")?;
    connection.create_window(0, window, root, 0, 0, 1, 1, 0, WindowClass::INPUT_ONLY, 0,
        &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE)).map_err(|_| "SurfaceActivationUnavailable")?
        .check().map_err(|_| "SurfaceActivationUnavailable")?;
    let property = atom(&connection, b"_DZC_SURFACE_REQUEST_TIME")?;
    connection.change_property8(PropMode::REPLACE, window, property, AtomEnum::INTEGER, &[0])
        .map_err(|_| "SurfaceActivationUnavailable")?;
    connection.flush().map_err(|_| "SurfaceActivationUnavailable")?;
    loop {
        active(deadline, current.as_ref()).map_err(|_| "SurfaceActivationUnavailable")?;
        match connection.poll_for_event().map_err(|_| "SurfaceActivationUnavailable")? {
            Some(Event::PropertyNotify(event)) if request_event(&event, window, property).is_some() => {
                    active(deadline, current.as_ref()).map_err(|_| "SurfaceActivationUnavailable")?;
                    // This dedicated connection owns only the unmapped probe. Its
                    // default Destroy close-down releases it on every return/error.
                    return Ok(event.time);
                }
            Some(_) => {}
            None => connection.stream().poll(PollMode::Readable).map_err(|_| "SurfaceActivationUnavailable")?,
        }
    }
}

fn numbers(connection: &XConnection, window: Window, property: Atom, expected_type: Atom, limit: u32) -> Result<Vec<u32>, String> {
    let reply = connection.get_property(false, window, property, expected_type, 0, limit)
        .map_err(|_| "SurfaceActivationUnavailable")?.reply().map_err(|_| "SurfaceActivationUnavailable")?;
    if reply.type_ != expected_type || reply.format != 32 || reply.bytes_after != 0 {
        return Err("SurfaceActivationUnavailable".into());
    }
    let values: Vec<_> = reply.value32().ok_or("SurfaceActivationUnavailable")?.collect();
    if values.len() > limit as usize { return Err("SurfaceActivationUnavailable".into()); }
    Ok(values)
}

pub(crate) struct Prepared {
    connection: XConnection,
    root: Window,
    window: Window,
    pid_atom: Atom,
    active_atom: Atom,
    clients_atom: Atom,
    pid: u32,
    timestamp: u32,
    deadline: Instant,
    current: Current,
}

fn unique_client(clients: impl IntoIterator<Item = Window>) -> Result<Option<Window>, String> {
    let mut candidate = None;
    for window in clients {
        if candidate.replace(window).is_some() { return Err("SurfaceActivationUnavailable".into()); }
    }
    Ok(candidate)
}

fn mapped_client(connection: &XConnection, root: Window, clients_atom: Atom, pid_atom: Atom, pid: u32) -> Result<Option<Window>, String> {
    let mut matches = Vec::new();
    for window in numbers(connection, root, clients_atom, AtomEnum::WINDOW.into(), MAX_CLIENTS)? {
        let owner = connection.get_property(false, window, pid_atom, AtomEnum::CARDINAL, 0, 1)
            .map_err(|_| "SurfaceActivationUnavailable")?.reply().map_err(|_| "SurfaceActivationUnavailable")?;
        if owner.type_ == 0 { continue; }
        if owner.type_ != u32::from(AtomEnum::CARDINAL) || owner.format != 32 || owner.bytes_after != 0 {
            return Err("SurfaceActivationUnavailable".into());
        }
        let owners: Vec<_> = owner.value32().ok_or("SurfaceActivationUnavailable")?.collect();
        if owners != [pid] { continue; }
        let attributes = connection.get_window_attributes(window).map_err(|_| "SurfaceActivationUnavailable")?
            .reply().map_err(|_| "SurfaceActivationUnavailable")?;
        if attributes.map_state == MapState::VIEWABLE { matches.push(window); }
    }
    unique_client(matches)
}

/// Resolve one exact mapped client without activating it. The owner rechecks
/// Host state after this lookup and before consuming the one-shot request.
pub(crate) fn prepare(pid: u32, timestamp: u32, deadline: Instant, current: Current) -> Result<Prepared, String> {
    crate::infrastructure::surface_focus::application_activation_data(timestamp)?;
    let (connection, screen) = connection(deadline, current.clone())?;
    let root = connection.setup().roots[screen].root;
    let clients_atom = atom(&connection, b"_NET_CLIENT_LIST")?;
    let pid_atom = atom(&connection, b"_NET_WM_PID")?;
    let active_atom = atom(&connection, b"_NET_ACTIVE_WINDOW")?;
    let window = loop {
        active(deadline, current.as_ref()).map_err(|_| "SurfaceActivationUnavailable")?;
        if let Some(window) = mapped_client(&connection, root, clients_atom, pid_atom, pid)? { break window; }
        // GTK's visible ACK can precede the WM managing the client. Discovery is
        // read-only and spends the same presentation budget; activation is once.
        std::thread::sleep(deadline.saturating_duration_since(Instant::now()).min(std::time::Duration::from_millis(20)));
    };
    if numbers(&connection, window, pid_atom, AtomEnum::CARDINAL.into(), 1)? != [pid]
        || connection.get_window_attributes(window).map_err(|_| "SurfaceActivationUnavailable")?
            .reply().map_err(|_| "SurfaceActivationUnavailable")?.map_state != MapState::VIEWABLE {
        return Err("SurfaceActivationUnavailable".into());
    }
    Ok(Prepared { connection, root, window, pid_atom, active_atom, clients_atom, pid, timestamp, deadline, current })
}

impl Prepared {
    /// Consumed once. No retries, direct focus writes or root-property writes.
    pub(crate) fn send(self, context: impl Fn() -> Result<(), String>) -> Result<(), String> {
        if mapped_client(&self.connection, self.root, self.clients_atom, self.pid_atom, self.pid)? != Some(self.window)
            || numbers(&self.connection, self.window, self.pid_atom, AtomEnum::CARDINAL.into(), 1)? != [self.pid]
            || self.connection.get_window_attributes(self.window).map_err(|_| "SurfaceActivationUnavailable")?
                .reply().map_err(|_| "SurfaceActivationUnavailable")?.map_state != MapState::VIEWABLE {
            return Err("SurfaceActivationUnavailable".into());
        }
        context()?;
        active(self.deadline, self.current.as_ref()).map_err(|_| "SurfaceDisplayCancelled")?;
        let event = ClientMessageEvent::new(32, self.window, self.active_atom,
            crate::infrastructure::surface_focus::application_activation_data(self.timestamp)?);
        self.connection.send_event(false, self.root, EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY, event)
            .map_err(|_| "SurfaceActivationUnavailable")?;
        self.connection.flush().map_err(|_| "SurfaceActivationUnavailable")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_client_can_wait_but_ambiguous_clients_are_rejected() {
        assert_eq!(unique_client([]).unwrap(), None);
        assert_eq!(unique_client([24]).unwrap(), Some(24));
        assert!(unique_client([24, 25]).is_err());
        assert!(unique_client([24, 24]).is_err());
    }
    #[test]
    fn only_the_owned_real_property_event_provides_request_time() {
        use x11rb::protocol::xproto::{PropertyNotifyEvent, PROPERTY_NOTIFY_EVENT};
        let event = PropertyNotifyEvent { response_type: PROPERTY_NOTIFY_EVENT, sequence: 0, window: 24, atom: 31, time: 123, state: Property::NEW_VALUE };
        assert_eq!(request_event(&event, 24, 31), Some(123));
        assert_eq!(request_event(&event, 25, 31), None);
        assert_eq!(request_event(&event, 24, 32), None);
        assert_eq!(request_event(&PropertyNotifyEvent { response_type: event.response_type | 0x80, ..event }, 24, 31), None);
        assert_eq!(request_event(&PropertyNotifyEvent { state: Property::DELETE, ..event }, 24, 31), None);
        assert_eq!(request_event(&PropertyNotifyEvent { time: 0, ..event }, 24, 31), None);
    }
    #[test]
    fn a_context_check_that_uses_up_the_deadline_cannot_authorize_io() {
        let deadline = Instant::now() + std::time::Duration::from_millis(2);
        assert!(active(deadline, &|| { std::thread::sleep(std::time::Duration::from_millis(3)); true }).is_err());
        assert!(active(Instant::now(), &|| panic!("expired request must not inspect context")).is_err());
        assert!(active(Instant::now() + std::time::Duration::from_secs(1), &|| false).is_err());
    }
}
