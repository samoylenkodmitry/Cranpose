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

// A query to every node on a link goes to `ff02::1` with the link's interface as the scope: a
// datagram without the scope would leave on the default network's interface.
#[test]
fn the_group_of_all_nodes_names_the_networks_interface() {
    let network = usb_network(17);
    let SocketAddr::V6(group) = network.all_nodes(9950) else {
        panic!("an IPv6 group")
    };
    assert_eq!(*group.ip(), Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 1));
    assert!(group.ip().is_multicast());
    assert_eq!(group.scope_id(), 17);
    assert_eq!(group.port(), 9950);
}
