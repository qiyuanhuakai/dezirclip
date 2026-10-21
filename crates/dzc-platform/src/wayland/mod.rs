//! Native CLIPBOARD data-control. No surfaces, XWayland fallback or external tools.
pub(crate) mod io;
mod owner;
mod payload;
pub(crate) use owner::replace_owner;
pub(crate) use owner::revoke_private_owner;
pub use owner::{write_in_process, write_in_process_with_intent, WaylandOwner};
pub use payload::{representations, representations_with_intent};

use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd};
use std::os::unix::net::UnixStream;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex, OnceLock,
};
use std::time::{Duration, Instant};
use wayland_client::{
    protocol::{wl_callback, wl_registry, wl_seat},
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
};
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1 as ext_device, ext_data_control_manager_v1 as ext_manager,
    ext_data_control_offer_v1 as ext_offer, ext_data_control_source_v1 as ext_source,
};
use wayland_protocols_wlr::data_control::v1::client::{
    zwlr_data_control_device_v1 as wlr_device, zwlr_data_control_manager_v1 as wlr_manager,
    zwlr_data_control_offer_v1 as wlr_offer, zwlr_data_control_source_v1 as wlr_source,
};

const SETUP_TIMEOUT: Duration = Duration::from_secs(3);
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(3);
pub const MAX_TRANSFER_BYTES: usize = 64 * 1024 * 1024;
pub const PASSWORD_HINT_MIME: &str = "x-kde-passwordManagerHint";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WaylandError {
    ConnectionUnavailable,
    InheritedSocketUnavailable,
    DataControlUnavailable,
    SeatUnavailable,
    SeatNotFound,
    PermissionDenied,
    ProtocolRejected,
    Disconnected,
    Cancelled,
    TimedOut,
    TransferFailed,
    TransferBudgetExceeded,
    UnsupportedMime,
    EmptySelection,
    SelectionChanged,
    OwnerCancelled,
    DisabledByConfiguration,
}
impl std::fmt::Display for WaylandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Wayland{self:?}")
    }
}
impl std::error::Error for WaylandError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DataControlProtocol {
    Ext,
    Wlr,
}

#[derive(Debug, Clone, Serialize)]
pub struct WaylandStatus {
    pub experimental: bool,
    pub protocol: DataControlProtocol,
    pub advertised_version: u32,
    pub bound_version: u32,
    pub seat_global: u32,
    pub seat_name: Option<String>,
    pub seat_version: u32,
    /// Device creation and a compositor roundtrip completed without finished/error.
    /// An operation may still fail if authorization is revoked afterward.
    pub permission: &'static str,
    pub source_identity: &'static str,
    pub primary_selection: bool,
}

pub fn requested() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var_os("WAYLAND_SOCKET").is_some()
        || std::env::var("XDG_SESSION_TYPE")
            .is_ok_and(|value| value.eq_ignore_ascii_case("wayland"))
}

pub fn service_disabled(service: &str) -> bool {
    std::env::var("DEZIRCLIP_DISABLE_LINUX_SERVICES")
        .or_else(|_| std::env::var("TIEZ_DISABLE_LINUX_SERVICES"))
        .map(|value| {
            value
                .split([',', ';', ' '])
                .any(|item| item.eq_ignore_ascii_case(service) || item.eq_ignore_ascii_case("all"))
        })
        .unwrap_or(false)
}

fn transfer_deadline() -> Instant {
    Instant::now() + TRANSFER_TIMEOUT
}

fn connect() -> Result<Connection, WaylandError> {
    if std::env::var_os("WAYLAND_SOCKET").is_some() && std::env::var_os("WAYLAND_DISPLAY").is_none()
    {
        // Do not consume a one-shot descriptor during a capability probe and
        // then falsely promise that independent later adapters can reconnect.
        return Err(WaylandError::InheritedSocketUnavailable);
    }
    let display = std::env::var_os("WAYLAND_DISPLAY").unwrap_or_else(|| "wayland-0".into());
    let display = std::path::PathBuf::from(display);
    let path = if display.is_absolute() {
        display
    } else {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .map(std::path::PathBuf::from)
            .filter(|path| path.is_absolute())
            .ok_or(WaylandError::ConnectionUnavailable)?;
        runtime.join(display)
    };
    let socket = socket2::Socket::new(socket2::Domain::UNIX, socket2::Type::STREAM, None)
        .map_err(|_| WaylandError::ConnectionUnavailable)?;
    let address = socket2::SockAddr::unix(path).map_err(|_| WaylandError::ConnectionUnavailable)?;
    socket
        .connect_timeout(&address, SETUP_TIMEOUT)
        .map_err(|_| WaylandError::ConnectionUnavailable)?;
    socket
        .set_nonblocking(false)
        .map_err(|_| WaylandError::ConnectionUnavailable)?;
    let fd: std::os::fd::OwnedFd = socket.into();
    Connection::from_socket(UnixStream::from(fd)).map_err(|_| WaylandError::ConnectionUnavailable)
}

