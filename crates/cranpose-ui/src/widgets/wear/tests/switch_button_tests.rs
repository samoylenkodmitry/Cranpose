use cranpose_ui_graphics::{DrawScopeDefault, Size as GraphicsSize};

use super::*;

fn colors() -> WearColors {
    WearColors {
        primary: Color::from_rgb_u8(0xB9, 0xF2, 0xFF),
        primary_container: Color::from_rgb_u8(0x0F, 0x36, 0x4E),
        surface_container: Color::from_rgb_u8(0x0A, 0x16, 0x22),
        outline: Color::from_rgb_u8(0x1D, 0x4D, 0x69),
        ..WearColors::default()
    }
}

#[test]
fn the_defaults_are_the_ones_the_tokens_declare() {
    let spec = SwitchButtonSpec::default();
    assert_eq!(spec.min_height, 52.0);
    assert_eq!(spec.corner_radius, 26.0);
    assert_eq!(spec.padding_horizontal, 14.0);
    assert_eq!(spec.padding_vertical, 8.0);
    assert_eq!(spec.switch_width, 32.0);
    assert_eq!(spec.switch_slot_height, 24.0);
    assert_eq!(spec.switch_height, 22.0);
    assert_eq!(spec.track_border_width, 2.0);
    assert_eq!(spec.thumb_radius_unchecked, 6.0);
    assert_eq!(spec.thumb_radius_checked, 9.0);
    assert_eq!(spec.control_spacing, 6.0);
}

#[test]
fn the_thumb_travels_between_the_two_measured_positions() {
    let spec = SwitchButtonSpec::default();
    let (unchecked, radius_off) = switch_thumb(spec, 0.0);
    assert_eq!(unchecked, 11.0, "22px at density 2");
    assert_eq!(radius_off, 6.0);
    let (checked, radius_on) = switch_thumb(spec, 1.0);
    assert_eq!(checked, 21.0, "42px at density 2");
    assert_eq!(radius_on, 9.0);
}

#[test]
fn a_checked_switch_draws_no_border_and_an_unchecked_one_does() {
    let checked = SwitchColors::of(colors(), true);
    assert_eq!(
        checked.track_border.3, 0.0,
        "track and border are both primary, so the border is suppressed"
    );
    assert_eq!(checked.track, colors().primary);
    assert_eq!(checked.thumb, colors().primary_container);

    let unchecked = SwitchColors::of(colors(), false);
    assert!(unchecked.track_border.3 > 0.0);
    assert_eq!(unchecked.track_border, colors().outline);
}

#[test]
fn a_checked_thumb_is_the_container_colour_and_still_gets_drawn() {
    let scheme = colors();
    let checked = SwitchColors::of(scheme, true);
    assert_eq!(
        checked.thumb, checked.container,
        "invisible against the row, and still occluding the track"
    );
    let mut scope = DrawScopeDefault::new(GraphicsSize::new(32.0, 22.0));
    draw_switch(
        &mut scope,
        SwitchButtonSpec::default().colors(scheme).progress(1.0),
        checked,
    );
    let primitives = scope.into_primitives();
    assert_eq!(primitives.len(), 8);
}

#[test]
fn an_unchecked_switch_draws_its_ring_and_no_tick() {
    let mut scope = DrawScopeDefault::new(GraphicsSize::new(32.0, 22.0));
    draw_switch(
        &mut scope,
        SwitchButtonSpec::default().colors(colors()).progress(0.0),
        SwitchColors::of(colors(), false),
    );
    assert_eq!(scope.into_primitives().len(), 3);
}

#[test]
fn the_tick_eases_out_rather_than_growing_linearly() {
    assert_eq!(switch_tick_scale(0.0), 0.0);
    assert_eq!(switch_tick_scale(1.0), 1.0);
    assert!(switch_tick_scale(0.5) > 0.5);
    assert!((switch_tick_scale(0.5) - 0.875).abs() < 1e-6);
}

#[test]
fn the_springs_are_the_ones_the_standard_motion_scheme_declares() {
    assert_eq!(SWITCH_THUMB_STIFFNESS, 1400.0);
    assert_eq!(SWITCH_COLOR_STIFFNESS, 260.0);
    assert_eq!(SWITCH_DAMPING_RATIO, 1.0);
}
