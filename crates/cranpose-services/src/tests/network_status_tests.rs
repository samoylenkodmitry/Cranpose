use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use super::*;

const ONLINE_METERED: NetworkStatus = NetworkStatus {
    connectivity: Connectivity::Online,
    metering: Metering::Metered,
};
const ONLINE_UNMETERED: NetworkStatus = NetworkStatus {
    connectivity: Connectivity::Online,
    metering: Metering::Unmetered,
};
const OFFLINE: NetworkStatus = NetworkStatus {
    connectivity: Connectivity::Offline,
    metering: Metering::Unknown,
};

/// A monitor whose observation and liveness a test sets.
struct Observed {
    status: Mutex<NetworkStatus>,
    alive: AtomicBool,
    revives: bool,
    reconnects: AtomicUsize,
}

impl Observed {
    fn new(status: NetworkStatus, alive: bool, revives: bool) -> Arc<Self> {
        Arc::new(Self {
            status: Mutex::new(status),
            alive: AtomicBool::new(alive),
            revives,
            reconnects: AtomicUsize::new(0),
        })
    }

    fn observe(&self, status: NetworkStatus) {
        if let Ok(mut current) = self.status.lock() {
            *current = status;
        }
    }
}

impl NetworkMonitor for Observed {
    fn status(&self) -> NetworkStatus {
        self.status
            .lock()
            .map_or_else(|_| NetworkStatus::default(), |status| *status)
    }

    fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }

    fn reconnect(&self) {
        self.reconnects.fetch_add(1, Ordering::AcqRel);
        if self.revives {
            self.alive.store(true, Ordering::Release);
        }
    }
}

#[test]
fn without_a_platform_monitor_nothing_is_known() {
    let _guard = crate::registry::test_service_guard();
    clear_platform_network_monitor();
    assert_eq!(
        network_status(),
        NetworkStatus {
            connectivity: Connectivity::Unknown,
            metering: Metering::Unknown,
        },
        "the fallback must not invent an online, unmetered connection"
    );
}

#[test]
fn a_monitor_reports_what_it_observes_as_it_changes() {
    let _guard = crate::registry::test_service_guard();
    let monitor = Observed::new(ONLINE_UNMETERED, true, false);
    set_platform_network_monitor(monitor.clone());
    for status in [ONLINE_METERED, OFFLINE, ONLINE_UNMETERED] {
        monitor.observe(status);
        assert_eq!(network_status(), status);
    }
}

#[test]
fn a_stopped_monitor_reconnects_before_its_status_is_read() {
    let _guard = crate::registry::test_service_guard();
    let monitor = Observed::new(ONLINE_METERED, false, true);
    set_platform_network_monitor(monitor.clone());
    assert_eq!(network_status(), ONLINE_METERED);
    assert_eq!(monitor.reconnects.load(Ordering::Acquire), 1);
}

#[test]
fn a_monitor_that_stays_stopped_reports_nothing_known() {
    let _guard = crate::registry::test_service_guard();
    let monitor = Observed::new(ONLINE_UNMETERED, false, false);
    set_platform_network_monitor(monitor.clone());
    assert_eq!(
        network_status(),
        NetworkStatus::default(),
        "a stopped monitor's last observation is stale"
    );
    assert_eq!(monitor.reconnects.load(Ordering::Acquire), 1);
}