enum RawOffer {
    Ext(ext_offer::ExtDataControlOfferV1),
    Wlr(wlr_offer::ZwlrDataControlOfferV1),
}
impl RawOffer {
    fn receive(&self, mime: &str, fd: BorrowedFd<'_>) {
        match self {
            Self::Ext(value) => value.receive(mime.to_owned(), fd),
            Self::Wlr(value) => value.receive(mime.to_owned(), fd),
        }
    }
}
impl Drop for RawOffer {
    fn drop(&mut self) {
        match self {
            Self::Ext(value) => value.destroy(),
            Self::Wlr(value) => value.destroy(),
        }
    }
}

#[derive(Clone)]
pub struct SelectionSnapshot {
    offer: Arc<RawOffer>,
    connection: Connection,
    mime_types: Arc<Vec<String>>,
    sequence: u64,
}
impl SelectionSnapshot {
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn mime_types(&self) -> &[String] {
        &self.mime_types
    }
    pub fn is_owned_copy(&self) -> bool {
        self.mime_types
            .iter()
            .any(|mime| mime == payload::own_marker())
    }
    /// Capture callers reject a frame if the monitored selection changed after
    /// preflight. Each snapshot always reads its original offer, never a new one.
    pub fn is_current(&self) -> bool {
        !MONITOR_ACTIVE.load(Ordering::Acquire)
            || CURRENT_SEQUENCE.load(Ordering::Acquire) == self.sequence
    }
    pub fn receive(&self, mime: &str, limit: usize) -> Result<Vec<u8>, WaylandError> {
        if !self.mime_types.iter().any(|offered| offered == mime) {
            return Err(WaylandError::UnsupportedMime);
        }
        if limit > MAX_TRANSFER_BYTES {
            return Err(WaylandError::TransferBudgetExceeded);
        }
        let (read, write) = io::pipe()?;
        self.offer.receive(mime, write.as_fd());
        drop(write);
        self.connection
            .flush()
            .map_err(|_| WaylandError::Disconnected)?;
        io::receive(read, limit, None, transfer_deadline())
    }
    /// None means the offer had no hint MIME. Only its bounded marker is read.
    pub fn password_hint(&self) -> Result<Option<bool>, WaylandError> {
        if !self
            .mime_types
            .iter()
            .any(|value| value == PASSWORD_HINT_MIME)
        {
            return Ok(None);
        }
        let bytes = self.receive(PASSWORD_HINT_MIME, 64)?;
        Ok(Some(bytes == b"secret"))
    }
}

struct OfferRecord {
    offer: Arc<RawOffer>,
    mimes: Vec<String>,
}
enum Manager {
    Ext(ext_manager::ExtDataControlManagerV1),
    Wlr(wlr_manager::ZwlrDataControlManagerV1),
}
enum Device {
    Ext(ext_device::ExtDataControlDeviceV1),
    Wlr(wlr_device::ZwlrDataControlDeviceV1),
}
enum Source {
    Ext(ext_source::ExtDataControlSourceV1),
    Wlr(wlr_source::ZwlrDataControlSourceV1),
}
impl Source {
    fn offer(&self, mime: String) {
        match self {
            Self::Ext(value) => value.offer(mime),
            Self::Wlr(value) => value.offer(mime),
        }
    }
}
impl Drop for Source {
    fn drop(&mut self) {
        match self {
            Self::Ext(value) => value.destroy(),
            Self::Wlr(value) => value.destroy(),
        }
    }
}

#[derive(Clone)]
pub struct MimeData {
    pub mime: String,
    pub bytes: Arc<[u8]>,
}

