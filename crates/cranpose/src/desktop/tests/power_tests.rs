use super::*;

#[test]
fn capabilities_match_what_this_build_can_actually_read() {
    let capabilities = DesktopPowerMonitor.capabilities();
    assert_eq!(
        capabilities.thermal,
        DesktopPowerMonitor.thermal_state().is_supported(),
        "a backend that claims thermal support must not answer Unsupported"
    );
    assert_eq!(
        capabilities.battery,
        DesktopPowerMonitor.battery_status().is_supported(),
        "a backend that claims a battery must not answer Unsupported"
    );
    assert!(!capabilities.background_restriction);
}

#[cfg(target_os = "macos")]
#[test]
fn a_mac_reports_a_thermal_reading_rather_than_unsupported() {
    assert!(matches!(
        DesktopPowerMonitor.thermal_state(),
        PowerReading::Known(_)
    ));
}

/// A Mac reports the battery `pmset` shows, and no battery when it shows
/// none.
#[cfg(target_os = "macos")]
#[test]
fn a_mac_reports_the_battery_pmset_shows() {
    let output = std::process::Command::new("pmset")
        .args(["-g", "batt"])
        .output()
        .expect("pmset runs");
    let shown = String::from_utf8_lossy(&output.stdout).into_owned();
    let reading = DesktopPowerMonitor.battery_status();
    let Some(line) = shown.lines().find(|line| line.contains("InternalBattery")) else {
        assert!(matches!(reading, PowerReading::Unsupported), "{shown}");
        return;
    };
    let percent: u8 = line
        .split('\t')
        .nth(1)
        .and_then(|fields| fields.split('%').next())
        .and_then(|percent| percent.trim().parse().ok())
        .expect("pmset shows the charge");
    let PowerReading::Known(status) = reading else {
        panic!("a Mac with a battery read none: {shown}");
    };
    assert!(
        status.percent.abs_diff(percent) <= 1,
        "{status:?} against {shown}"
    );
    assert_eq!(status.charging, shown.contains("'AC Power'"), "{shown}");
}

#[cfg(target_os = "linux")]
#[test]
fn a_linux_zone_is_at_the_highest_trip_point_its_temperature_reached() {
    use std::path::Path;

    fn zone(root: &Path, name: &str, mode: &str, temperature: u32, trips: &[(&str, u32)]) {
        let zone = root.join(name);
        std::fs::create_dir_all(&zone).expect("zone");
        std::fs::write(zone.join("mode"), mode).expect("mode");
        std::fs::write(zone.join("temp"), format!("{temperature}\n")).expect("temp");
        for (index, (kind, trip)) in trips.iter().enumerate() {
            std::fs::write(zone.join(format!("trip_point_{index}_type")), kind).expect("type");
            std::fs::write(
                zone.join(format!("trip_point_{index}_temp")),
                trip.to_string(),
            )
            .expect("trip");
        }
    }

    let root = crate::test_scratch_dir("thermal-zones");
    assert!(matches!(
        linux::thermal_state(&root),
        PowerReading::Unsupported
    ));
    zone(
        &root,
        "thermal_zone0",
        "enabled",
        30_000,
        &[("active", 50_000)],
    );
    assert!(matches!(
        linux::thermal_state(&root),
        PowerReading::Known(ThermalState::Normal)
    ));
    zone(
        &root,
        "thermal_zone1",
        "enabled",
        62_000,
        &[
            ("critical", 105_000),
            ("passive", 60_000),
            ("active", 50_000),
        ],
    );
    zone(
        &root,
        "thermal_zone2",
        "disabled",
        99_000,
        &[("hot", 90_000)],
    );
    assert!(
        matches!(
            linux::thermal_state(&root),
            PowerReading::Known(ThermalState::Moderate)
        ),
        "past the passive trip point; the disabled zone does not count"
    );
}
