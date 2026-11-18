use super::*;
use crate::SpatialCapabilities;

#[test]
fn a_preference_is_kept_between_zero_and_one() {
    cranpose_testing::run_test_composition(|_| {
        let environment = SpatialEnvironment::new();
        assert_eq!(environment.preferred_passthrough_opacity(), None);
        environment.set_preferred_passthrough_opacity(Some(2.5));
        assert_eq!(environment.preferred_passthrough_opacity(), Some(1.0));
        assert_eq!(environment.preference(), Some(1.0));
        environment.set_preferred_passthrough_opacity(Some(-1.0));
        assert_eq!(environment.preferred_passthrough_opacity(), Some(0.0));
        environment.set_preferred_passthrough_opacity(None);
        assert_eq!(environment.preference(), None);
    });
}

#[test]
fn the_headset_tells_what_the_room_shows() {
    cranpose_testing::run_test_composition(|_| {
        let environment = SpatialEnvironment::new();
        environment.set_preferred_passthrough_opacity(Some(1.0));
        assert_eq!(
            environment.current_passthrough_opacity(),
            0.0,
            "the preference alone shows nothing"
        );
        environment.show(1.0);
        assert_eq!(environment.current_passthrough_opacity(), 1.0);
    });
}

#[test]
fn only_a_headset_with_a_passthrough_backend_controls_the_room() {
    let outside = SpatialCapabilities::default();
    assert!(!outside.is_spatial_ui_enabled() && !outside.is_passthrough_control_enabled());
    let fixed = SpatialCapabilities::in_headset(false);
    assert!(fixed.is_spatial_ui_enabled() && !fixed.is_passthrough_control_enabled());
    assert!(SpatialCapabilities::in_headset(true).is_passthrough_control_enabled());
}
