use std::{rc::Rc, sync::Arc};

use cranpose_services::{
    BatteryStatus, DeviceInfo, PowerCapabilities, PowerMonitor, PowerReading, ThermalState,
    set_platform_device_info, set_platform_power_monitor,
};
use objc2_foundation::NSProcessInfo;

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
            background_restriction: false,
        }
    }

    fn thermal_state(&self) -> PowerReading<ThermalState> {
        PowerReading::Known(crate::apple_thermal::thermal_state())
    }

    fn battery_status(&self) -> PowerReading<BatteryStatus> {
        let Some(marker) = objc2::MainThreadMarker::new() else {
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
