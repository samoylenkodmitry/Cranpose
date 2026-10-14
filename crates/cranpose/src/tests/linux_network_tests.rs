use std::time::{Duration, Instant};

use super::*;

const NM_STATE_UNKNOWN: u32 = 0;
const NM_STATE_CONNECTING: u32 = 40;
const NM_METERED_UNKNOWN: u32 = 0;

#[test]
fn a_global_connection_is_online_and_metered_as_networkmanager_guesses() {
    let online = |metering| NetworkStatus {
        connectivity: Connectivity::Online,
        metering,
    };
    for (metered, metering) in [
        (NM_METERED_YES, Metering::Metered),
        (NM_METERED_GUESS_YES, Metering::Metered),
        (NM_METERED_NO, Metering::Unmetered),
        (NM_METERED_GUESS_NO, Metering::Unmetered),
        (NM_METERED_UNKNOWN, Metering::Unknown),
    ] {
        assert_eq!(
            manager_status(NM_STATE_CONNECTED_GLOBAL, metered),
            online(metering)
        );
    }
}

#[test]
fn a_connection_without_the_internet_is_offline_and_meters_nothing() {
    for state in [
        NM_STATE_ASLEEP,
        NM_STATE_DISCONNECTED,
        NM_STATE_DISCONNECTING,
        NM_STATE_CONNECTED_LOCAL,
        NM_STATE_CONNECTED_SITE,
    ] {
        assert_eq!(
            manager_status(state, NM_METERED_YES),
            NetworkStatus {
                connectivity: Connectivity::Offline,
                metering: Metering::Unknown,
            },
            "state {state}"
        );
    }
    for state in [NM_STATE_UNKNOWN, NM_STATE_CONNECTING] {
        assert_eq!(
            manager_status(state, NM_METERED_NO),
            NetworkStatus::default(),
            "state {state} does not say yet whether the internet is reachable"
        );
    }
}

/// Needs NetworkManager on the system bus, as on the Linux CI runner; where
/// it is absent the monitor reports `Unknown`, which is that case's answer.
#[test]
fn the_registered_monitor_reports_networkmanager_state() {
    let reachable = std::process::Command::new("busctl")
        .args(["--system", "status", SERVICE])
        .output()
        .is_ok_and(|output| output.status.success());
    register();
    let deadline = Instant::now() + Duration::from_secs(5);
    while cranpose_services::network_status().connectivity == Connectivity::Unknown
        && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(10));
    }
    let connectivity = cranpose_services::network_status().connectivity;
    if reachable {
        assert_ne!(
            connectivity,
            Connectivity::Unknown,
            "NetworkManager runs but the monitor read nothing"
        );
    } else {
        assert_eq!(connectivity, Connectivity::Unknown);
    }
}
