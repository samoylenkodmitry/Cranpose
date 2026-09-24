//! Network status (connectivity + whether the connection is metered). Apps use
//! it to defer or confirm large transfers on cellular/metered links.
//!
//! The default reports online + unmetered; platform backends can install a real
//! monitor via [`set_platform_network_monitor`] (iOS `NWPathMonitor`, Android
//! `ConnectivityManager`, web `navigator.connection`).

use std::sync::{Arc, OnceLock};

use crate::registry::{RecoveryGate, ServiceRegistry};

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
static NETWORK_MONITOR_HANDLE: OnceLock<NetworkMonitorRef> = OnceLock::new();
static NETWORK_RECOVERY: RecoveryGate = RecoveryGate::new();

struct PlatformNetworkMonitor;

fn registered_network_monitor() -> NetworkMonitorRef {
    PLATFORM_NETWORK_MONITOR
        .get_or_warn("network monitor")
        .unwrap_or_else(|| {
            DEFAULT_NETWORK_MONITOR
                .get_or_init(|| Arc::new(DefaultNetworkMonitor))
                .clone()
        })
}

fn active_network_monitor() -> NetworkMonitorRef {
    let monitor = registered_network_monitor();
    if monitor.is_alive() {
        NETWORK_RECOVERY.succeeded();
    } else if NETWORK_RECOVERY.try_start() {
        monitor.reconnect();
    }
    monitor
}

impl NetworkMonitor for PlatformNetworkMonitor {
    fn status(&self) -> NetworkStatus {
        active_network_monitor().status()
    }

    fn is_alive(&self) -> bool {
        registered_network_monitor().is_alive()
    }

    fn reconnect(&self) {
        registered_network_monitor().reconnect();
    }
}

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
    NETWORK_MONITOR_HANDLE
        .get_or_init(|| Arc::new(PlatformNetworkMonitor))
        .clone()
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
#[path = "tests/network_status_tests.rs"]
mod tests;