struct State {
    globals: BTreeMap<u32, (String, u32)>,
    seats: BTreeMap<u32, (wl_seat::WlSeat, u32, Option<String>)>,
    manager_global: Option<u32>,
    seat_global: Option<u32>,
    sync: u64,
    offers: HashMap<u32, OfferRecord>,
    selection: Option<SelectionSnapshot>,
    sequence: u64,
    changed: bool,
    error: Option<WaylandError>,
    owner_cancelled: bool,
    payloads: Vec<MimeData>,
    leases: Vec<dzc_core::api::NativeReadLease>,
    cancel: Arc<UnixStream>,
}
impl State {
    fn offer(&mut self, id: u32, offer: RawOffer) {
        self.offers.insert(
            id,
            OfferRecord {
                offer: Arc::new(offer),
                mimes: Vec::new(),
            },
        );
    }
    fn mime(&mut self, id: u32, mime: String) {
        if let Some(record) = self.offers.get_mut(&id) {
            if record.mimes.len() >= 128 || mime.len() > 256 {
                self.error = Some(WaylandError::TransferBudgetExceeded);
            } else if !record.mimes.contains(&mime) {
                record.mimes.push(mime);
            }
        }
    }
    fn selection(&mut self, id: Option<u32>, connection: &Connection) {
        self.sequence = NEXT_SEQUENCE.fetch_add(1, Ordering::AcqRel);
        self.selection =
            id.and_then(|id| self.offers.remove(&id))
                .map(|record| SelectionSnapshot {
                    offer: record.offer,
                    connection: connection.clone(),
                    mime_types: Arc::new(record.mimes),
                    sequence: self.sequence,
                });
        self.changed = true;
    }
    fn send(&mut self, mime: String, fd: std::os::fd::OwnedFd) {
        if self.owner_cancelled {
            return;
        }
        if let Some(data) = self.payloads.iter().find(|data| data.mime == mime) {
            // A cancelled receiver may close its pipe. It does not revoke other offered formats.
            if matches!(
                io::send_authorized(
                    fd,
                    &data.bytes,
                    Some(&self.cancel),
                    transfer_deadline(),
                    &self.leases
                ),
                Err(WaylandError::Cancelled | WaylandError::PermissionDenied)
            ) {
                self.owner_cancelled = true;
            }
        }
    }
}
static NEXT_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static CURRENT: OnceLock<Mutex<Option<SelectionSnapshot>>> = OnceLock::new();
static CURRENT_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static MONITOR_ACTIVE: AtomicBool = AtomicBool::new(false);
static MONITOR_READY: AtomicBool = AtomicBool::new(false);

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        _: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => {
                state.globals.insert(name, (interface, version));
            }
            wl_registry::Event::GlobalRemove { name } => {
                state.globals.remove(&name);
                if state.manager_global == Some(name) || state.seat_global == Some(name) {
                    state.error = Some(WaylandError::PermissionDenied);
                }
            }
            _ => (),
        }
    }
}
impl Dispatch<wl_callback::WlCallback, u64> for State {
    fn event(
        state: &mut Self,
        _: &wl_callback::WlCallback,
        _: wl_callback::Event,
        token: &u64,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        state.sync = *token;
    }
}
impl Dispatch<wl_seat::WlSeat, u32> for State {
    fn event(
        state: &mut Self,
        _: &wl_seat::WlSeat,
        event: wl_seat::Event,
        id: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Name { name } = event {
            if let Some(seat) = state.seats.get_mut(id) {
                seat.2 = Some(name);
            }
        }
    }
}
macro_rules! manager_dispatch {
    ($interface:ty) => {
        impl Dispatch<$interface, ()> for State {
            fn event(
                _: &mut Self,
                _: &$interface,
                _: <$interface as Proxy>::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
            }
        }
    };
}
manager_dispatch!(ext_manager::ExtDataControlManagerV1);
manager_dispatch!(wlr_manager::ZwlrDataControlManagerV1);

macro_rules! device_dispatch {
    ($interface:ty, $event:path, $offer:ty, $variant:ident) => {
        impl Dispatch<$interface, ()> for State {
            fn event(state: &mut Self, _: &$interface, event: <$interface as Proxy>::Event, _: &(), connection: &Connection, _: &QueueHandle<Self>) {
                use $event as Event;
                match event {
                    Event::DataOffer { id } => state.offer(id.id().protocol_id(), RawOffer::$variant(id)),
                    Event::Selection { id } => state.selection(id.map(|value| value.id().protocol_id()), connection),
                    Event::PrimarySelection { id: Some(id) } => { state.offers.remove(&id.id().protocol_id()); }
                    Event::Finished => state.error = Some(WaylandError::PermissionDenied),
                    _ => (),
                }
            }
            wayland_client::event_created_child!(State, $interface, [0 => ($offer, ())]);
        }
    };
}
device_dispatch!(
    ext_device::ExtDataControlDeviceV1,
    ext_device::Event,
    ext_offer::ExtDataControlOfferV1,
    Ext
);
device_dispatch!(
    wlr_device::ZwlrDataControlDeviceV1,
    wlr_device::Event,
    wlr_offer::ZwlrDataControlOfferV1,
    Wlr
);

