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
mod tests {
    use super::*;

    #[test]
    fn pressure_maps_in_ascending_order() {
        assert_eq!(
            from_process_info(NSProcessInfoThermalState::Nominal),
            ThermalState::Normal
        );
        assert_eq!(
            from_process_info(NSProcessInfoThermalState::Fair),
            ThermalState::Light
        );
        assert_eq!(
            from_process_info(NSProcessInfoThermalState::Serious),
            ThermalState::Severe
        );
        assert_eq!(
            from_process_info(NSProcessInfoThermalState::Critical),
            ThermalState::Critical
        );
    }
}
