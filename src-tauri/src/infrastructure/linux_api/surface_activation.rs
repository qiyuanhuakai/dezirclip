//! Request-bound application activation. The WM may refuse every request.
use std::io::{self, IoSlice};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::time::Instant;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{Atom, AtomEnum, ClientMessageEvent, ConnectionExt, CreateWindowAux, EventMask, MapState, PropMode, Property, Window, WindowClass};
use x11rb::protocol::Event;
use x11rb::reexports::x11rb_protocol::{parse_display::{parse_display, ConnectAddress, ParsedDisplay}, xauth::get_auth};
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

fn loopback_ip(address: IpAddr) -> bool {
    address.is_loopback() || match address {
        IpAddr::V6(value) => value.to_ipv4_mapped().is_some_and(|value| value.is_loopback()),
        IpAddr::V4(_) => false,
    }
}

fn local_tcp_addresses(host: &str, port: u16) -> io::Result<Vec<SocketAddr>> {
    if port < 6000 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "X TCP port rejected"));
    }
    // Do not invoke an unbounded resolver on this request path. Only the local
    // canonical name and explicit loopback addresses are supported over TCP.
    if host.eq_ignore_ascii_case("localhost") || host.eq_ignore_ascii_case("localhost.") {
        return Ok(vec![SocketAddr::new(Ipv6Addr::LOCALHOST.into(), port),
                       SocketAddr::new(Ipv4Addr::LOCALHOST.into(), port)]);
    }
    let numeric = host.strip_prefix('[').and_then(|value| value.strip_suffix(']')).unwrap_or(host);
    let address: IpAddr = numeric.parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Only local X TCP addresses are supported"))?;
    if !loopback_ip(address) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Nonlocal X TCP address rejected"));
    }
    Ok(vec![SocketAddr::new(address, port)])
}

fn validate_tcp_display_port(display: &ParsedDisplay) -> io::Result<()> {
    // x11rb adds 6000 when producing TCP instructions. Reject overflow before
    // asking for them; an explicit Unix-only display does not use a TCP port.
    if display.protocol.as_deref() != Some("unix") && display.host != "unix"
        && display.display.checked_add(6000).is_none() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "X display TCP port overflow"));
    }
    Ok(())
}

fn local_tcp_socket(address: SocketAddr, deadline: Instant, current: &dyn Fn() -> bool) -> io::Result<TcpStream> {
    active(deadline, current)?;
    if address.port() < 6000 || !loopback_ip(address.ip()) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Local X TCP address rejected"));
    }
    let family = if address.is_ipv4() { libc::AF_INET } else { libc::AF_INET6 };
    // Keep the same nonblocking connect/poll/ownership rules as the Unix path.
    let raw = unsafe { libc::socket(family, libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK, 0) };
    if raw < 0 { return Err(io::Error::last_os_error()); }
    let descriptor = unsafe { OwnedFd::from_raw_fd(raw) };
    active(deadline, current)?;
    let result = match address {
        SocketAddr::V4(value) => {
            let address = libc::sockaddr_in { sin_family: libc::AF_INET as libc::sa_family_t,
                sin_port: value.port().to_be(), sin_addr: libc::in_addr { s_addr: u32::from_ne_bytes(value.ip().octets()) },
                sin_zero: [0; 8] };
            unsafe { libc::connect(descriptor.as_raw_fd(), (&address as *const libc::sockaddr_in).cast(),
                std::mem::size_of_val(&address) as libc::socklen_t) }
        }
        SocketAddr::V6(value) => {
            let address = libc::sockaddr_in6 { sin6_family: libc::AF_INET6 as libc::sa_family_t,
                sin6_port: value.port().to_be(), sin6_flowinfo: value.flowinfo().to_be(),
                sin6_addr: libc::in6_addr { s6_addr: value.ip().octets() }, sin6_scope_id: value.scope_id() };
            unsafe { libc::connect(descriptor.as_raw_fd(), (&address as *const libc::sockaddr_in6).cast(),
                std::mem::size_of_val(&address) as libc::socklen_t) }
        }
    };
    if result != 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EINPROGRESS) { return Err(error); }
        poll(descriptor.as_raw_fd(), PollMode::Writable, deadline, current)?;
        let mut value: libc::c_int = 0;
        let mut size = std::mem::size_of_val(&value) as libc::socklen_t;
        if unsafe { libc::getsockopt(descriptor.as_raw_fd(), libc::SOL_SOCKET, libc::SO_ERROR,
            (&mut value as *mut libc::c_int).cast(), &mut size) } != 0 {
            return Err(io::Error::last_os_error());
        }
        if value != 0 { return Err(io::Error::from_raw_os_error(value)); }
    }
    active(deadline, current)?;
    Ok(TcpStream::from(descriptor))
}

