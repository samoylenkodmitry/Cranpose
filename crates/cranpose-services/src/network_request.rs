//! Networks that an application asks the system for by the link they run
//! over, such as the network over a USB cable to one computer, and sockets
//! that use only that network.
//!
//! Jetpack Compose has no such API. The names follow Android's
//! `ConnectivityManager`: [`NetworkRequest`] stands for `NetworkRequest`,
//! [`NetworkTransport`] for `NetworkCapabilities.TRANSPORT_*`, and the states
//! of [`NetworkRequestState`] for the callbacks `onAvailable`, `onLost` and
//! `onUnavailable`.
//!
//! A request lives while its [`RequestedNetwork`] lives. Dropping it gives the
//! network back, as `unregisterNetworkCallback` does. The system may hand a
//! request a network, take it away and hand it another one later. The
//! application reads [`RequestedNetwork::state`] when it needs to know.
//!
//! A network's sockets come from [`Network::bind_udp`] and
//! [`Network::connect_tcp`]: the system then routes them over that network
//! only, whatever its default network is. [`Network::all_nodes`] names the
//! IPv6 group of every node on the network's link, so an application finds a
//! service on a network without addresses, such as a USB link with
//! link-local addresses only, by a datagram to that group. The replies carry
//! their senders' link-local addresses with the interface as their scope.
//!
//! Platforms: Android asks `ConnectivityManager.requestNetwork` in a build
//! whose build script declares `cranpose_capabilities::Use::network_request()`,
//! and binds sockets with `android_setsocknetwork`. A request without the
//! internet removes `NET_CAPABILITY_INTERNET` and `NET_CAPABILITY_TRUSTED`,
//! as a headset's USB network needs. Android serves a request of an
//! application in the background for a limited time only, so an application
//! asks while it is in front and asks again after
//! [`NetworkRequestState::Unavailable`]. Every other platform answers
//! [`NetworkRequestState::Unavailable`] at once: iOS, macOS, watchOS, Linux,
//! Windows and the web hand no network to one application, and their sockets
//! reach every interface that the system routes to.

use std::{
    fmt,
    net::{IpAddr, Ipv6Addr, SocketAddr, SocketAddrV6},
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
};
#[cfg(any(unix, windows))]
use std::{
    io,
    net::{TcpStream, UdpSocket},
    time::Duration,
};

#[cfg(any(unix, windows))]
use socket2::{Domain, Protocol, Socket, Type};

use crate::registry::ServiceRegistry;

/// The kind of link a network runs over, as Android's
/// `NetworkCapabilities.TRANSPORT_*` names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NetworkTransport {
    /// A mobile operator's network.
    Cellular,
    /// Wi-Fi.
    Wifi,
    /// A Bluetooth personal area network.
    Bluetooth,
    /// A wired Ethernet port.
    Ethernet,
    /// A USB cable to another device, such as a computer that a headset is
    /// plugged into.
    Usb,
}

/// How far a requested network must reach.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NetworkReach {
    /// The network reaches the internet.
    Internet,
    /// The network may reach only the devices on its own link.
    Local,
}

/// What an application asks the system for. Android's `NetworkRequest` with
/// one transport.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NetworkRequest {
    /// The link the network runs over.
    pub transport: NetworkTransport,
    /// Whether the network must reach the internet.
    pub reach: NetworkReach,
}

impl NetworkRequest {
    /// A network over `transport` that need not reach the internet, such as
    /// the network over a USB cable to one computer.
    pub const fn local(transport: NetworkTransport) -> Self {
        Self {
            transport,
            reach: NetworkReach::Local,
        }
    }

    /// A network over `transport` that reaches the internet.
    pub const fn internet(transport: NetworkTransport) -> Self {
        Self {
            transport,
            reach: NetworkReach::Internet,
        }
    }
}

/// The system's handle of a network, as Android's
/// `Network.getNetworkHandle()` gives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NetworkHandle(pub u64);

