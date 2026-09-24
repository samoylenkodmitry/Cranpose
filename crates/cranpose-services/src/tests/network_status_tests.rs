use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::*;

#[test]
fn default_is_online_unmetered_and_overridable() {
    let _guard = crate::registry::test_service_guard();
    clear_platform_network_monitor();
    assert_eq!(
        network_status(),
        NetworkStatus {
            online: true,
            metered: false
        }
    );
    struct Metered;
    impl NetworkMonitor for Metered {
        fn status(&self) -> NetworkStatus {
            NetworkStatus {
                online: true,
                metered: true,
            }
        }

        fn is_alive(&self) -> bool {
            true
        }

        fn reconnect(&self) {}
    }
    set_platform_network_monitor(Arc::new(Metered));
    assert!(network_status().metered);
    clear_platform_network_monitor();
}

#[test]
fn dead_monitor_reconnects_before_status_is_read() {
    let _guard = crate::registry::test_service_guard();
    struct Reconnecting {
        alive: AtomicBool,
        reconnects: AtomicUsize,
    }
    impl NetworkMonitor for Reconnecting {
        fn status(&self) -> NetworkStatus {
            NetworkStatus {
                online: self.alive.load(Ordering::Acquire),
                metered: false,
            }
        }
        fn is_alive(&self) -> bool {
            self.alive.load(Ordering::Acquire)
        }
        fn reconnect(&self) {
            self.reconnects.fetch_add(1, Ordering::AcqRel);
            self.alive.store(true, Ordering::Release);
        }
    }
    clear_platform_network_monitor();
    let monitor = Arc::new(Reconnecting {
        alive: AtomicBool::new(false),
        reconnects: AtomicUsize::new(0),
    });
    set_platform_network_monitor(monitor.clone());
    assert!(network_status().online);
    assert_eq!(monitor.reconnects.load(Ordering::Acquire), 1);
    clear_platform_network_monitor();
}
