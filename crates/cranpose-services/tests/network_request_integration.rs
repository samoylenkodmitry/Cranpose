use std::{
    io,
    net::{IpAddr, Ipv6Addr, SocketAddr, TcpListener},
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use cranpose_services::{
    LinkSpeed, Network, NetworkHandle, NetworkReach, NetworkRequest, NetworkRequestId,
    NetworkRequestState, NetworkRequester, NetworkTransport, SocketHandle,
    clear_platform_network_requester, publish_network_request_state, request_network,
    set_platform_network_requester,
};

fn held<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

// The requester is one per process, so the tests take turns.
fn one_requester_at_a_time() -> MutexGuard<'static, ()> {
    static REQUESTER: Mutex<()> = Mutex::new(());
    held(&REQUESTER)
}

// A platform that records what was asked of it.
#[derive(Default)]
struct Fake {
    requests: Mutex<Vec<(NetworkRequestId, NetworkRequest)>>,
    released: Mutex<Vec<NetworkRequestId>>,
    binds: AtomicUsize,
    refuse_requests: AtomicBool,
    refuse_binds: AtomicBool,
}

impl Fake {
    fn installed() -> Arc<Self> {
        let fake = Arc::new(Self::default());
        set_platform_network_requester(Arc::clone(&fake) as Arc<dyn NetworkRequester>);
        fake
    }

    fn last_id(&self) -> NetworkRequestId {
        held(&self.requests).last().expect("a request").0
    }
}

impl NetworkRequester for Fake {
    fn request(&self, id: NetworkRequestId, request: NetworkRequest) -> Result<(), String> {
        if self.refuse_requests.load(Ordering::SeqCst) {
            return Err("refused".into());
        }
        held(&self.requests).push((id, request));
        Ok(())
    }

    fn release(&self, id: NetworkRequestId) {
        held(&self.released).push(id);
    }

    fn bind_socket(&self, _network: NetworkHandle, _socket: SocketHandle<'_>) -> io::Result<()> {
        self.binds.fetch_add(1, Ordering::SeqCst);
        if self.refuse_binds.load(Ordering::SeqCst) {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "no"));
        }
        Ok(())
    }
}

fn usb_network(interface_index: u32) -> Network {
    Network {
        handle: NetworkHandle(7),
        interface_name: "usb0".into(),
        interface_index,
        addresses: vec![IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 2))],
        link_speed: LinkSpeed {
            downstream_kbps: Some(480_000),
            upstream_kbps: Some(480_000),
        },
    }
}

#[test]
fn a_platform_without_network_requests_answers_unavailable_at_once() {
    let _turn = one_requester_at_a_time();
    clear_platform_network_requester();
    let requested = request_network(NetworkRequest::local(NetworkTransport::Usb));
    assert_eq!(requested.state(), NetworkRequestState::Unavailable);
    assert_eq!(requested.network(), None);
}

#[test]
fn a_request_follows_the_systems_answers_and_gives_its_network_back_when_dropped() {
    let _turn = one_requester_at_a_time();
    let fake = Fake::installed();
    let requested = request_network(NetworkRequest::local(NetworkTransport::Usb));
    assert_eq!(requested.state(), NetworkRequestState::Pending);
    let (id, asked) = *held(&fake.requests).last().expect("a request");
    assert_eq!(asked.transport, NetworkTransport::Usb);
    assert_eq!(asked.reach, NetworkReach::Local);

    publish_network_request_state(id, NetworkRequestState::Available(usb_network(17)));
    assert_eq!(requested.network(), Some(usb_network(17)));
    // The network went away. The request stays and gets the next network
    publish_network_request_state(id, NetworkRequestState::Lost);
    assert_eq!(requested.state(), NetworkRequestState::Lost);
    publish_network_request_state(id, NetworkRequestState::Available(usb_network(18)));
    assert_eq!(
        requested.network().map(|network| network.interface_index),
        Some(18)
    );

    drop(requested);
    assert_eq!(*held(&fake.released), [id]);
    // A late answer to the dropped request reaches no newer request
    let next = request_network(NetworkRequest::internet(NetworkTransport::Wifi));
    publish_network_request_state(id, NetworkRequestState::Available(usb_network(17)));
    assert_eq!(next.state(), NetworkRequestState::Pending);
    assert_ne!(fake.last_id(), id);
    clear_platform_network_requester();
}

#[test]
fn a_request_that_the_system_refuses_is_unavailable() {
    let _turn = one_requester_at_a_time();
    let fake = Fake::installed();
    fake.refuse_requests.store(true, Ordering::SeqCst);
    let requested = request_network(NetworkRequest::local(NetworkTransport::Ethernet));
    assert_eq!(requested.state(), NetworkRequestState::Unavailable);
    clear_platform_network_requester();
}

#[test]
fn sockets_are_bound_to_the_network_before_they_connect() {
    let _turn = one_requester_at_a_time();
    let fake = Fake::installed();
    let network = usb_network(0);
    let listener = TcpListener::bind((Ipv6Addr::LOCALHOST, 0)).expect("a listener");
    let address = listener.local_addr().expect("its address");

    let stream = network
        .connect_tcp(address, Duration::from_secs(2))
        .expect("a connection");
    assert_eq!(stream.peer_addr().expect("a peer"), address);
    let udp = network
        .bind_udp(SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0)))
        .expect("a UDP socket");
    assert_ne!(udp.local_addr().expect("its address").port(), 0);
    assert_eq!(fake.binds.load(Ordering::SeqCst), 2);

    // A network that the system will not bind gives no socket at all
    fake.refuse_binds.store(true, Ordering::SeqCst);
    let refused = network.connect_tcp(address, Duration::from_secs(2));
    assert_eq!(
        refused.map(|_| ()).map_err(|error| error.kind()),
        Err(io::ErrorKind::PermissionDenied)
    );
    clear_platform_network_requester();
    let unbound = network.bind_udp(SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0)));
    assert_eq!(
        unbound.map(|_| ()).map_err(|error| error.kind()),
        Err(io::ErrorKind::Unsupported)
    );
}

// The loopback interface of macOS has a link-local address, so a query to
// the group of all nodes on it finds a service the way it does on a USB link.
#[cfg(target_os = "macos")]
#[test]
fn a_query_to_all_nodes_finds_a_service_by_its_link_local_address() {
    let _turn = one_requester_at_a_time();
    let _fake = Fake::installed();
    let loopback = 1;
    let service = std::net::UdpSocket::bind((Ipv6Addr::UNSPECIFIED, 0)).expect("a service");
    service
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("a timeout");
    let port = service.local_addr().expect("its address").port();
    let network = usb_network(loopback);
    let client = network
        .bind_udp(SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0)))
        .expect("a client");
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("a timeout");

    client
        .send_to(b"who", network.all_nodes(port))
        .expect("a query");
    let mut buf = [0u8; 8];
    let (n, asker) = service.recv_from(&mut buf).expect("the query");
    assert_eq!(&buf[..n], b"who");
    service.send_to(b"me", asker).expect("an answer");
    let (n, from) = client.recv_from(&mut buf).expect("the answer");
    assert_eq!(&buf[..n], b"me");
    let SocketAddr::V6(from) = from else {
        panic!("an IPv6 answer")
    };
    assert!(from.ip().is_unicast_link_local(), "{from}");
    assert_eq!(from.scope_id(), loopback);
    clear_platform_network_requester();
}