/// The speed of a network's link in kilobits a second, as the system
/// estimates it. `None` where the system gives no estimate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct LinkSpeed {
    /// Toward this device.
    pub downstream_kbps: Option<u32>,
    /// Away from this device.
    pub upstream_kbps: Option<u32>,
}

/// A network that the system handed to a request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Network {
    /// The system's handle, which binds sockets to the network.
    pub handle: NetworkHandle,
    /// The name of the network's interface, such as `usb0`.
    pub interface_name: String,
    /// The index of the interface: the scope of an IPv6 link-local address
    /// on this network.
    pub interface_index: u32,
    /// This device's addresses on the network.
    pub addresses: Vec<IpAddr>,
    /// The system's estimate of the link's speed.
    pub link_speed: LinkSpeed,
}

// The group of every IPv6 node on a link (RFC 4291).
const ALL_NODES: Ipv6Addr = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 1);

/// A socket of the system, borrowed for one call.
#[cfg(unix)]
pub type SocketHandle<'a> = std::os::fd::BorrowedFd<'a>;
/// A socket of the system, borrowed for one call.
#[cfg(windows)]
pub type SocketHandle<'a> = std::os::windows::io::BorrowedSocket<'a>;

#[cfg(unix)]
fn socket_handle(socket: &Socket) -> SocketHandle<'_> {
    std::os::fd::AsFd::as_fd(socket)
}

#[cfg(windows)]
fn socket_handle(socket: &Socket) -> SocketHandle<'_> {
    std::os::windows::io::AsSocket::as_socket(socket)
}

impl Network {
    /// The group of every IPv6 node on this network's link, `ff02::1` with
    /// the network's interface as its scope, at `port`. A datagram from
    /// [`bind_udp`](Self::bind_udp) to it reaches every device on the link,
    /// which answers from its link-local address.
    pub fn all_nodes(&self, port: u16) -> SocketAddr {
        SocketAddr::V6(SocketAddrV6::new(ALL_NODES, port, 0, self.interface_index))
    }

    /// A UDP socket bound to `local` that sends and receives over this
    /// network only.
    #[cfg(any(unix, windows))]
    pub fn bind_udp(&self, local: SocketAddr) -> io::Result<UdpSocket> {
        let socket = Socket::new(Domain::for_address(local), Type::DGRAM, Some(Protocol::UDP))?;
        self.bind_socket(&socket)?;
        socket.bind(&local.into())?;
        Ok(socket.into())
    }

    /// A TCP connection to `remote` over this network only. `timeout` bounds
    /// the connect.
    #[cfg(any(unix, windows))]
    pub fn connect_tcp(&self, remote: SocketAddr, timeout: Duration) -> io::Result<TcpStream> {
        let socket = Socket::new(
            Domain::for_address(remote),
            Type::STREAM,
            Some(Protocol::TCP),
        )?;
        self.bind_socket(&socket)?;
        socket.connect_timeout(&remote.into(), timeout)?;
        Ok(socket.into())
    }

    #[cfg(any(unix, windows))]
    fn bind_socket(&self, socket: &Socket) -> io::Result<()> {
        let requester = PLATFORM_NETWORK_REQUESTER.get().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Unsupported,
                "this platform binds no socket to a network",
            )
        })?;
        requester.bind_socket(self.handle, socket_handle(socket))
    }
}

/// Where a request stands.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum NetworkRequestState {
    /// The system has not answered yet. A request for a USB network waits
    /// here while no cable is in, or while the other device has not allowed
    /// the link.
    #[default]
    Pending,
    /// The system handed this network to the request. Android's
    /// `onAvailable`, with the properties of its link.
    Available(Network),
    /// The network went away, as Android's `onLost` says. The request stays,
    /// and the system may hand it a network again.
    Lost,
    /// The system will not serve the request, as Android's `onUnavailable`
    /// says, or the platform hands no network to an application. The request
    /// is over: a new one has to be made.
    Unavailable,
}

/// The number of one request, which a platform carries back with its
/// answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NetworkRequestId(u64);

