use cranpose_ui_graphics::{DrawPrimitive, DrawScopeDefault, Size};

use super::*;

fn scene(geometry: IndicatorGeometry, alpha: f32) -> Vec<DrawPrimitive> {
    let mut scope = DrawScopeDefault::new(Size::new(227.0, 227.0));
    draw_scroll_indicator(
        &mut scope,
        geometry,
        ScrollIndicatorSpec::default().alpha(alpha),
    );
    scope.into_primitives()
}

#[test]
fn the_indicator_is_three_segments_not_a_thumb_over_a_rail() {
    let primitives = scene(
        IndicatorGeometry {
            thumb: 0.4,
            offset: 0.3,
        },
        1.0,
    );
    assert_eq!(
        primitives.len(),
        3,
        "track, thumb, track — and no full-length rail underneath"
    );
}

#[test]
fn a_segment_shorter_than_its_stroke_becomes_a_dot() {
    let primitives = scene(
        IndicatorGeometry {
            thumb: 0.7,
            offset: 0.0,
        },
        1.0,
    );
    assert_eq!(primitives.len(), 2);
}

#[test]
fn a_faded_indicator_draws_nothing_at_all() {
    let primitives = scene(
        IndicatorGeometry {
            thumb: 0.4,
            offset: 0.3,
        },
        0.0,
    );
    assert!(primitives.is_empty());
}

#[test]
fn a_display_with_no_room_does_not_panic() {
    let mut scope = DrawScopeDefault::new(Size::new(1.0, 1.0));
    draw_scroll_indicator(
        &mut scope,
        IndicatorGeometry {
            thumb: 0.4,
            offset: 0.3,
        },
        ScrollIndicatorSpec::default(),
    );
    assert!(scope.into_primitives().is_empty());
}
