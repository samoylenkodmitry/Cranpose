//! Network status on Windows from `Windows.Networking.Connectivity`: the
//! internet connection profile's connectivity level and cost, read again on
//! each `NetworkStatusChanged` event.

use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use cranpose_services::{
    Connectivity, Metering, NetworkMonitor, NetworkStatus, set_platform_network_monitor,
};
use windows::Networking::Connectivity::{
    ConnectionProfile, NetworkConnectivityLevel, NetworkCostType, NetworkInformation,
    NetworkStatusChangedEventHandler,
};

struct WindowsNetworkMonitor {
    status: Arc<Mutex<NetworkStatus>>,
}

impl NetworkMonitor for WindowsNetworkMonitor {
    fn status(&self) -> NetworkStatus {
        *self.status.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn is_alive(&self) -> bool {
        true
    }

    fn reconnect(&self) {}
}

/// Reads the status, subscribes to its changes once, and installs the
/// monitor. The subscription lasts as long as the process.
pub(crate) fn register() {
    static STATUS: OnceLock<Arc<Mutex<NetworkStatus>>> = OnceLock::new();
    let status = STATUS.get_or_init(|| {
        let status = Arc::new(Mutex::new(read_status()));
        let observed = Arc::clone(&status);
        let handler = NetworkStatusChangedEventHandler::new(move |_| {
            *observed.lock().unwrap_or_else(PoisonError::into_inner) = read_status();
            Ok(())
        });
        if let Err(error) = NetworkInformation::NetworkStatusChanged(&handler) {
            log::warn!("network status: no change events: {error}");
        }
        status
    });
    set_platform_network_monitor(Arc::new(WindowsNetworkMonitor {
        status: Arc::clone(status),
    }));
}

fn read_status() -> NetworkStatus {
    match NetworkInformation::GetInternetConnectionProfile() {
        Ok(profile) => profile_status(&profile).unwrap_or_default(),
        // No profile connects to the internet: the call returns null, which
        // reads as an error whose code is success.
        Err(error) if error.code().is_ok() => offline(),
        Err(_) => NetworkStatus::default(),
    }
}

fn profile_status(profile: &ConnectionProfile) -> windows::core::Result<NetworkStatus> {
    let level = profile.GetNetworkConnectivityLevel()?;
    let cost = profile.GetConnectionCost()?;
    let surcharged = cost.Roaming()? || cost.OverDataLimit()?;
    Ok(connection_status(
        level,
        cost.NetworkCostType()?,
        surcharged,
    ))
}

fn offline() -> NetworkStatus {
    NetworkStatus {
        connectivity: Connectivity::Offline,
        metering: Metering::Unknown,
    }
}

/// What a profile's connectivity level and cost say. Only internet access
/// is online: constrained access is a captive portal. A fixed or variable
/// cost, roaming or a passed data limit is metered.
fn connection_status(
    level: NetworkConnectivityLevel,
    cost: NetworkCostType,
    surcharged: bool,
) -> NetworkStatus {
    if level != NetworkConnectivityLevel::InternetAccess {
        return offline();
    }
    let metering =
        if surcharged || cost == NetworkCostType::Fixed || cost == NetworkCostType::Variable {
            Metering::Metered
        } else if cost == NetworkCostType::Unrestricted {
            Metering::Unmetered
        } else {
            Metering::Unknown
        };
    NetworkStatus {
        connectivity: Connectivity::Online,
        metering,
    }
}

#[cfg(test)]
#[path = "tests/windows_network_tests.rs"]
mod tests;
