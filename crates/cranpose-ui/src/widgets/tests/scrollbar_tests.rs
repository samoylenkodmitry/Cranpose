use std::sync::Arc;

use cranpose_core::{DefaultScheduler, Runtime};
use cranpose_ui_graphics::{DrawPrimitive, DrawScopeDefault};

use super::*;

fn scrollable_state(viewport: f32, content: f32, offset: f32) -> ScrollState {
    let state = ScrollState::new(0.0);
    state.set_viewport_extent(viewport);
    state.set_max_value((content - viewport).max(0.0));
    state.scroll_to(offset);
    state
}

fn scene(size: Size, state: ScrollState, axis: Axis, spec: ScrollbarSpec) -> Vec<DrawPrimitive> {
    let mut scope = DrawScopeDefault::new(size);
    draw_scrollbar(&mut scope, state, axis, spec, false);
    scope.into_primitives()
}

fn rects(primitives: &[DrawPrimitive]) -> Vec<Rect> {
    primitives
        .iter()
        .filter_map(|primitive| match primitive {
            DrawPrimitive::Rect { rect, .. } => Some(*rect),
            DrawPrimitive::RoundRect { rect, .. } => Some(*rect),
            _ => None,
        })
        .collect()
}

#[test]
fn colors_answer_for_the_current_interaction() {
    let colors = ScrollbarColors::default();
    assert_eq!(colors.thumb_for(false), colors.thumb);
    assert_eq!(colors.thumb_for(true), colors.dragged_thumb);
}

#[test]
fn a_bar_is_a_pill_unless_it_was_asked_for_something_squarer() {
    let spec = ScrollbarSpec::default();
    assert_eq!(spec.resolved_corner_radius(8.0), 4.0);
    assert_eq!(spec.corner_radius(0.0).resolved_corner_radius(8.0), 0.0);
    assert_eq!(spec.corner_radius(-3.0).resolved_corner_radius(8.0), 0.0);
}

#[test]
fn spec_builders_clamp_to_drawable_values() {
    let spec = ScrollbarSpec::default()
        .thickness(-4.0)
        .min_thumb_extent(-1.0)
        .hide_when_content_fits(false);
    assert_eq!(spec.thickness, 0.0);
    assert_eq!(spec.min_thumb_extent, 0.0);
    assert!(!spec.hide_when_content_fits);
}

#[test]
fn content_that_fits_draws_nothing_at_all() {
    let _runtime = Runtime::new(Arc::new(DefaultScheduler));
    let _app_context = crate::render_state::app_context_test_scope();
    let state = scrollable_state(200.0, 200.0, 0.0);
    let primitives = scene(
        Size::new(8.0, 200.0),
        state,
        Axis::Vertical,
        ScrollbarSpec::default(),
    );
    assert!(primitives.is_empty());
}

#[test]
fn content_that_fits_still_draws_its_track_when_the_bar_is_pinned_visible() {
    let _runtime = Runtime::new(Arc::new(DefaultScheduler));
    let _app_context = crate::render_state::app_context_test_scope();
    let state = scrollable_state(200.0, 200.0, 0.0);
    let spec = ScrollbarSpec::default().hide_when_content_fits(false);
    let primitives = scene(Size::new(8.0, 200.0), state, Axis::Vertical, spec);
    assert_eq!(
        rects(&primitives),
        vec![Rect::from_size(Size::new(8.0, 200.0))]
    );
}

#[test]
fn the_thumb_is_the_share_of_content_on_screen_and_moves_with_it() {
    let _runtime = Runtime::new(Arc::new(DefaultScheduler));
    let _app_context = crate::render_state::app_context_test_scope();
    let spec = ScrollbarSpec::default().min_thumb_extent(0.0);

    let top = scrollable_state(200.0, 800.0, 0.0);
    let drawn = rects(&scene(Size::new(8.0, 200.0), top, Axis::Vertical, spec));
    assert_eq!(drawn.len(), 2, "a track and a thumb");
    assert_eq!(
        drawn[1],
        Rect::from_origin_size(Point::new(0.0, 0.0), Size::new(8.0, 50.0))
    );

    let bottom = scrollable_state(200.0, 800.0, 600.0);
    let drawn = rects(&scene(Size::new(8.0, 200.0), bottom, Axis::Vertical, spec));
    assert_eq!(
        drawn[1],
        Rect::from_origin_size(Point::new(0.0, 150.0), Size::new(8.0, 50.0))
    );
}

