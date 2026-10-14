//! The battery of a Mac, from IOKit's power sources: the internal
//! battery's charge, and whether external power feeds it.
#![expect(unsafe_code)]

use cranpose_services::{BatteryStatus, PowerReading};
use objc2_core_foundation::{CFDictionary, CFNumber, CFRetained, CFString, CFType};
use objc2_io_kit::{
    IOPSCopyPowerSourcesInfo, IOPSCopyPowerSourcesList, IOPSGetPowerSourceDescription,
};

pub(crate) fn battery_status() -> PowerReading<BatteryStatus> {
    internal_battery().map_or(PowerReading::Unsupported, PowerReading::Known)
}

fn internal_battery() -> Option<BatteryStatus> {
    let info = IOPSCopyPowerSourcesInfo()?;
    // SAFETY: `info` is the snapshot IOKit returned, as the call asks for.
    let sources = unsafe { IOPSCopyPowerSourcesList(Some(&info)) }?;
    // SAFETY: the list holds power source handles, which are CF objects.
    let sources =
        unsafe { CFRetained::cast_unchecked::<objc2_core_foundation::CFArray<CFType>>(sources) };
    (0..sources.len()).find_map(|index| {
        let source = sources.get(index)?;
        // SAFETY: `source` comes from the list of `info`.
        let description = unsafe { IOPSGetPowerSourceDescription(Some(&info), Some(&source)) }?;
        // SAFETY: a power source description maps key strings to values.
        let description =
            unsafe { CFRetained::cast_unchecked::<CFDictionary<CFString, CFType>>(description) };
        battery_of(&description)
    })
}

/// The status of a power source described by `description`, when it is the
/// Mac's internal battery.
fn battery_of(description: &CFDictionary<CFString, CFType>) -> Option<BatteryStatus> {
    let text = |key: &'static str| {
        let value = description.get(&CFString::from_static_str(key))?;
        Some(value.downcast::<CFString>().ok()?.to_string())
    };
    let number = |key: &'static str| {
        let value = description.get(&CFString::from_static_str(key))?;
        value.downcast::<CFNumber>().ok()?.as_i64()
    };
    if text("Type")? != "InternalBattery" {
        return None;
    }
    let current = number("Current Capacity")?;
    let max = number("Max Capacity").filter(|max| *max > 0)?;
    let percent = (current * 100 / max).clamp(0, 100);
    Some(BatteryStatus {
        percent: u8::try_from(percent).unwrap_or(100),
        charging: text("Power Source State").is_some_and(|state| state == "AC Power"),
    })
}
