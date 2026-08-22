//! Desktop power and device information.
//!
//! Registered as the platform power monitor by the desktop backend, so an
//! application reads heat and battery through the same contract on a laptop as
//! on a phone.
//!
//! What a desktop can answer differs by system, and the point of
//! [`PowerReading`] is that the difference is visible rather than papered over:
//!
//! | | thermal | battery |
//! | --- | --- | --- |
//! | macOS | `NSProcessInfo`'s thermal pressure | not read here |
//! | Linux | no pressure API to read | `/sys/class/power_supply` |
//! | Windows | not read here | not read here |
//!
//! Linux exposes temperatures, not pressure. Turning degrees into
//! [`ThermalState`] means choosing thresholds per machine, and a number this
//! module invented would read as a measurement while being a guess — so it
//! answers `Unsupported`, which is what it can honestly say.

use cranpose_services::{
    set_platform_power_monitor, BatteryStatus, PowerCapabilities, PowerMonitor, PowerReading,
    ThermalState,
};
use std::sync::Arc;

/// Installs the desktop power monitor.
pub(crate) fn register() {
    set_platform_power_monitor(Arc::new(DesktopPowerMonitor));
}

struct DesktopPowerMonitor;

impl PowerMonitor for DesktopPowerMonitor {
    fn capabilities(&self) -> PowerCapabilities {
        PowerCapabilities {
            thermal: cfg!(target_os = "macos"),
            battery: cfg!(target_os = "linux"),
            // No desktop system restricts an application's background work the
            // way a mobile one does; there is nothing to report or request.
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

    /// The same reading iOS gives, from the same Foundation call.
    pub(super) fn thermal_state() -> PowerReading<ThermalState> {
        PowerReading::Known(crate::apple_thermal::thermal_state())
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{BatteryStatus, PowerReading};
    use std::path::Path;

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
        // The directory exists on every Linux with a power-supply class, so a
        // machine with no battery in it is a desktop rather than an unknown.
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
        // The gap this closes: with no desktop backend registered at all, an
        // application throttling on heat could never throttle on a Mac.
        assert!(matches!(
            DesktopPowerMonitor.thermal_state(),
            PowerReading::Known(_)
        ));
    }
}
