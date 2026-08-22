//! Thermal pressure as Foundation reports it, shared by both Apple shells.
//!
//! `NSProcessInfo.thermalState` is Foundation rather than UIKit, so a Mac and
//! an iPhone answer the same question with the same four levels. One mapping
//! serves both: a second copy would be a second place for the thresholds to
//! drift apart.

use cranpose_services::ThermalState;
use objc2_foundation::{NSProcessInfo, NSProcessInfoThermalState};

/// What the system currently reports.
pub(crate) fn thermal_state() -> ThermalState {
    from_process_info(NSProcessInfo::processInfo().thermalState())
}

/// The framework level for one `NSProcessInfo` pressure value.
///
/// Compared with `>=` rather than matched exactly: the enum is one Apple may
/// extend, and a level above `Critical` is still critical.
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
