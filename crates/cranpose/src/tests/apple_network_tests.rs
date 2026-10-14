use std::time::{Duration, Instant};

use super::*;

const NW_PATH_STATUS_INVALID: i32 = 0;
const NW_PATH_STATUS_SATISFIABLE: i32 = 3;

#[test]
fn a_satisfied_path_is_metered_when_expensive_or_constrained() {
    let online = |metering| NetworkStatus {
        connectivity: Connectivity::Online,
        metering,
    };
    assert_eq!(
        path_status(NW_PATH_STATUS_SATISFIED, false, false),
        online(Metering::Unmetered)
    );
    assert_eq!(
        path_status(NW_PATH_STATUS_SATISFIED, true, false),
        online(Metering::Metered),
        "cellular or a hotspot"
    );
    assert_eq!(
        path_status(NW_PATH_STATUS_SATISFIED, false, true),
        online(Metering::Metered),
        "Low Data Mode"
    );
}

#[test]
fn a_path_that_is_not_up_meters_nothing() {
    assert_eq!(
        path_status(NW_PATH_STATUS_UNSATISFIED, true, true),
        NetworkStatus {
            connectivity: Connectivity::Offline,
            metering: Metering::Unknown,
        }
    );
    for status in [NW_PATH_STATUS_SATISFIABLE, NW_PATH_STATUS_INVALID] {
        assert_eq!(
            path_status(status, true, false),
            NetworkStatus::default(),
            "a path that a connection attempt may bring up is not known"
        );
    }
}

#[test]
fn the_registered_monitor_reports_the_current_path() {
    register();
    let deadline = Instant::now() + Duration::from_secs(5);
    while cranpose_services::network_status().connectivity == Connectivity::Unknown
        && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_ne!(
        cranpose_services::network_status().connectivity,
        Connectivity::Unknown,
        "the path monitor reported no path"
    );
}