macro_rules! offer_dispatch {
    ($interface:ty, $event:path) => {
        impl Dispatch<$interface, ()> for State {
            fn event(
                state: &mut Self,
                offer: &$interface,
                event: <$interface as Proxy>::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
                use $event as Event;
                if let Event::Offer { mime_type } = event {
                    state.mime(offer.id().protocol_id(), mime_type);
                }
            }
        }
    };
}
offer_dispatch!(ext_offer::ExtDataControlOfferV1, ext_offer::Event);
offer_dispatch!(wlr_offer::ZwlrDataControlOfferV1, wlr_offer::Event);
macro_rules! source_dispatch {
    ($interface:ty, $event:path) => {
        impl Dispatch<$interface, ()> for State {
            fn event(
                state: &mut Self,
                _: &$interface,
                event: <$interface as Proxy>::Event,
                _: &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
                use $event as Event;
                match event {
                    Event::Send { mime_type, fd } => state.send(mime_type, fd),
                    Event::Cancelled => state.owner_cancelled = true,
                    _ => (),
                }
            }
        }
    };
}
source_dispatch!(ext_source::ExtDataControlSourceV1, ext_source::Event);
source_dispatch!(wlr_source::ZwlrDataControlSourceV1, wlr_source::Event);

struct Client {
    connection: Connection,
    queue: EventQueue<State>,
    state: State,
    manager: Option<Manager>,
    device: Option<Device>,
    status: Option<WaylandStatus>,
}
impl Client {
    fn new(cancel: Arc<UnixStream>) -> Result<Self, WaylandError> {
        if service_disabled("wayland_clipboard") {
            return Err(WaylandError::DisabledByConfiguration);
        }
        let connection = connect()?;
        let queue = connection.new_event_queue();
        let mut client = Self {
            connection,
            queue,
            state: State {
                globals: BTreeMap::new(),
                seats: BTreeMap::new(),
                manager_global: None,
                seat_global: None,
                sync: 0,
                offers: HashMap::new(),
                selection: None,
                sequence: 0,
                changed: false,
                error: None,
                owner_cancelled: false,
                payloads: Vec::new(),
                leases: Vec::new(),
                cancel,
            },
            manager: None,
            device: None,
            status: None,
        };
        let registry = client
            .connection
            .display()
            .get_registry(&client.queue.handle(), ());
        client.sync()?;
        let choice =
            choose_protocol(&client.state.globals).ok_or(WaylandError::DataControlUnavailable)?;
        let qh = client.queue.handle();
        client.manager = Some(match choice.1 {
            DataControlProtocol::Ext => Manager::Ext(registry.bind(choice.0, choice.3, &qh, ())),
            DataControlProtocol::Wlr => Manager::Wlr(registry.bind(choice.0, choice.3, &qh, ())),
        });
        client.state.manager_global = Some(choice.0);
        for (&id, (name, version)) in &client.state.globals {
            if name == "wl_seat" && *version >= 1 {
                let version = (*version).min(2);
                client
                    .state
                    .seats
                    .insert(id, (registry.bind(id, version, &qh, id), version, None));
            }
        }
        client.sync()?;
        let wanted = std::env::var("DEZIRCLIP_WAYLAND_SEAT").ok();
        let selected = client.state.seats.iter().find(|(_, value)| {
            wanted
                .as_ref()
                .is_none_or(|name| value.2.as_ref() == Some(name))
        });
        let (&seat_id, (seat, seat_version, seat_name)) = selected.ok_or(if wanted.is_some() {
            WaylandError::SeatNotFound
        } else {
            WaylandError::SeatUnavailable
        })?;
        let status = WaylandStatus {
            experimental: true,
            protocol: choice.1,
            advertised_version: choice.2,
            bound_version: choice.3,
            seat_global: seat_id,
            seat_name: seat_name.clone(),
            seat_version: *seat_version,
            permission: "data_device_negotiated",
            source_identity: "not_provided_by_data_control",
            primary_selection: false,
        };
        client.device = Some(match client.manager.as_ref().expect("manager was bound") {
            Manager::Ext(manager) => Device::Ext(manager.get_data_device(seat, &qh, ())),
            Manager::Wlr(manager) => Device::Wlr(manager.get_data_device(seat, &qh, ())),
        });
        client.state.seat_global = Some(seat_id);
        client.sync()?;
        client.status = Some(status);
        Ok(client)
    }
    fn dispatch_until(
        &mut self,
        deadline: Instant,
        done: impl Fn(&State) -> bool,
    ) -> Result<(), WaylandError> {
        loop {
            io::check_cancel(Some(&self.state.cancel))?;
            self.queue
                .dispatch_pending(&mut self.state)
                .map_err(|_| WaylandError::Disconnected)?;
            if let Some(error) = self.state.error {
                return Err(error);
            }
            if done(&self.state) {
                return Ok(());
            }
            self.queue.flush().map_err(|_| WaylandError::Disconnected)?;
            if let Some(guard) = self.queue.prepare_read() {
                io::wait(
                    guard.connection_fd().as_raw_fd(),
                    libc::POLLIN,
                    Some(&self.state.cancel),
                    deadline,
                )?;
                guard.read().map_err(|_| WaylandError::Disconnected)?;
            }
        }
    }
    fn sync(&mut self) -> Result<(), WaylandError> {
        let token = self.state.sync + 1;
        self.connection.display().sync(&self.queue.handle(), token);
        self.dispatch_until(Instant::now() + SETUP_TIMEOUT, |state| state.sync >= token)
    }
    fn publish(&mut self, payloads: Vec<MimeData>) -> Result<Source, WaylandError> {
        validate_payloads(&payloads)?;
        io::check_cancel(Some(&self.state.cancel))?;
        self.state.payloads = payloads;
        let qh = self.queue.handle();
        let source = match self.manager.as_ref().expect("manager was bound") {
            Manager::Ext(value) => Source::Ext(value.create_data_source(&qh, ())),
            Manager::Wlr(value) => Source::Wlr(value.create_data_source(&qh, ())),
        };
        for data in &self.state.payloads {
            source.offer(data.mime.clone());
        }
        match (&self.device, &source) {
            (Some(Device::Ext(device)), Source::Ext(source)) => device.set_selection(Some(source)),
            (Some(Device::Wlr(device)), Source::Wlr(source)) => device.set_selection(Some(source)),
            _ => return Err(WaylandError::ProtocolRejected),
        }
        self.sync()?;
        if self.state.owner_cancelled {
            return Err(WaylandError::OwnerCancelled);
        }
        Ok(source)
    }
}