fn connection(deadline: Instant, current: Current) -> Result<(XConnection, usize), String> {
    active(deadline, current.as_ref()).map_err(|_| "SurfaceActivationUnavailable")?;
    let display = parse_display(None).map_err(|_| "SurfaceActivationUnavailable")?;
    validate_tcp_display_port(&display).map_err(|_| "SurfaceActivationUnavailable")?;
    let screen = usize::from(display.screen);
    for address in display.connect_instruction() {
        let connected = match address {
            ConnectAddress::Socket(path) => [true, false].into_iter()
                .find_map(|abstract_socket| local_socket(&path, abstract_socket, deadline, current.as_ref()).ok())
                .ok_or_else(|| io::Error::new(io::ErrorKind::ConnectionRefused, "Local X socket unavailable"))
                .and_then(DefaultStream::from_unix_stream),
            ConnectAddress::Hostname(host, port) => local_tcp_addresses(host, port).and_then(|addresses| {
                addresses.into_iter().find_map(|address| local_tcp_socket(address, deadline, current.as_ref()).ok())
                    .ok_or_else(|| io::Error::new(io::ErrorKind::ConnectionRefused, "Local X TCP socket unavailable"))
                    .and_then(DefaultStream::from_tcp_stream)
            }),
            _ => continue,
        };
        if let Ok((stream, (family, address))) = connected {
            active(deadline, current.as_ref()).map_err(|_| "SurfaceActivationUnavailable")?;
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
            .map_err(|_| "SurfaceActivationUnavailable")?
            .check().map_err(|_| "SurfaceActivationUnavailable")?;
        self.connection.flush().map_err(|_| "SurfaceActivationUnavailable")?;
        Ok(())
    }
}

fn restore_write(
    deadline: Instant,
    current: &dyn Fn() -> bool,
    context: &mut impl FnMut() -> Result<(), String>,
    now: impl Fn() -> Instant,
    write: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    if !current() { return Err("SurfaceDisplayCancelled".into()); }
    if now() >= deadline { return Err("SurfaceFocusRestoreFailed".into()); }
    context()?;
    if !current() { return Err("SurfaceDisplayCancelled".into()); }
    if now() >= deadline { return Err("SurfaceFocusRestoreFailed".into()); }
    write()
}

/// Original-target restoration keeps the existing action order. It is not a
/// helper-show activation policy; each write is guarded on this bounded stream.
pub(crate) fn restore_once(window: Window, deadline: Instant, current: Current,
    mut context: impl FnMut() -> Result<(), String>) -> Result<(), String> {
    active(deadline, current.as_ref()).map_err(|_| "SurfaceFocusRestoreFailed")?;
    let (connection, screen) = connection(deadline, current.clone()).map_err(|_| "SurfaceFocusRestoreFailed")?;
    let root = connection.setup().roots[screen].root;
    let active_atom = atom(&connection, b"_NET_ACTIVE_WINDOW").map_err(|_| "SurfaceFocusRestoreFailed")?;
    restore_write(deadline, current.as_ref(), &mut context, Instant::now, || {
        let event = ClientMessageEvent::new(32, window, active_atom, [2, x11rb::CURRENT_TIME, window, 0, 0]);
        connection.send_event(false, root, EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY, event)
            .map_err(|_| "SurfaceFocusRestoreFailed")?.check().map_err(|_| "SurfaceFocusRestoreFailed")?;
        Ok(())
    })?;
    restore_write(deadline, current.as_ref(), &mut context, Instant::now, || {
        connection.configure_window(window, &x11rb::protocol::xproto::ConfigureWindowAux::new()
            .stack_mode(x11rb::protocol::xproto::StackMode::ABOVE))
            .map_err(|_| "SurfaceFocusRestoreFailed")?.check().map_err(|_| "SurfaceFocusRestoreFailed")?;
        Ok(())
    })?;
    restore_write(deadline, current.as_ref(), &mut context, Instant::now, || {
        connection.set_input_focus(x11rb::protocol::xproto::InputFocus::POINTER_ROOT, window, x11rb::CURRENT_TIME)
            .map_err(|_| "SurfaceFocusRestoreFailed")?.check().map_err(|_| "SurfaceFocusRestoreFailed")?;
        Ok(())
    })?;
    connection.flush().map_err(|_| "SurfaceFocusRestoreFailed")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_tcp_display_addresses_do_not_require_dns() {
        for name in ["localhost", "LOCALHOST", "localhost."] {
            assert_eq!(local_tcp_addresses(name, 6000).unwrap(), vec![
                SocketAddr::new(Ipv6Addr::LOCALHOST.into(), 6000),
                SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 6000),
            ]);
        }
        for display in ["127.0.0.1:0", "127.8.2.4:12", "::1:0", "[::1]:1", "[::ffff:127.0.0.1]:0"] {
            let parsed = parse_display(Some(display)).unwrap();
            validate_tcp_display_port(&parsed).unwrap();
            let ConnectAddress::Hostname(host, port) = parsed.connect_instruction().next().unwrap() else { panic!("TCP display expected") };
            let addresses = local_tcp_addresses(host, port).unwrap();
            assert_eq!(addresses.len(), 1);
            assert!(loopback_ip(addresses[0].ip()));
            assert_eq!(addresses[0].port(), 6000 + parsed.display);
        }
    }
    #[test]
    fn tcp_display_rejects_nonlocal_names_addresses_and_port_wraparound() {
        for host in ["localhost.evil", "localhost..", "localhost.localdomain", "server", "localhost:6000", "127.0.0.1.evil",
            "0.0.0.0", "192.168.1.2", "::", "::127.0.0.1", "[::127.0.0.1]", "[2001:db8::1]", "[::ffff:192.168.1.2]", "[::1", "::1]", "::1%lo"] {
            assert!(local_tcp_addresses(host, 6000).is_err(), "unexpected endpoint: {host}");
        }
        for port in [0, 1, 5999] { assert!(local_tcp_addresses("localhost", port).is_err()); }
        assert_eq!(local_tcp_addresses("127.0.0.1", u16::MAX).unwrap()[0].port(), u16::MAX);
        validate_tcp_display_port(&parse_display(Some("localhost:59535")).unwrap()).unwrap();
        assert!(validate_tcp_display_port(&parse_display(Some("localhost:59536")).unwrap()).is_err());
        assert!(validate_tcp_display_port(&parse_display(Some("[::1]:65535")).unwrap()).is_err());
        validate_tcp_display_port(&ParsedDisplay { host: "unix".into(), protocol: None, display: u16::MAX, screen: 0 }).unwrap();
        validate_tcp_display_port(&parse_display(Some("unix/host:65535")).unwrap()).unwrap();
    }
    #[test]
    fn expired_or_cancelled_tcp_request_never_starts_a_socket() {
        let address = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 6000);
        assert!(local_tcp_socket(address, Instant::now(), &|| panic!("expired request must not inspect context")).is_err());
        assert!(local_tcp_socket(address, Instant::now() + std::time::Duration::from_secs(1), &|| false).is_err());
        assert!(local_tcp_socket("192.168.1.2:6000".parse().unwrap(),
            Instant::now() + std::time::Duration::from_secs(1), &|| true).is_err());
        assert!(local_tcp_socket("127.0.0.1:5999".parse().unwrap(),
            Instant::now() + std::time::Duration::from_secs(1), &|| true).is_err());
    }
    #[test]
    fn restoration_context_that_finishes_at_deadline_cannot_write() {
        let start = Instant::now();
        let deadline = start + std::time::Duration::from_secs(5);
        let now = std::cell::Cell::new(start);
        let contexts = std::cell::Cell::new(0);
        let writes = std::cell::Cell::new(0);
        assert_eq!(restore_write(deadline, &|| true, &mut || {
            contexts.set(contexts.get() + 1); now.set(deadline); Ok(())
        }, || now.get(), || { writes.set(writes.get() + 1); Ok(()) }).unwrap_err(), "SurfaceFocusRestoreFailed");
        assert_eq!(contexts.get(), 1);
        assert_eq!(writes.get(), 0);
    }
    #[test]
    fn restoration_cancel_or_target_change_during_context_cannot_write() {
        let now = Instant::now();
        let current = std::cell::Cell::new(true);
        let writes = std::cell::Cell::new(0);
        assert_eq!(restore_write(now + std::time::Duration::from_secs(5), &|| current.get(), &mut || {
            current.set(false); Ok(())
        }, || now, || { writes.set(writes.get() + 1); Ok(()) }).unwrap_err(), "SurfaceDisplayCancelled");
        assert_eq!(restore_write(now + std::time::Duration::from_secs(5), &|| true,
            &mut || Err("SurfaceFocusChanged".into()), || now,
            || { writes.set(writes.get() + 1); Ok(()) }).unwrap_err(), "SurfaceFocusChanged");
        assert_eq!(writes.get(), 0);
    }
    #[test]
    fn each_restoration_action_rechecks_its_context() {
        let now = Instant::now();
        let mut contexts = 0;
        let mut writes = 0;
        for action in 0..3 {
            let result = restore_write(now + std::time::Duration::from_secs(5), &|| true, &mut || {
                contexts += 1;
                if action == 2 { Err("SurfaceTargetChanged".into()) } else { Ok(()) }
            }, || now, || { writes += 1; Ok(()) });
            assert_eq!(result.is_ok(), action < 2);
        }
        assert_eq!((contexts, writes), (3, 2));
    }
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
