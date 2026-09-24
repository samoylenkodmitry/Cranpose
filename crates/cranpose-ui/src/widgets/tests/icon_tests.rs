use super::*;

#[test]
fn icon_path_is_parseable() {
    assert!(VectorPath::parse("M0 0h24v24H0z").is_ok());
}

#[test]
fn an_icon_button_is_never_smaller_than_the_minimum_touch_target() {
    let spec = IconButtonSpec::default().with_touch_target(20.0);
    assert_eq!(spec.resolved_touch_target(), MINIMUM_TOUCH_TARGET);
    let roomy = IconButtonSpec::default().with_touch_target(64.0);
    assert_eq!(roomy.resolved_touch_target(), 64.0);
}

#[test]
fn colours_follow_the_state_and_fall_back_to_the_resting_surface() {
    let rest = Color(0.1, 0.1, 0.1, 1.0);
    let held = Color(0.2, 0.2, 0.2, 1.0);
    let off = Color(0.3, 0.3, 0.3, 1.0);

    let full = IconButtonColors::default()
        .with_background(rest)
        .with_pressed_background(held)
        .with_disabled_background(off);
    assert_eq!(full.surface(true, false), Some(rest));
    assert_eq!(full.surface(true, true), Some(held));
    assert_eq!(full.surface(false, false), Some(off));

    let plain = IconButtonColors::default().with_background(rest);
    assert_eq!(plain.surface(true, true), Some(rest));
    assert_eq!(plain.surface(false, true), Some(rest));

    assert_eq!(IconButtonColors::default().surface(true, true), None);
}

#[test]
fn an_icon_states_its_size_and_tint() {
    assert_eq!(IconSpec::default().size, DEFAULT_ICON_SIZE);
    assert_eq!(IconSpec::default().tint, None);
    let spec = IconSpec::sized(16.0).with_tint(Color(1.0, 0.0, 0.0, 1.0));
    assert_eq!(spec.size, 16.0);
    assert_eq!(spec.tint, Some(Color(1.0, 0.0, 0.0, 1.0)));
    assert_eq!(spec.with_size(32.0).size, 32.0);
}

#[test]
fn a_disabled_icon_button_still_publishes_itself() {
    let spec = IconButtonSpec::default().with_enabled(false);
    assert!(!spec.enabled);
    assert_eq!(spec.resolved_touch_target(), MINIMUM_TOUCH_TARGET);
}