fn choose_protocol(
    globals: &BTreeMap<u32, (String, u32)>,
) -> Option<(u32, DataControlProtocol, u32, u32)> {
    globals
        .iter()
        .find(|(_, (name, version))| name == "ext_data_control_manager_v1" && *version >= 1)
        .map(|(&id, (_, version))| (id, DataControlProtocol::Ext, *version, 1))
        .or_else(|| {
            globals
                .iter()
                .find(|(_, (name, version))| {
                    name == "zwlr_data_control_manager_v1" && *version >= 1
                })
                .map(|(&id, (_, version))| {
                    (id, DataControlProtocol::Wlr, *version, (*version).min(2))
                })
        })
}

fn validate_payloads(payloads: &[MimeData]) -> Result<(), WaylandError> {
    if payloads.is_empty() || payloads.len() > 128 {
        return Err(WaylandError::UnsupportedMime);
    }
    let mut total = 0usize;
    let mut unique = std::collections::HashSet::new();
    let mut buffers = std::collections::HashSet::new();
    for data in payloads {
        if data.mime.is_empty()
            || data.mime.len() > 256
            || data.mime.contains(['\0', '\r', '\n'])
            || !unique.insert(&data.mime)
        {
            return Err(WaylandError::UnsupportedMime);
        }
        if buffers.insert((data.bytes.as_ptr() as usize, data.bytes.len())) {
            total = total
                .checked_add(data.bytes.len())
                .ok_or(WaylandError::TransferBudgetExceeded)?;
        }
        if total > MAX_TRANSFER_BYTES {
            return Err(WaylandError::TransferBudgetExceeded);
        }
    }
    Ok(())
}

fn cancel_pair() -> Result<(Arc<UnixStream>, UnixStream), WaylandError> {
    let (read, write) = UnixStream::pair().map_err(|_| WaylandError::ConnectionUnavailable)?;
    write
        .set_nonblocking(true)
        .map_err(|_| WaylandError::ConnectionUnavailable)?;
    Ok((Arc::new(read), write))
}

