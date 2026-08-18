//! Network status (connectivity + whether the connection is metered). Apps use
//! it to defer or confirm large transfers on cellular/metered links.
//!
//! The default reports online + unmetered; platform backends can install a real
//! monitor via [`set_platform_network_monitor`] (iOS `NWPathMonitor`, Android
//! `ConnectivityManager`, web `navigator.connection`).

use crate::registry::{RecoveryGate, ServiceRegistry};
use std::sync::{Arc, OnceLock};

/// A snapshot of the current network state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NetworkStatus {
    /// Whether the device currently has a usable network path.
    pub online: bool,
    /// Whether the active path is metered/expensive (cellular, hotspot, …).
    pub metered: bool,
}

impl Default for NetworkStatus {
    fn default() -> Self {
        Self {
            online: true,
            metered: false,
        }
    }
}

/// Reports the current network status.
pub trait NetworkMonitor: Send + Sync {
    fn status(&self) -> NetworkStatus;
    fn is_alive(&self) -> bool;
    fn reconnect(&self);
}

pub type NetworkMonitorRef = Arc<dyn NetworkMonitor>;

struct DefaultNetworkMonitor;

impl NetworkMonitor for DefaultNetworkMonitor {
    fn status(&self) -> NetworkStatus {
        NetworkStatus::default()
    }

    fn is_alive(&self) -> bool {
        true
    }

    fn reconnect(&self) {}
}

static PLATFORM_NETWORK_MONITOR: ServiceRegistry<dyn NetworkMonitor> = ServiceRegistry::new();
static DEFAULT_NETWORK_MONITOR: OnceLock<NetworkMonitorRef> = OnceLock::new();
static NETWORK_RECOVERY: RecoveryGate = RecoveryGate::new();

/// Installs a platform network monitor, replacing any previous one.
pub fn set_platform_network_monitor(monitor: NetworkMonitorRef) {
    PLATFORM_NETWORK_MONITOR.set(monitor);
    NETWORK_RECOVERY.succeeded();
}

/// Removes any registered platform network monitor (tests and teardown).
pub fn clear_platform_network_monitor() {
    PLATFORM_NETWORK_MONITOR.clear();
}

/// The active network monitor: the platform one if installed, else the default
/// (online, unmetered).
pub fn network_monitor() -> NetworkMonitorRef {
    PLATFORM_NETWORK_MONITOR
        .get_or_warn("network monitor")
        .unwrap_or_else(|| {
            DEFAULT_NETWORK_MONITOR
                .get_or_init(|| Arc::new(DefaultNetworkMonitor))
                .clone()
        })
}

/// Convenience: the current network status.
pub fn network_status() -> NetworkStatus {
    let monitor = network_monitor();
    if monitor.is_alive() {
        NETWORK_RECOVERY.succeeded();
    } else if NETWORK_RECOVERY.try_start() {
        monitor.reconnect();
    }
    monitor.status()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

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
}
