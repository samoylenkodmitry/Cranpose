use std::sync::PoisonError;

use super::*;

struct DesktopMonitor;

impl PowerMonitor for DesktopMonitor {
    fn capabilities(&self) -> PowerCapabilities {
        PowerCapabilities {
            thermal: false,
            battery: false,
            background_restriction: false,
        }
    }
}

struct PhoneMonitor;

impl PowerMonitor for PhoneMonitor {
    fn capabilities(&self) -> PowerCapabilities {
        PowerCapabilities {
            thermal: true,
            battery: true,
            background_restriction: true,
        }
    }
    fn thermal_state(&self) -> PowerReading<ThermalState> {
        PowerReading::Known(ThermalState::Severe)
    }
    fn battery_status(&self) -> PowerReading<BatteryStatus> {
        PowerReading::Known(BatteryStatus {
            percent: 12,
            charging: false,
        })
    }
    fn unrestricted_background_work(&self) -> PowerReading<bool> {
        PowerReading::Known(false)
    }
}

#[test]
fn a_platform_without_power_apis_says_unsupported_rather_than_full() {
    let _guard = crate::registry::test_service_guard();
    clear_platform_power_monitor();
    let state = power_state();
    assert_eq!(state, PowerState::unsupported());
    assert!(!state.thermal.is_supported());
    assert!(!state.should_pause_work());
    assert_eq!(power_capabilities(), PowerCapabilities::default());
}

#[test]
fn a_backend_that_measures_nothing_still_reports_its_capabilities() {
    let _guard = crate::registry::test_service_guard();
    set_platform_power_monitor(Arc::new(DesktopMonitor));
    assert!(!power_capabilities().thermal);
    assert_eq!(power_state().battery, PowerReading::Unsupported);
    clear_platform_power_monitor();
}

#[test]
fn severe_thermal_pressure_pauses_sustained_work() {
    let _guard = crate::registry::test_service_guard();
    set_platform_power_monitor(Arc::new(PhoneMonitor));
    let state = power_state();
    assert!(state.should_pause_work());
    assert_eq!(
        state.battery.known().map(|battery| battery.percent),
        Some(12)
    );
    assert!(!state.unrestricted_background_work.unwrap_or(true));
    clear_platform_power_monitor();
}

#[test]
fn observers_see_published_changes_and_stop_when_dropped() {
    let _guard = crate::registry::test_service_guard();
    clear_platform_power_monitor();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorder = Arc::clone(&seen);
    let registration = observe_power_state(move |state| {
        recorder
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(state.thermal);
    });
    publish_power_state(PowerState {
        thermal: PowerReading::Known(ThermalState::Moderate),
        ..PowerState::unsupported()
    });
    assert_eq!(
        seen.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_slice(),
        [PowerReading::Known(ThermalState::Moderate)]
    );
    drop(registration);
    publish_power_state(PowerState::unsupported());
    assert_eq!(seen.lock().unwrap_or_else(PoisonError::into_inner).len(), 1);
}
