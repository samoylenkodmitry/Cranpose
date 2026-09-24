use super::*;

#[test]
fn spec_builders_define_orientation_and_input_policy() {
    let spec = SliderSpec::new()
        .orientation(SliderOrientation::Vertical)
        .reverse_direction(true)
        .thumb_extent(11.0)
        .enabled(false)
        .rotary_step(-0.2);
    assert_eq!(spec.orientation, SliderOrientation::Vertical);
    assert!(spec.reverse_direction);
    assert_eq!(spec.thumb_extent, 11.0);
    assert!(!spec.enabled);
    assert_eq!(spec.rotary_step, 0.2);
}

#[test]
fn pointer_position_tracks_thumb_centre_and_reverse_direction() {
    assert_eq!(value_for_position(5.0, 110.0, 10.0, false), 0.0);
    assert_eq!(value_for_position(105.0, 110.0, 10.0, false), 1.0);
    assert_eq!(value_for_position(55.0, 110.0, 10.0, false), 0.5);
    assert_eq!(value_for_position(5.0, 110.0, 10.0, true), 1.0);
}

#[test]
fn zero_travel_is_stable() {
    assert_eq!(value_for_position(0.0, 0.0, 0.0, false), 0.0);
    assert_eq!(value_for_position(0.0, 0.0, 0.0, true), 1.0);
}
