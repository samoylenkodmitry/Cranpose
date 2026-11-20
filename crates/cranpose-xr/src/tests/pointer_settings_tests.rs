use cranpose::PointerButton;

use super::*;
use crate::ControllerInput;

#[test]
fn the_settings_start_with_the_default_mapping_and_the_beams_shown() {
    let settings = PointerSettings::new();
    assert_eq!(settings.mapping(), PointerMapping::default());
    assert!(settings.beams_visible());
}

#[test]
fn a_change_through_one_handle_shows_in_every_clone() {
    let settings = PointerSettings::new();
    let app = settings.clone();
    app.set_mapping(
        PointerMapping::new()
            .with_button(ControllerInput::Squeeze, Some(PointerButton::Secondary))
            .with_button(ControllerInput::Primary, None)
            .with_scroll_speed(-300.0),
    );
    app.set_beams_visible(false);
    let mapping = settings.mapping();
    assert_eq!(
        mapping.button(ControllerInput::Squeeze),
        Some(PointerButton::Secondary)
    );
    assert_eq!(mapping.button(ControllerInput::Primary), None);
    assert_eq!(
        mapping.button(ControllerInput::Trigger),
        Some(PointerButton::Primary)
    );
    assert!((mapping.scroll_speed() + 300.0).abs() < f32::EPSILON);
    assert!(!settings.beams_visible());
    assert_eq!(settings, app);
    assert_ne!(settings, PointerSettings::new());
}

#[test]
fn the_default_mapping_is_a_mouse() {
    let mapping = PointerMapping::default();
    let buttons = ControllerInput::ALL.map(|input| mapping.button(input));
    assert_eq!(
        buttons,
        [
            Some(PointerButton::Primary),
            None,
            Some(PointerButton::Secondary),
            None,
            Some(PointerButton::Middle),
            None,
        ]
    );
    assert!(mapping.scroll_speed() > 0.0);
}

#[test]
fn a_speed_that_is_not_a_number_scrolls_nothing() {
    assert_eq!(
        PointerMapping::new()
            .with_scroll_speed(f32::NAN)
            .scroll_speed(),
        0.0
    );
    assert_eq!(
        PointerMapping::new()
            .with_scroll_speed(f32::INFINITY)
            .scroll_speed(),
        0.0
    );
}
