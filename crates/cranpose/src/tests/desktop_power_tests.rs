use super::*;

#[test]
fn capabilities_match_what_this_build_can_actually_read() {
    let capabilities = DesktopPowerMonitor.capabilities();
    assert_eq!(
        capabilities.thermal,
        DesktopPowerMonitor.thermal_state().is_supported(),
        "a backend that claims thermal support must not answer Unsupported"
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