impl NetworkRequestId {
    /// The id with this number, as a platform got it from
    /// [`NetworkRequestId::get`].
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// The number of this id.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A platform's way to ask the system for networks.
pub trait NetworkRequester: Send + Sync {
    /// Asks the system for a network. The platform reports each answer with
    /// [`publish_network_request_state`] and the same `id`. An error means
    /// that the system took no request.
    fn request(&self, id: NetworkRequestId, request: NetworkRequest) -> Result<(), String>;
    /// Gives the network of the request back and ends the request.
    fn release(&self, id: NetworkRequestId);
    /// Makes the system route `socket` over the network with `network` as
    /// its handle only. The socket is not connected yet.
    #[cfg(any(unix, windows))]
    fn bind_socket(&self, network: NetworkHandle, socket: SocketHandle<'_>) -> io::Result<()>;
}

/// Shared handle to a [`NetworkRequester`].
pub type NetworkRequesterRef = Arc<dyn NetworkRequester>;

static PLATFORM_NETWORK_REQUESTER: ServiceRegistry<dyn NetworkRequester> = ServiceRegistry::new();

struct Entry {
    id: NetworkRequestId,
    state: NetworkRequestState,
}

static REQUESTS: Mutex<Vec<Entry>> = Mutex::new(Vec::new());
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn requests() -> MutexGuard<'static, Vec<Entry>> {
    REQUESTS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Installs the platform's way to ask for networks, replacing any previous
/// one. Requests made before keep their state.
pub fn set_platform_network_requester(requester: NetworkRequesterRef) {
    PLATFORM_NETWORK_REQUESTER.set(requester);
}

/// Removes the platform's way to ask for networks (tests and teardown).
pub fn clear_platform_network_requester() {
    PLATFORM_NETWORK_REQUESTER.clear();
}

/// A request that holds its network while it lives. Dropping it gives the
/// network back.
pub struct RequestedNetwork {
    id: NetworkRequestId,
    request: NetworkRequest,
}

impl RequestedNetwork {
    /// What was asked.
    pub fn request(&self) -> NetworkRequest {
        self.request
    }

    /// Where the request stands now.
    pub fn state(&self) -> NetworkRequestState {
        requests()
            .iter()
            .find(|entry| entry.id == self.id)
            .map_or(NetworkRequestState::Unavailable, |entry| {
                entry.state.clone()
            })
    }

    /// The network that the system handed to the request, while it has one.
    pub fn network(&self) -> Option<Network> {
        match self.state() {
            NetworkRequestState::Available(network) => Some(network),
            _ => None,
        }
    }
}

impl fmt::Debug for RequestedNetwork {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RequestedNetwork")
            .field("request", &self.request)
            .field("state", &self.state())
            .finish()
    }
}

impl Drop for RequestedNetwork {
    fn drop(&mut self) {
        requests().retain(|entry| entry.id != self.id);
        if let Some(requester) = PLATFORM_NETWORK_REQUESTER.get() {
            requester.release(self.id);
        }
    }
}

/// Asks the system for a network as `request` says. The request lives
/// while the returned value lives. Where the platform hands no network to an
/// application, its state is [`NetworkRequestState::Unavailable`] at once.
pub fn request_network(request: NetworkRequest) -> RequestedNetwork {
    let id = NetworkRequestId(NEXT_ID.fetch_add(1, Ordering::Relaxed));
    requests().push(Entry {
        id,
        state: NetworkRequestState::Pending,
    });
    let asked = match PLATFORM_NETWORK_REQUESTER.get() {
        Some(requester) => requester.request(id, request),
        None => Err("this platform hands no network to an application".to_owned()),
    };
    if let Err(reason) = asked {
        log::info!("network request for {request:?}: {reason}");
        publish_network_request_state(id, NetworkRequestState::Unavailable);
    }
    RequestedNetwork { id, request }
}

/// Records the system's answer to a request. A platform calls it from any
/// thread. An answer to a request that was dropped is ignored.
pub fn publish_network_request_state(id: NetworkRequestId, state: NetworkRequestState) {
    if let Some(entry) = requests().iter_mut().find(|entry| entry.id == id) {
        entry.state = state;
    }
}
