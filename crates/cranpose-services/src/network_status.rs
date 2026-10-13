//! Network status (connectivity + whether the connection is metered). Apps use
//! it to defer or confirm large transfers on cellular/metered links.
//!
//! Each part is an observation or [`Unknown`](Connectivity::Unknown): with no
//! platform monitor installed via [`set_platform_network_monitor`], or one
//! that stopped, both parts are unknown. Android observes both through
//! `ConnectivityManager`; web observes connectivity through `navigator.onLine`
//! and cannot tell metering. An app that defers large transfers on metered
//! links starts one on its own only when the status says
//! [`Unmetered`](Metering::Unmetered).

use std::sync::{Arc, OnceLock};

use crate::registry::{RecoveryGate, ServiceRegistry};

/// Whether the device has a usable network path.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Connectivity {
    /// A path that can reach the internet.
    Online,
    /// No usable path.
    Offline,
    /// Nothing observes connectivity: no platform monitor, or one that
    /// stopped. A request may still succeed.
    #[default]
    Unknown,
}

/// Whether the active network path costs the user by the byte.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Metering {
    /// Cellular, a hotspot, or a network the user marked as metered.
    Metered,
    /// Wi-Fi or Ethernet the user did not mark as metered.
    Unmetered,
    /// The platform cannot tell, nothing observes the network, or there is
    /// no active path to meter.
    #[default]
    Unknown,
}

/// A snapshot of the current network state. The default knows nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct NetworkStatus {
    pub connectivity: Connectivity,
    pub metering: Metering,
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
    /// The monitor's observation, or nothing known while it stays stopped.
    fn status(&self) -> NetworkStatus {
        let monitor = active_network_monitor();
        if monitor.is_alive() {
            monitor.status()
        } else {
            NetworkStatus::default()
        }
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

/// The active network monitor: the platform one if installed, else a default
/// that knows nothing.
pub fn network_monitor() -> NetworkMonitorRef {
    NETWORK_MONITOR_HANDLE
        .get_or_init(|| Arc::new(PlatformNetworkMonitor))
        .clone()
}

/// The current network status. A stopped platform monitor gets a reconnect
/// attempt first.
pub fn network_status() -> NetworkStatus {
    network_monitor().status()
}

#[cfg(test)]
#[path = "tests/network_status_tests.rs"]
mod tests;
