use std::time::{Duration, Instant};

use super::*;

#[test]
fn internet_access_is_online_and_metered_by_its_cost() {
    let online = |metering| NetworkStatus {
        connectivity: Connectivity::Online,
        metering,
    };
    let internet = NetworkConnectivityLevel::InternetAccess;
    for (cost, metering) in [
        (NetworkCostType::Unrestricted, Metering::Unmetered),
        (NetworkCostType::Fixed, Metering::Metered),
        (NetworkCostType::Variable, Metering::Metered),
        (NetworkCostType::Unknown, Metering::Unknown),
    ] {
        assert_eq!(connection_status(internet, cost, false), online(metering));
    }
    assert_eq!(
        connection_status(internet, NetworkCostType::Unrestricted, true),
        online(Metering::Metered),
        "roaming or over the data limit"
    );
}

#[test]
fn a_profile_without_internet_access_is_offline() {
    for level in [
        NetworkConnectivityLevel::None,
        NetworkConnectivityLevel::LocalAccess,
        NetworkConnectivityLevel::ConstrainedInternetAccess,
    ] {
        assert_eq!(
            connection_status(level, NetworkCostType::Fixed, true),
            offline()
        );
    }
}

#[test]
fn the_registered_monitor_reports_the_internet_profile() {
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
        "Windows reported no connection profile"
    );
}
