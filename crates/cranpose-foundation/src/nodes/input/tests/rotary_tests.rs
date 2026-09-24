use super::*;

#[test]
fn new_stores_pixel_amounts_verbatim() {
    let event = RotaryScrollEvent::new(-12.0, 3.5, 4_200);

    assert_eq!(event.vertical_scroll_pixels, -12.0);
    assert_eq!(event.horizontal_scroll_pixels, 3.5);
    assert_eq!(event.uptime_millis, 4_200);
}

#[test]
fn step_accumulator_retains_partial_travel_and_emits_every_crossed_step() {
    let mut steps = RotaryStepAccumulator::new(10.0);
    assert_eq!(steps.accept(6.0), 0);
    assert_eq!(steps.accept(6.0), 1);
    assert_eq!(steps.accept(29.0), 3);
    assert_eq!(steps.accept(-12.0), -1);
}

#[test]
fn step_accumulator_reset_drops_partial_travel() {
    let mut steps = RotaryStepAccumulator::new(10.0);
    assert_eq!(steps.accept(9.0), 0);
    steps.reset();
    assert_eq!(steps.accept(1.0), 0);
}

#[test]
fn positive_detents_become_negative_pixels() {
    assert_eq!(rotary_scroll_pixels_from_detents(1.0, 64.0), -64.0);
    assert_eq!(rotary_scroll_pixels_from_detents(-1.0, 64.0), 64.0);
    assert_eq!(rotary_scroll_pixels_from_detents(0.0, 64.0), 0.0);
}

#[test]
fn from_detents_matches_compose_and_feeds_both_axes() {
    let event = RotaryScrollEvent::from_detents(2.0, 64.0, 48.0, 9);

    assert_eq!(event.vertical_scroll_pixels, -128.0);
    assert_eq!(event.horizontal_scroll_pixels, -96.0);
    assert_eq!(event.uptime_millis, 9);
}

#[test]
fn from_detents_is_sign_symmetric() {
    let up = RotaryScrollEvent::from_detents(1.5, 64.0, 64.0, 0);
    let down = RotaryScrollEvent::from_detents(-1.5, 64.0, 64.0, 0);

    assert_eq!(up.vertical_scroll_pixels, -down.vertical_scroll_pixels);
    assert!(up.vertical_scroll_pixels < 0.0);
    assert!(down.vertical_scroll_pixels > 0.0);
}

#[test]
fn a_wheel_turned_up_lands_where_a_crown_turned_up_does() {
    let wheel = RotaryScrollEvent::from_wheel_pixels(64.0, 0.0, 0);
    let crown = RotaryScrollEvent::from_detents(1.0, 64.0, 64.0, 0);

    assert_eq!(
        wheel.vertical_scroll_pixels, crown.vertical_scroll_pixels,
        "a positive wheel delta and a positive detent are the same physical turn"
    );
    assert!(wheel.vertical_scroll_pixels < 0.0);
}

#[test]
fn a_wheel_carries_each_axis_on_its_own() {
    let event = RotaryScrollEvent::from_wheel_pixels(12.0, -5.0, 77);

    assert_eq!(event.vertical_scroll_pixels, -12.0);
    assert_eq!(event.horizontal_scroll_pixels, 5.0);
    assert_eq!(event.uptime_millis, 77);
    assert!(RotaryScrollEvent::from_wheel_pixels(0.0, 0.0, 1).is_empty());
}

#[test]
fn is_empty_rejects_zero_and_non_finite_amounts() {
    assert!(RotaryScrollEvent::default().is_empty());
    assert!(RotaryScrollEvent::new(0.0, 0.0, 1).is_empty());
    assert!(RotaryScrollEvent::new(f32::NAN, f32::INFINITY, 1).is_empty());

    assert!(!RotaryScrollEvent::new(-1.0, 0.0, 1).is_empty());
    assert!(!RotaryScrollEvent::new(0.0, 2.0, 1).is_empty());
}

#[test]
fn event_is_copy_and_allocation_free() {
    fn assert_copy<T: Copy>() {}
    assert_copy::<RotaryScrollEvent>();
    assert_eq!(
        std::mem::size_of::<RotaryScrollEvent>(),
        std::mem::size_of::<f32>() * 2 + std::mem::size_of::<u64>()
    );
}
