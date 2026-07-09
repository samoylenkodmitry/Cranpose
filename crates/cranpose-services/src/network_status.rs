//! Network status (connectivity + whether the connection is metered). Apps use
//! it to defer or confirm large transfers on cellular/metered links.
//!
//! The default reports online + unmetered; platform backends can install a real
//! monitor via [`set_platform_network_monitor`] (iOS `NWPathMonitor`, Android
//! `ConnectivityManager`, web `navigator.connection`).

use std::cell::RefCell;
use std::rc::Rc;

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
pub trait NetworkMonitor {
    fn status(&self) -> NetworkStatus;
}

pub type NetworkMonitorRef = Rc<dyn NetworkMonitor>;

struct DefaultNetworkMonitor;

impl NetworkMonitor for DefaultNetworkMonitor {
    fn status(&self) -> NetworkStatus {
        NetworkStatus::default()
    }
}

thread_local! {
    static PLATFORM_NETWORK_MONITOR: RefCell<Option<NetworkMonitorRef>> = const { RefCell::new(None) };
}

/// Installs a platform network monitor, replacing any previous one.
pub fn set_platform_network_monitor(monitor: NetworkMonitorRef) {
    PLATFORM_NETWORK_MONITOR.with(|cell| *cell.borrow_mut() = Some(monitor));
}

/// Removes any registered platform network monitor (tests and teardown).
pub fn clear_platform_network_monitor() {
    PLATFORM_NETWORK_MONITOR.with(|cell| *cell.borrow_mut() = None);
}

/// The active network monitor: the platform one if installed, else the default
/// (online, unmetered).
pub fn network_monitor() -> NetworkMonitorRef {
    PLATFORM_NETWORK_MONITOR
        .with(|cell| cell.borrow().clone())
        .unwrap_or_else(|| Rc::new(DefaultNetworkMonitor))
}

/// Convenience: the current network status.
pub fn network_status() -> NetworkStatus {
    network_monitor().status()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_online_unmetered_and_overridable() {
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
        }
        set_platform_network_monitor(Rc::new(Metered));
        assert!(network_status().metered);
        clear_platform_network_monitor();
    }
}
