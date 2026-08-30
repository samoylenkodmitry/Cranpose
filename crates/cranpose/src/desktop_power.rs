use std::sync::Arc;

use cranpose_services::{
    BatteryStatus, PowerCapabilities, PowerMonitor, PowerReading, ThermalState,
    set_platform_power_monitor,
};

pub(crate) fn register() {
    set_platform_power_monitor(Arc::new(DesktopPowerMonitor));
}

struct DesktopPowerMonitor;

impl PowerMonitor for DesktopPowerMonitor {
    fn capabilities(&self) -> PowerCapabilities {
        PowerCapabilities {
            thermal: cfg!(target_os = "macos"),
            battery: cfg!(target_os = "linux"),
            background_restriction: false,
        }
    }

    fn thermal_state(&self) -> PowerReading<ThermalState> {
        #[cfg(target_os = "macos")]
        {
            macos::thermal_state()
        }
        #[cfg(not(target_os = "macos"))]
        {
            PowerReading::Unsupported
        }
    }

    fn battery_status(&self) -> PowerReading<BatteryStatus> {
        #[cfg(target_os = "linux")]
        {
            linux::battery_status()
        }
        #[cfg(not(target_os = "linux"))]
        {
            PowerReading::Unsupported
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::{PowerReading, ThermalState};

    pub(super) fn thermal_state() -> PowerReading<ThermalState> {
        PowerReading::Known(crate::apple_thermal::thermal_state())
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::path::Path;

    use super::{BatteryStatus, PowerReading};

    const POWER_SUPPLY: &str = "/sys/class/power_supply";

    pub(super) fn battery_status() -> PowerReading<BatteryStatus> {
        let Ok(entries) = std::fs::read_dir(POWER_SUPPLY) else {
            return PowerReading::Unsupported;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if read_trimmed(&path.join("type")).as_deref() != Some("Battery") {
                continue;
            }
            let Some(status) = read_battery(&path) else {
                continue;
            };
            return PowerReading::Known(status);
        }
        PowerReading::Unsupported
    }

    fn read_battery(path: &Path) -> Option<BatteryStatus> {
        let percent: u8 = read_trimmed(&path.join("capacity"))?.parse().ok()?;
        let status = read_trimmed(&path.join("status"))?;
        Some(BatteryStatus {
            percent: percent.min(100),
            charging: matches!(status.as_str(), "Charging" | "Full"),
        })
    }

    fn read_trimmed(path: &Path) -> Option<String> {
        Some(std::fs::read_to_string(path).ok()?.trim().to_owned())
    }
}

#[cfg(test)]
mod tests {
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
}