pub struct WaylandClipboardBackend;
impl WaylandClipboardBackend {
    pub fn monitor_active() -> bool {
        MONITOR_READY.load(Ordering::Acquire)
    }
    pub fn probe() -> Result<WaylandStatus, WaylandError> {
        let (cancel, _trigger) = cancel_pair()?;
        Client::new(cancel)?
            .status
            .ok_or(WaylandError::ProtocolRejected)
    }
    pub fn snapshot() -> Result<SelectionSnapshot, WaylandError> {
        if MONITOR_ACTIVE.load(Ordering::Acquire) {
            return CURRENT
                .get_or_init(Default::default)
                .lock()
                .map_err(|_| WaylandError::Disconnected)?
                .clone()
                .ok_or(WaylandError::EmptySelection);
        }
        let (cancel, _trigger) = cancel_pair()?;
        Client::new(cancel)?
            .state
            .selection
            .ok_or(WaylandError::EmptySelection)
    }
    /// Owns the selection until replaced, cancelled or disconnected. The ready
    /// callback runs only after the compositor has processed set_selection.
    pub fn serve(
        payloads: Vec<MimeData>,
        ready: impl FnOnce(),
        cancel: Arc<UnixStream>,
    ) -> Result<(), WaylandError> {
        Self::serve_authorized(payloads, Vec::new(), None, ready, cancel)
    }
    pub(crate) fn serve_authorized(
        payloads: Vec<MimeData>,
        leases: Vec<dzc_core::api::NativeReadLease>,
        mut exports: Option<crate::private_files::BoundExport>,
        ready: impl FnOnce(),
        cancel: Arc<UnixStream>,
    ) -> Result<(), WaylandError> {
        let result = (|| {
            let mut client = Client::new(cancel)?;
            if leases.iter().any(|lease| !lease.is_current()) {
                return Err(WaylandError::PermissionDenied);
            }
            client.state.leases = leases;
            let _source = client.publish(payloads)?;
            if client.state.leases.iter().any(|lease| !lease.is_current()) {
                return Err(WaylandError::PermissionDenied);
            }
            ready();
            let private_deadline = Instant::now() + Duration::from_secs(300);
            loop {
                if !client.state.leases.is_empty()
                    && (Instant::now() >= private_deadline
                        || client.state.leases.iter().any(|lease| !lease.is_current()))
                {
                    return Ok(());
                }
                // Long blocking waits are interrupted by the owned cancellation socket.
                // Private owners additionally bound export lifetime, matching the native child lease.
                let wait = if client.state.leases.is_empty() {
                    Duration::from_secs(24 * 60 * 60)
                } else {
                    Duration::from_millis(100)
                };
                match client.dispatch_until(Instant::now() + wait, |state| state.owner_cancelled) {
                    Ok(()) | Err(WaylandError::Cancelled) => return Ok(()),
                    Err(WaylandError::TimedOut) => (),
                    Err(error) => return Err(error),
                }
                if client.state.owner_cancelled {
                    return Ok(());
                }
            }
        })();
        if let Some(export) = exports.as_mut() {
            export.close().map_err(|_| WaylandError::PermissionDenied)?;
        }
        result
    }
    pub fn monitor(
        cancel: Arc<UnixStream>,
        mut changed: impl FnMut(u64),
    ) -> Result<(), WaylandError> {
        Self::monitor_ready(cancel, || {}, &mut changed)
    }
    pub(crate) fn monitor_ready(
        cancel: Arc<UnixStream>,
        ready: impl FnOnce(),
        mut changed: impl FnMut(u64),
    ) -> Result<(), WaylandError> {
        if service_disabled("clipboard_listener") || service_disabled("wayland_clipboard") {
            return Err(WaylandError::DisabledByConfiguration);
        }
        if MONITOR_ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(WaylandError::ProtocolRejected);
        }
        struct Clear;
        impl Drop for Clear {
            fn drop(&mut self) {
                if let Ok(mut value) = CURRENT.get_or_init(Default::default).lock() {
                    *value = None;
                }
                CURRENT_SEQUENCE.store(0, Ordering::Release);
                MONITOR_READY.store(false, Ordering::Release);
                MONITOR_ACTIVE.store(false, Ordering::Release);
            }
        }
        let _clear = Clear;
        let mut client = Client::new(cancel)?;
        MONITOR_READY.store(true, Ordering::Release);
        ready();
        loop {
            if client.state.changed {
                client.state.changed = false;
                *CURRENT
                    .get_or_init(Default::default)
                    .lock()
                    .map_err(|_| WaylandError::Disconnected)? = client.state.selection.clone();
                CURRENT_SEQUENCE.store(client.state.sequence, Ordering::Release);
                changed(client.state.sequence);
            }
            match client.dispatch_until(
                Instant::now() + Duration::from_secs(24 * 60 * 60),
                |state| state.changed,
            ) {
                Ok(()) => (),
                Err(WaylandError::Cancelled) => return Ok(()),
                Err(WaylandError::TimedOut) => (),
                Err(error) => return Err(error),
            }
        }
    }
    pub fn cancellation_pair() -> Result<(Arc<UnixStream>, UnixStream), WaylandError> {
        cancel_pair()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn protocol_selection_prefers_ext_and_only_binds_supported_versions() {
        let globals = [
            (2, ("zwlr_data_control_manager_v1".into(), 9)),
            (3, ("ext_data_control_manager_v1".into(), 4)),
        ]
        .into();
        assert_eq!(
            choose_protocol(&globals),
            Some((3, DataControlProtocol::Ext, 4, 1))
        );
        assert_eq!(
            choose_protocol(&[(2, ("zwlr_data_control_manager_v1".into(), 1))].into()),
            Some((2, DataControlProtocol::Wlr, 1, 1))
        );
        assert!(choose_protocol(&[(2, ("wl_data_device_manager".into(), 3))].into()).is_none());
    }
    #[test]
    fn mime_payloads_are_bounded_and_duplicates_are_rejected() {
        let data = MimeData {
            mime: "text/plain".into(),
            bytes: Arc::from(&b"synthetic"[..]),
        };
        assert!(validate_payloads(&[data.clone()]).is_ok());
        assert_eq!(
            validate_payloads(&[data.clone(), data]),
            Err(WaylandError::UnsupportedMime)
        );
        assert_eq!(
            validate_payloads(&[MimeData {
                mime: "text/plain\n".into(),
                bytes: Arc::from([])
            }]),
            Err(WaylandError::UnsupportedMime)
        );
    }
    #[test]
    fn isolated_uri_offer_falls_back_to_text_unless_local_files_exist() {
        if std::env::var("DEZIRCLIP_RUN_WAYLAND_SMOKE_TESTS").as_deref() != Ok("1") {
            return;
        }
        let runtime = std::path::PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap());
        assert!(
            runtime.is_absolute() && runtime.join("dezirclip-isolated-wayland-fixture").is_file()
        );
        assert!(std::env::var_os("DISPLAY").is_none());
        use dzc_core::capture::{CaptureData, CaptureReadRequest, CaptureReader, CaptureSkip};
        use std::io::Write;
        for uri in [
            "https://example.invalid/synthetic\r\n",
            "file:///tmp/synthetic-file.txt\r\n",
        ] {
            for files in [true, false] {
                let (cancel, mut trigger) = cancel_pair().unwrap();
                let (tx, rx) = std::sync::mpsc::sync_channel(1);
                let payloads = vec![
                    MimeData {
                        mime: "text/uri-list".into(),
                        bytes: Arc::from(uri.as_bytes()),
                    },
                    MimeData {
                        mime: "text/plain".into(),
                        bytes: Arc::from(&b"synthetic URI fallback"[..]),
                    },
                ];
                let worker = std::thread::spawn(move || {
                    WaylandClipboardBackend::serve(
                        payloads,
                        || {
                            tx.send(()).unwrap();
                        },
                        cancel,
                    )
                });
                rx.recv_timeout(Duration::from_secs(5)).unwrap();
                let mut reader = crate::capture::NativeCaptureReader::default();
                let data = reader
                    .read(CaptureReadRequest {
                        files,
                        rich_text: true,
                    })
                    .unwrap();
                if uri.starts_with("https:") {
                    assert!(
                        matches!(data, Some(CaptureData::Text(value)) if value == "synthetic URI fallback")
                    );
                    assert!(!reader.metadata().files);
                } else if files {
                    assert!(
                        matches!(data, Some(CaptureData::Files(paths)) if paths == vec!["/tmp/synthetic-file.txt"])
                    );
                    assert!(reader.metadata().files);
                } else {
                    assert!(data.is_none());
                    assert_eq!(reader.empty_reason(), CaptureSkip::FilesDisabled);
                }
                trigger.write_all(&[1]).unwrap();
                assert_eq!(worker.join().unwrap(), Ok(()));
            }
        }
    }
    #[test]
    fn isolated_compositor_clipboard_lifecycle() {
        if std::env::var("DEZIRCLIP_RUN_WAYLAND_SMOKE_TESTS").as_deref() != Ok("1") {
            return;
        }
        let runtime =
            std::path::PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").expect("private runtime"));
        assert!(
            runtime.is_absolute() && runtime.join("dezirclip-isolated-wayland-fixture").is_file()
        );
        assert!(std::env::var_os("DISPLAY").is_none());
        let status = WaylandClipboardBackend::probe().unwrap();
        println!(
            "native Wayland negotiated: {}",
            serde_json::to_string(&status).unwrap()
        );
        let owner = |text: &'static [u8], rich: bool, sensitive: bool| {
            let (cancel, trigger) = cancel_pair().unwrap();
            let (tx, rx) = std::sync::mpsc::sync_channel(1);
            let mut payloads = vec![MimeData {
                mime: "text/plain".into(),
                bytes: Arc::from(text),
            }];
            if rich {
                payloads.push(MimeData {
                    mime: "text/html".into(),
                    bytes: Arc::from(&b"<b>synthetic A</b>"[..]),
                });
            }
            if sensitive {
                payloads.push(MimeData {
                    mime: PASSWORD_HINT_MIME.into(),
                    bytes: Arc::from(&b"secret"[..]),
                });
            }
            let thread = std::thread::spawn(move || {
                WaylandClipboardBackend::serve(
                    payloads,
                    || {
                        tx.send(()).unwrap();
                    },
                    cancel,
                )
            });
            rx.recv_timeout(Duration::from_secs(5)).unwrap();
            (trigger, thread)
        };
        let (_first_cancel, first) = owner(b"synthetic A", true, true);
        let first_offer = WaylandClipboardBackend::snapshot().unwrap();
        assert_eq!(
            first_offer.receive("text/plain", 1024).unwrap(),
            b"synthetic A"
        );
        assert_eq!(
            first_offer.receive("text/html", 1024).unwrap(),
            b"<b>synthetic A</b>"
        );
        assert_eq!(first_offer.password_hint().unwrap(), Some(true));
        use dzc_core::capture::{CaptureData, CaptureReadRequest, CaptureReader};
        let request = CaptureReadRequest {
            files: true,
            rich_text: true,
        };
        assert_eq!(
            crate::capture::NativeCaptureReader::default()
                .read(request)
                .unwrap_err()
                .code,
            "WaylandExplicitPrivacyExclusion"
        );
        assert_eq!(
            first_offer.receive("application/x-not-offered", 64),
            Err(WaylandError::UnsupportedMime)
        );
        let (cancel, mut stop) = cancel_pair().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let monitor = std::thread::spawn(move || {
            WaylandClipboardBackend::monitor(cancel, |sequence| {
                tx.send(sequence).unwrap();
            })
        });
        let first_sequence = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let (_second_cancel, second) = owner(b"synthetic B", false, false);
        let second_sequence = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(second_sequence > first_sequence);
        assert_eq!(first.join().unwrap(), Ok(()));
        let current = WaylandClipboardBackend::snapshot().unwrap();
        assert_eq!(current.sequence(), second_sequence);
        assert_eq!(current.receive("text/plain", 1024).unwrap(), b"synthetic B");
        assert_eq!(current.password_hint().unwrap(), None);
        assert!(
            matches!(crate::capture::NativeCaptureReader::default().read(request).unwrap(), Some(CaptureData::Text(value)) if value == "synthetic B")
        );
        let (mut third_cancel, third) = owner(b"   ", true, false);
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(second.join().unwrap(), Ok(()));
        assert!(
            matches!(crate::capture::NativeCaptureReader::default().read(request).unwrap(), Some(CaptureData::RichText { text, html }) if text == "   " && html == "<b>synthetic A</b>")
        );
        use std::io::Write;
        let started = Instant::now();
        stop.write_all(&[1]).unwrap();
        assert_eq!(monitor.join().unwrap(), Ok(()));
        assert!(started.elapsed() < Duration::from_secs(2));
        third_cancel.write_all(&[1]).unwrap();
        assert_eq!(third.join().unwrap(), Ok(()));
        assert!(matches!(
            WaylandClipboardBackend::snapshot(),
            Err(WaylandError::EmptySelection)
        ));
        let mut subscription = crate::capture::subscribe_clipboard(Arc::new(|_| {})).unwrap();
        subscription.wait_ready(Duration::from_secs(5)).unwrap();
        assert!(!subscription.is_stopped());
        subscription.stop().unwrap();
        assert!(subscription.is_stopped());
        subscription.stop().unwrap();
        println!("native text/HTML/hint/owner-replacement/cancel/clear assertions passed");
    }
}
