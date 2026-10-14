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
            thermal: self.thermal_state().is_supported(),
            battery: self.battery_status().is_supported(),
            background_restriction: false,
        }
    }

    fn thermal_state(&self) -> PowerReading<ThermalState> {
        #[cfg(target_os = "macos")]
        {
            macos::thermal_state()
        }
        #[cfg(target_os = "linux")]
        {
            linux::thermal_state(std::path::Path::new(linux::THERMAL))
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            PowerReading::Unsupported
        }
    }

    fn battery_status(&self) -> PowerReading<BatteryStatus> {
        #[cfg(target_os = "linux")]
        {
            linux::battery_status()
        }
        #[cfg(target_os = "windows")]
        {
            windows_power::battery_status()
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            PowerReading::Unsupported
        }
    }
}

/// The battery from WinRT's `PowerManager`.
#[cfg(target_os = "windows")]
mod windows_power {
    use windows::System::Power::{BatteryStatus as WindowsBattery, PowerManager};

    use super::{BatteryStatus, PowerReading};

    pub(super) fn battery_status() -> PowerReading<BatteryStatus> {
        let Ok(status) = PowerManager::BatteryStatus() else {
            return PowerReading::Unsupported;
        };
        if status == WindowsBattery::NotPresent {
            return PowerReading::Unsupported;
        }
        let Ok(percent) = PowerManager::RemainingChargePercent() else {
            return PowerReading::Unsupported;
        };
        PowerReading::Known(BatteryStatus {
            percent: u8::try_from(percent.clamp(0, 100)).unwrap_or(100),
            // Idle: on external power and neither charging nor draining, as
            // a full battery is.
            charging: status == WindowsBattery::Charging || status == WindowsBattery::Idle,
        })
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

    use super::{BatteryStatus, PowerReading, ThermalState};

    const POWER_SUPPLY: &str = "/sys/class/power_supply";
    pub(super) const THERMAL: &str = "/sys/class/thermal";

    /// The highest pressure over the enabled thermal zones under `root`:
    /// each zone is at the highest trip point its temperature reached.
    /// `Unsupported` when no zone has a temperature and trip points.
    pub(super) fn thermal_state(root: &Path) -> PowerReading<ThermalState> {
        let Ok(entries) = std::fs::read_dir(root) else {
            return PowerReading::Unsupported;
        };
        let state = entries
            .flatten()
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("thermal_zone")
            })
            .filter_map(|entry| zone_state(&entry.path()))
            .max();
        state.map_or(PowerReading::Unsupported, PowerReading::Known)
    }

    fn zone_state(zone: &Path) -> Option<ThermalState> {
        if read_trimmed(&zone.join("mode")).as_deref() == Some("disabled") {
            return None;
        }
        let temperature: i64 = read_trimmed(&zone.join("temp"))?.parse().ok()?;
        let mut state = None;
        for index in 0.. {
            let Some(kind) = read_trimmed(&zone.join(format!("trip_point_{index}_type"))) else {
                break;
            };
            let Some(trip) = read_trimmed(&zone.join(format!("trip_point_{index}_temp")))
                .and_then(|trip| trip.parse::<i64>().ok())
            else {
                continue;
            };
            let reached = if temperature >= trip {
                trip_level(&kind)
            } else {
                ThermalState::Normal
            };
            state = Some(state.map_or(reached, |state: ThermalState| state.max(reached)));
        }
        state
    }

    /// The pressure a trip point of `kind` stands for: fans at an active
    /// one, throttling at a passive one, shutdown close at hot and critical.
    fn trip_level(kind: &str) -> ThermalState {
        match kind {
            "critical" => ThermalState::Critical,
            "hot" => ThermalState::Severe,
            "passive" => ThermalState::Moderate,
            "active" => ThermalState::Light,
            _ => ThermalState::Normal,
        }
    }

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
#[path = "tests/desktop_power_tests.rs"]
mod tests;
