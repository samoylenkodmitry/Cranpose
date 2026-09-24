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
