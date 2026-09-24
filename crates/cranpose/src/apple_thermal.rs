use cranpose_services::ThermalState;
use objc2_foundation::{NSProcessInfo, NSProcessInfoThermalState};

pub(crate) fn thermal_state() -> ThermalState {
    from_process_info(NSProcessInfo::processInfo().thermalState())
}

pub(crate) fn from_process_info(state: NSProcessInfoThermalState) -> ThermalState {
    if state >= NSProcessInfoThermalState::Critical {
        ThermalState::Critical
    } else if state >= NSProcessInfoThermalState::Serious {
        ThermalState::Severe
    } else if state >= NSProcessInfoThermalState::Fair {
        ThermalState::Light
    } else {
        ThermalState::Normal
    }
}

#[cfg(test)]
#[path = "tests/apple_thermal_tests.rs"]
mod tests;
