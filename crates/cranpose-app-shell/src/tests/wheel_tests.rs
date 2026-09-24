use super::*;

fn wheel(x: f32, y: f32) -> WheelScroll {
    WheelScroll::new(Point { x, y }, 0)
}

#[test]
fn a_plain_sample_is_neither_a_zoom_nor_axis_swapped() {
    let sample = wheel(3.0, -40.0);

    assert!(!sample.is_zoom());
    assert_eq!(sample.scroll_delta(), sample.delta);
}

#[test]
fn ctrl_makes_a_sample_a_zoom_that_grows_when_the_wheel_turns_up() {
    let up = wheel(0.0, NOTCH_LOGICAL_PX).with_modifiers(Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    });
    let down = wheel(0.0, -NOTCH_LOGICAL_PX).with_modifiers(Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    });

    assert!(up.is_zoom());
    assert!((up.zoom_factor() - ZOOM_PER_NOTCH).abs() < 1.0e-6);
    assert!((up.zoom_factor() * down.zoom_factor() - 1.0).abs() < 1.0e-6);
}

#[test]
fn alt_moves_a_vertical_wheel_onto_the_horizontal_axis() {
    let alt = Modifiers {
        alt: true,
        ..Modifiers::NONE
    };

    assert_eq!(
        wheel(0.0, 48.0).with_modifiers(alt).scroll_delta(),
        Point { x: 48.0, y: 0.0 }
    );
    assert_eq!(
        wheel(12.0, 48.0).with_modifiers(alt).scroll_delta(),
        Point { x: 12.0, y: 0.0 }
    );
}
