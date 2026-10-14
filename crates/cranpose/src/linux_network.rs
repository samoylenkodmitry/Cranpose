//! Network status on Linux from NetworkManager over the system D-Bus: its
//! `State` and `Metered` properties, read again on each of its
//! `PropertiesChanged` signals. Without NetworkManager, or without a system
//! bus, the status stays `Unknown`.

use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use cranpose_services::{
    Connectivity, Metering, NetworkMonitor, NetworkStatus, set_platform_network_monitor,
};
use zbus::blocking::{Connection, Proxy, fdo::PropertiesProxy};

const SERVICE: &str = "org.freedesktop.NetworkManager";
const PATH: &str = "/org/freedesktop/NetworkManager";
const INTERFACE: &str = "org.freedesktop.NetworkManager";

/// `NM_STATE_*`: the overall state of the machine's networking.
const NM_STATE_ASLEEP: u32 = 10;
const NM_STATE_DISCONNECTED: u32 = 20;
const NM_STATE_DISCONNECTING: u32 = 30;
const NM_STATE_CONNECTED_LOCAL: u32 = 50;
const NM_STATE_CONNECTED_SITE: u32 = 60;
const NM_STATE_CONNECTED_GLOBAL: u32 = 70;

/// `NM_METERED_*` of the primary connection.
const NM_METERED_YES: u32 = 1;
const NM_METERED_NO: u32 = 2;
const NM_METERED_GUESS_YES: u32 = 3;
const NM_METERED_GUESS_NO: u32 = 4;

struct LinuxNetworkMonitor {
    status: Arc<Mutex<NetworkStatus>>,
}

impl NetworkMonitor for LinuxNetworkMonitor {
    fn status(&self) -> NetworkStatus {
        *self.status.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn is_alive(&self) -> bool {
        true
    }

    fn reconnect(&self) {}
}

/// Starts watching NetworkManager once and installs the monitor.
pub(crate) fn register() {
    static STATUS: OnceLock<Arc<Mutex<NetworkStatus>>> = OnceLock::new();
    let status = STATUS.get_or_init(|| {
        let status = Arc::new(Mutex::new(NetworkStatus::default()));
        let observed = Arc::clone(&status);
        let started = std::thread::Builder::new()
            .name("cranpose-network".into())
            .spawn(move || {
                if let Err(error) = watch(&observed) {
                    log::info!("network status: NetworkManager is not reachable: {error}");
                }
            });
        if let Err(error) = started {
            log::warn!("network status: could not start its thread: {error}");
        }
        status
    });
    set_platform_network_monitor(Arc::new(LinuxNetworkMonitor {
        status: Arc::clone(status),
    }));
}

/// Reads NetworkManager's state now and after each change, until the bus
/// connection ends.
fn watch(status: &Mutex<NetworkStatus>) -> zbus::Result<()> {
    let connection = Connection::system()?;
    let manager = Proxy::new_owned(connection.clone(), SERVICE, PATH, INTERFACE)?;
    let properties = PropertiesProxy::builder(&connection)
        .destination(SERVICE)?
        .path(PATH)?
        .build()?;
    let changes = properties.receive_properties_changed()?;
    let read = || -> zbus::Result<NetworkStatus> {
        Ok(manager_status(
            manager.get_property::<u32>("State")?,
            manager.get_property::<u32>("Metered")?,
        ))
    };
    store(status, read()?);
    for change in changes {
        if change
            .args()
            .is_ok_and(|args| args.interface_name().as_str() == INTERFACE)
        {
            store(status, read()?);
        }
    }
    Ok(())
}

fn store(status: &Mutex<NetworkStatus>, reading: NetworkStatus) {
    *status.lock().unwrap_or_else(PoisonError::into_inner) = reading;
}

/// What NetworkManager's `State` and `Metered` say. Only a global
/// connection reaches the internet; a local or site one does not.
fn manager_status(state: u32, metered: u32) -> NetworkStatus {
    let connectivity = match state {
        NM_STATE_CONNECTED_GLOBAL => Connectivity::Online,
        NM_STATE_ASLEEP
        | NM_STATE_DISCONNECTED
        | NM_STATE_DISCONNECTING
        | NM_STATE_CONNECTED_LOCAL
        | NM_STATE_CONNECTED_SITE => Connectivity::Offline,
        // Unknown, or connecting: the next change says how it ended.
        _ => Connectivity::Unknown,
    };
    let metering = match (connectivity, metered) {
        (Connectivity::Online, NM_METERED_YES | NM_METERED_GUESS_YES) => Metering::Metered,
        (Connectivity::Online, NM_METERED_NO | NM_METERED_GUESS_NO) => Metering::Unmetered,
        _ => Metering::Unknown,
    };
    NetworkStatus {
        connectivity,
        metering,
    }
}

#[cfg(test)]
#[path = "tests/linux_network_tests.rs"]
mod tests;