#[test]
fn a_horizontal_bar_lays_its_thumb_along_the_other_axis() {
    let _runtime = Runtime::new(Arc::new(DefaultScheduler));
    let _app_context = crate::render_state::app_context_test_scope();
    let spec = ScrollbarSpec::default().min_thumb_extent(0.0);
    let state = scrollable_state(200.0, 800.0, 600.0);
    let drawn = rects(&scene(Size::new(200.0, 8.0), state, Axis::Horizontal, spec));
    assert_eq!(
        drawn[1],
        Rect::from_origin_size(Point::new(150.0, 0.0), Size::new(50.0, 8.0))
    );
}

#[test]
fn a_very_long_document_keeps_a_thumb_big_enough_to_grab() {
    let _runtime = Runtime::new(Arc::new(DefaultScheduler));
    let _app_context = crate::render_state::app_context_test_scope();
    let state = scrollable_state(200.0, 200_000.0, 0.0);
    let drawn = rects(&scene(
        Size::new(8.0, 200.0),
        state,
        Axis::Vertical,
        ScrollbarSpec::default(),
    ));
    assert_eq!(drawn[1].height, DEFAULT_MIN_THUMB_EXTENT);
}

#[test]
fn a_bar_with_no_room_draws_nothing() {
    let _runtime = Runtime::new(Arc::new(DefaultScheduler));
    let _app_context = crate::render_state::app_context_test_scope();
    let state = scrollable_state(200.0, 800.0, 0.0);
    assert!(
        scene(
            Size::new(0.0, 200.0),
            state,
            Axis::Vertical,
            ScrollbarSpec::default()
        )
        .is_empty()
    );
    assert!(
        scene(
            Size::new(8.0, 0.0),
            state,
            Axis::Vertical,
            ScrollbarSpec::default()
        )
        .is_empty()
    );
}

#[test]
fn a_specs_thumb_bounds_state_its_minimum_as_a_track_fraction() {
    let spec = ScrollbarSpec::default();
    let bounds = spec.thumb_bounds(240.0);
    assert!(
        (bounds.minimum() - DEFAULT_MIN_THUMB_EXTENT / 240.0).abs() < 1.0e-6,
        "a 24dp floor on a 240dp track is a tenth of it"
    );
    assert_eq!(bounds.maximum(), 1.0, "a thumb may still fill its track");
}

#[test]
fn a_thumb_floor_taller_than_its_track_asks_for_the_whole_track() {
    let bounds = ScrollbarSpec::default().thumb_bounds(10.0);
    assert_eq!(
        bounds.minimum(),
        1.0,
        "a 24dp floor cannot fit in 10dp, so the thumb takes everything"
    );
    assert_eq!(bounds.maximum(), 1.0);
}

#[test]
fn a_track_with_no_extent_leaves_the_thumb_unbounded() {
    for track in [0.0_f32, -40.0, f32::NAN, f32::INFINITY] {
        let bounds = ScrollbarSpec::default().thumb_bounds(track);
        assert_eq!(
            bounds.minimum(),
            0.0,
            "track {track} must not floor a thumb"
        );
        assert_eq!(bounds.maximum(), 1.0);
    }
}

#[test]
fn resolved_corner_radius_is_a_pill_until_a_caller_squares_it() {
    let spec = ScrollbarSpec::default();
    assert_eq!(
        spec.resolved_corner_radius(8.0),
        4.0,
        "half the thickness reads as a pill"
    );
    assert_eq!(
        ScrollbarSpec::default()
            .corner_radius(0.0)
            .resolved_corner_radius(8.0),
        0.0,
        "a caller asking for square corners gets them"
    );
    assert_eq!(
        ScrollbarSpec::default()
            .corner_radius(-5.0)
            .resolved_corner_radius(8.0),
        0.0,
        "a negative radius is not a shape"
    );
}
