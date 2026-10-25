#![expect(unsafe_code)]
//! Network status on iOS and macOS. Network.framework's path monitor reports
//! each change of the default network path on a private queue, and the
//! monitor installed here keeps the last report.

use std::{
    ffi::c_void,
    sync::{Arc, Mutex, OnceLock, PoisonError},
};

use block2::{Block, RcBlock};
use cranpose_services::{
    Connectivity, Metering, NetworkMonitor, NetworkStatus, set_platform_network_monitor,
};
use dispatch2::{DispatchQueue, DispatchRetained};

/// An `nw_path_monitor_t` or an `nw_path_t`.
type NwObject = *mut c_void;

const NW_PATH_STATUS_SATISFIED: i32 = 1;
const NW_PATH_STATUS_UNSATISFIED: i32 = 2;

#[link(name = "Network", kind = "framework")]
unsafe extern "C" {
    fn nw_path_monitor_create() -> NwObject;
    fn nw_path_monitor_set_queue(monitor: NwObject, queue: *mut c_void);
    fn nw_path_monitor_set_update_handler(monitor: NwObject, handler: &Block<dyn Fn(NwObject)>);
    fn nw_path_monitor_start(monitor: NwObject);
    fn nw_path_get_status(path: NwObject) -> i32;
    fn nw_path_is_expensive(path: NwObject) -> bool;
    fn nw_path_is_constrained(path: NwObject) -> bool;
}

/// The path monitor of the process, which runs from its first registration
/// to the process's end.
struct PathMonitor {
    status: Arc<Mutex<NetworkStatus>>,
    _queue: DispatchRetained<DispatchQueue>,
}

struct AppleNetworkMonitor {
    status: Arc<Mutex<NetworkStatus>>,
}

impl NetworkMonitor for AppleNetworkMonitor {
    fn status(&self) -> NetworkStatus {
        *self.status.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn is_alive(&self) -> bool {
        true
    }

    fn reconnect(&self) {}
}

/// Starts the path monitor once and installs it as the network monitor.
pub(crate) fn register() {
    static MONITOR: OnceLock<PathMonitor> = OnceLock::new();
    let monitor = MONITOR.get_or_init(start_path_monitor);
    set_platform_network_monitor(Arc::new(AppleNetworkMonitor {
        status: Arc::clone(&monitor.status),
    }));
}

fn start_path_monitor() -> PathMonitor {
    let status = Arc::new(Mutex::new(NetworkStatus::default()));
    let observed = Arc::clone(&status);
    let handler = RcBlock::new(move |path: NwObject| {
        // SAFETY: the monitor passes a valid path for the length of the call.
        let reading = unsafe {
            path_status(
                nw_path_get_status(path),
                nw_path_is_expensive(path),
                nw_path_is_constrained(path),
            )
        };
        *observed.lock().unwrap_or_else(PoisonError::into_inner) = reading;
    });
    let queue = DispatchQueue::new("dev.cranpose.network", None);
    let queue_object = DispatchRetained::as_ptr(&queue).as_ptr().cast::<c_void>();
    // SAFETY: the monitor is new and nothing else holds it. It copies the
    // handler block and retains the queue, which `PathMonitor` also keeps,
    // and it is never released, so it reports until the process ends.
    unsafe {
        let monitor = nw_path_monitor_create();
        if !monitor.is_null() {
            nw_path_monitor_set_queue(monitor, queue_object);
            nw_path_monitor_set_update_handler(monitor, &handler);
            nw_path_monitor_start(monitor);
        }
    }
    PathMonitor {
        status,
        _queue: queue,
    }
}

/// What a path's status and costs say: an expensive path (cellular, a
/// personal hotspot) or a constrained one (Low Data Mode) is metered.
fn path_status(status: i32, expensive: bool, constrained: bool) -> NetworkStatus {
    let connectivity = match status {
        NW_PATH_STATUS_SATISFIED => Connectivity::Online,
        NW_PATH_STATUS_UNSATISFIED => Connectivity::Offline,
        // Invalid, or satisfiable: a connection attempt may still bring up
        // a path.
        _ => Connectivity::Unknown,
    };
    let metering = match connectivity {
        Connectivity::Online if expensive || constrained => Metering::Metered,
        Connectivity::Online => Metering::Unmetered,
        Connectivity::Offline | Connectivity::Unknown => Metering::Unknown,
    };
    NetworkStatus {
        connectivity,
        metering,
    }
}

#[cfg(test)]
#[path = "tests/network_tests.rs"]
mod tests;
