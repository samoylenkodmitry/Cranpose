//! iOS device information via `NSProcessInfo`.
//!
//! Registered as the platform device info (see
//! [`cranpose_services::set_platform_device_info`]) by the iOS backend.

use std::{rc::Rc, sync::Arc};

use cranpose_services::{
    set_platform_device_info, set_platform_power_monitor, BatteryStatus, DeviceInfo,
    PowerCapabilities, PowerMonitor, PowerReading, ThermalState,
};
use objc2_foundation::NSProcessInfo;

/// Installs the iOS device info as the platform device info.
pub(crate) fn register() {
    set_platform_device_info(Rc::new(IosDeviceInfo));
    set_platform_power_monitor(Arc::new(IosPowerMonitor));
}

struct IosDeviceInfo;

impl DeviceInfo for IosDeviceInfo {
    fn total_memory_bytes(&self) -> Option<u64> {
        Some(NSProcessInfo::processInfo().physicalMemory())
    }
}

struct IosPowerMonitor;

impl PowerMonitor for IosPowerMonitor {
    fn capabilities(&self) -> PowerCapabilities {
        PowerCapabilities {
            thermal: true,
            battery: true,
            // iOS has Low Power Mode but no per-app background restriction to
            // report, so the framework says so rather than guessing.
            background_restriction: false,
        }
    }

    fn thermal_state(&self) -> PowerReading<ThermalState> {
        PowerReading::Known(crate::apple_thermal::thermal_state())
    }

    fn battery_status(&self) -> PowerReading<BatteryStatus> {
        let Some(marker) = objc2::MainThreadMarker::new() else {
            // Battery level is main-thread only on UIKit; off it, the platform
            // has an answer this call cannot reach.
            return PowerReading::Unknown;
        };
        use objc2_ui_kit::{UIDevice, UIDeviceBatteryState};
        let device = UIDevice::currentDevice(marker);
        device.setBatteryMonitoringEnabled(true);
        let level = device.batteryLevel();
        PowerReading::Known(BatteryStatus {
            percent: if level < 0.0 {
                100
            } else {
                (level * 100.0).round().clamp(0.0, 100.0) as u8
            },
            charging: matches!(
                device.batteryState(),
                UIDeviceBatteryState::Charging | UIDeviceBatteryState::Full
            ),
        })
    }
}
