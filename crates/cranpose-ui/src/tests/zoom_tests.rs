use std::{rc::Rc, sync::Arc};

use cranpose_core::{DefaultScheduler, Runtime};
use cranpose_foundation::{
    BasicModifierNodeContext, ModifierNodeChain, PointerButton, PointerButtons, PointerEvent,
    PointerEventKind,
};
use cranpose_ui_graphics::Point;

use crate::{collect_modifier_slices, zoom::ZoomState, Modifier};

fn with_test_runtime<T>(f: impl FnOnce() -> T) -> T {
    let _runtime = Runtime::new(Arc::new(DefaultScheduler));
    f()
}

fn pointer_handler_for(modifier: Modifier) -> (Rc<dyn Fn(PointerEvent)>, ModifierNodeChain) {
    let elements = modifier.elements();
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();
    chain.update_from_slice(&elements, &mut context);
    let slices = collect_modifier_slices(&chain);
    let handler = slices
        .pointer_inputs()
        .first()
        .cloned()
        .expect("zoomable modifier should provide pointer input handler");
    (handler, chain)
}

fn touch(kind: PointerEventKind, id: u64, x: f32, y: f32) -> PointerEvent {
    PointerEvent::new(kind, Point { x, y }, Point { x, y })
        .with_buttons(PointerButtons::new().with(PointerButton::Primary))
        .with_id(id)
}

// ─────────────────────────────────────────────────────────────────────────────
// ZoomState transform math
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn apply_transform_zooms_about_the_centroid() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = ZoomState::with_scale_range(0.5, 8.0);

        // Zoom 2x about (100, 50). The content point that was displayed at
        // (100, 50) must still be displayed at (100, 50) afterwards.
        state.apply_transform(Point { x: 100.0, y: 50.0 }, Point { x: 0.0, y: 0.0 }, 2.0);

        assert_eq!(state.scale_non_reactive(), 2.0);
        let offset = state.offset_non_reactive();
        // Content point c = (100,50) (scale was 1, offset 0). New display
        // position: c * 2 + offset must equal (100, 50).
        assert!(
            (100.0 * 2.0 + offset.x - 100.0).abs() < 1e-4
                && (50.0 * 2.0 + offset.y - 50.0).abs() < 1e-4,
            "focal point must stay fixed, offset={offset:?}"
        );
    });
}

#[test]
fn apply_transform_focal_point_stays_fixed_across_steps() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = ZoomState::with_scale_range(0.5, 8.0);
        let focal = Point { x: 40.0, y: 200.0 };

        // The content point rendered at `focal` before the gesture:
        let content = focal; // identity transform
        for zoom_step in [1.3f32, 1.3, 0.9, 1.1] {
            state.apply_transform(focal, Point { x: 0.0, y: 0.0 }, zoom_step);
            let scale = state.scale_non_reactive();
            let offset = state.offset_non_reactive();
            let display = Point {
                x: content.x * scale + offset.x,
                y: content.y * scale + offset.y,
            };
            assert!(
                (display.x - focal.x).abs() < 1e-3 && (display.y - focal.y).abs() < 1e-3,
                "focal content point drifted to {display:?} at scale {scale}"
            );
        }
    });
}

#[test]
fn apply_transform_clamps_scale_to_range() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = ZoomState::with_scale_range(1.0, 4.0);

        state.apply_transform(Point { x: 0.0, y: 0.0 }, Point { x: 0.0, y: 0.0 }, 100.0);
        assert_eq!(state.scale_non_reactive(), 4.0);

        state.apply_transform(Point { x: 0.0, y: 0.0 }, Point { x: 0.0, y: 0.0 }, 0.0001);
        assert_eq!(state.scale_non_reactive(), 1.0);
    });
}

#[test]
fn apply_transform_pans_by_the_gesture_delta() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = ZoomState::with_scale_range(0.5, 8.0);
        state.set_scale(2.0);

        state.apply_transform(Point { x: 0.0, y: 0.0 }, Point { x: 15.0, y: -7.0 }, 1.0);

        assert_eq!(state.offset_non_reactive(), Point { x: 15.0, y: -7.0 });
        assert_eq!(state.scale_non_reactive(), 2.0);
    });
}

#[test]
fn reset_restores_identity() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = ZoomState::new();
        state.apply_transform(Point { x: 10.0, y: 10.0 }, Point { x: 5.0, y: 5.0 }, 3.0);
        assert!(state.is_transformed());

        state.reset();
        assert!(!state.is_transformed());
        assert_eq!(state.scale_non_reactive(), 1.0);
        assert_eq!(state.offset_non_reactive(), Point { x: 0.0, y: 0.0 });
    });
}

#[test]
fn layer_reflects_scale_offset_with_top_left_origin() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = ZoomState::new();
        state.set_scale(2.5);
        state.set_offset(Point { x: -30.0, y: 12.0 });

        let layer = state.layer();
        assert_eq!(layer.scale_x, 2.5);
        assert_eq!(layer.scale_y, 2.5);
        assert_eq!(layer.translation_x, -30.0);
        assert_eq!(layer.translation_y, 12.0);
        assert_eq!(layer.transform_origin.pivot_fraction_x, 0.0);
        assert_eq!(layer.transform_origin.pivot_fraction_y, 0.0);
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// zoomable modifier gesture handling
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn two_finger_pinch_out_scales_up_and_consumes_moves() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = ZoomState::new();
        let (handler, _chain) = pointer_handler_for(Modifier::empty().zoomable(state));

        handler(touch(PointerEventKind::Down, 0, 100.0, 100.0));
        handler(touch(PointerEventKind::Down, 1, 200.0, 100.0));

        // Spread the second finger out: distance 100 -> 200.
        let pinch_move = touch(PointerEventKind::Move, 1, 300.0, 100.0);
        handler(pinch_move.clone());

        assert!(
            (state.scale_non_reactive() - 2.0).abs() < 1e-4,
            "doubling the spread must double the scale, got {}",
            state.scale_non_reactive()
        );
        assert!(
            pinch_move.is_consumed(),
            "active pinch must consume move events"
        );

        handler(touch(PointerEventKind::Up, 1, 300.0, 100.0));
        handler(touch(PointerEventKind::Up, 0, 100.0, 100.0));
    });
}

#[test]
fn pinch_keeps_content_under_the_fingers() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = ZoomState::with_scale_range(1.0, 8.0);
        let (handler, _chain) = pointer_handler_for(Modifier::empty().zoomable(state));

        handler(touch(PointerEventKind::Down, 0, 100.0, 100.0));
        handler(touch(PointerEventKind::Down, 1, 200.0, 100.0));
        // Symmetric pinch out about (150, 100).
        handler(touch(PointerEventKind::Move, 0, 50.0, 100.0));
        handler(touch(PointerEventKind::Move, 1, 250.0, 100.0));

        let scale = state.scale_non_reactive();
        let offset = state.offset_non_reactive();
        // The content point that started under the pinch center (150, 100)
        // must still render at the pinch center.
        let display_x = 150.0 * scale + offset.x;
        let display_y = 100.0 * scale + offset.y;
        assert!(
            (display_x - 150.0).abs() < 1.0 && (display_y - 100.0).abs() < 1.0,
            "pinch center drifted: ({display_x}, {display_y}) at scale {scale}"
        );
        assert!(scale > 1.5, "symmetric spread must zoom in, got {scale}");
    });
}

#[test]
fn secondary_pointer_down_and_up_are_consumed() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = ZoomState::new();
        let (handler, _chain) = pointer_handler_for(Modifier::empty().zoomable(state));

        handler(touch(PointerEventKind::Down, 0, 100.0, 100.0));
        let secondary_down = touch(PointerEventKind::Down, 1, 200.0, 100.0);
        handler(secondary_down.clone());
        assert!(
            secondary_down.is_consumed(),
            "secondary finger down must not leak to single-pointer handlers"
        );

        let secondary_up = touch(PointerEventKind::Up, 1, 200.0, 100.0);
        handler(secondary_up.clone());
        assert!(
            secondary_up.is_consumed(),
            "secondary finger up must not leak to single-pointer handlers"
        );
    });
}

#[test]
fn single_finger_drag_pans_only_when_transformed() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = ZoomState::new();
        let (handler, _chain) = pointer_handler_for(Modifier::empty().zoomable(state));

        // At identity: a one-finger drag must NOT pan (enclosing scrollables
        // own plain drags).
        handler(touch(PointerEventKind::Down, 0, 100.0, 100.0));
        let plain_drag = touch(PointerEventKind::Move, 0, 100.0, 160.0);
        handler(plain_drag.clone());
        handler(touch(PointerEventKind::Up, 0, 100.0, 160.0));
        assert_eq!(state.offset_non_reactive(), Point { x: 0.0, y: 0.0 });
        assert!(
            !plain_drag.is_consumed(),
            "identity zoomable must not steal plain drags"
        );

        // Zoomed in: the same drag pans the content.
        state.set_scale(2.0);
        handler(touch(PointerEventKind::Down, 0, 100.0, 100.0));
        handler(touch(PointerEventKind::Move, 0, 100.0, 120.0)); // crosses slop
        handler(touch(PointerEventKind::Move, 0, 100.0, 160.0));
        handler(touch(PointerEventKind::Up, 0, 100.0, 160.0));

        let offset = state.offset_non_reactive();
        assert!(
            offset.y > 30.0,
            "zoomed content must pan with the finger, got {offset:?}"
        );
    });
}

#[test]
fn two_finger_centroid_pan_at_identity_scale_is_inert() {
    // Two fingers moving in parallel are a net centroid pan. While the
    // content is not zoomed in, that pan must not displace it. The fingers
    // move one event at a time (like real dispatch), so the range allows
    // sub-1 scales to keep the shrink/grow spread steps symmetric.
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = ZoomState::with_scale_range(0.5, 8.0);
        let (handler, _chain) = pointer_handler_for(Modifier::empty().zoomable(state));

        handler(touch(PointerEventKind::Down, 0, 100.0, 100.0));
        handler(touch(PointerEventKind::Down, 1, 200.0, 100.0));
        // Both fingers translate by (+40, +25): spread unchanged overall.
        handler(touch(PointerEventKind::Move, 0, 140.0, 125.0));
        handler(touch(PointerEventKind::Move, 1, 240.0, 125.0));
        handler(touch(PointerEventKind::Up, 1, 240.0, 125.0));
        handler(touch(PointerEventKind::Up, 0, 140.0, 125.0));

        assert!(
            (state.scale_non_reactive() - 1.0).abs() < 1e-4,
            "a parallel two-finger pan must not change the scale, got {}",
            state.scale_non_reactive()
        );
        assert_eq!(
            state.offset_non_reactive(),
            Point { x: 0.0, y: 0.0 },
            "centroid pan at identity scale must be inert"
        );
    });
}

#[test]
fn released_pinch_that_zoomed_back_out_leaves_no_offset() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = ZoomState::new();
        let (handler, _chain) = pointer_handler_for(Modifier::empty().zoomable(state));

        handler(touch(PointerEventKind::Down, 0, 100.0, 100.0));
        handler(touch(PointerEventKind::Down, 1, 200.0, 100.0));
        // Pinch out: spread 100 -> 200 (scale 2), off-center so the focal
        // anchoring produces a non-zero offset.
        handler(touch(PointerEventKind::Move, 1, 300.0, 100.0));
        assert!(state.is_zoomed_in(), "pinch out must zoom in");
        // Drag both fingers (centroid pan while zoomed): offset changes.
        handler(touch(PointerEventKind::Move, 0, 130.0, 140.0));
        handler(touch(PointerEventKind::Move, 1, 330.0, 140.0));
        // Pinch back in past identity: spread 200 -> 80 (scale clamps to 1).
        handler(touch(PointerEventKind::Move, 1, 210.0, 140.0));
        handler(touch(PointerEventKind::Up, 1, 210.0, 140.0));
        handler(touch(PointerEventKind::Up, 0, 130.0, 140.0));

        assert_eq!(state.scale_non_reactive(), 1.0);
        assert_eq!(
            state.offset_non_reactive(),
            Point { x: 0.0, y: 0.0 },
            "a released pinch that zoomed back out must not leave a stray offset"
        );
        assert!(!state.is_transformed());
    });
}

#[test]
fn one_finger_drag_does_not_pan_offset_only_state() {
    // Pan is gated on scale > 1, not on "any transform": a programmatic
    // offset at identity scale must not turn drags into pans.
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = ZoomState::new();
        let (handler, _chain) = pointer_handler_for(Modifier::empty().zoomable(state));
        state.set_offset(Point { x: 5.0, y: 5.0 });

        handler(touch(PointerEventKind::Down, 0, 100.0, 100.0));
        let drag = touch(PointerEventKind::Move, 0, 100.0, 160.0);
        handler(drag.clone());
        handler(touch(PointerEventKind::Up, 0, 100.0, 160.0));

        assert_eq!(state.offset_non_reactive(), Point { x: 5.0, y: 5.0 });
        assert!(
            !drag.is_consumed(),
            "an unzoomed zoomable must not steal drags even with a programmatic offset"
        );
    });
}

fn timed_touch(kind: PointerEventKind, x: f32, y: f32, time_ms: i64) -> PointerEvent {
    touch(kind, 0, x, y).with_time_ms(Some(time_ms))
}

#[test]
fn double_tap_resets_transformed_state() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = ZoomState::new();
        let (handler, _chain) = pointer_handler_for(Modifier::empty().zoomable(state));
        state.set_scale(2.5);
        state.set_offset(Point { x: -30.0, y: 12.0 });

        handler(timed_touch(PointerEventKind::Down, 100.0, 100.0, 0));
        handler(timed_touch(PointerEventKind::Up, 100.0, 100.0, 50));
        handler(timed_touch(PointerEventKind::Down, 103.0, 98.0, 200));
        let second_up = timed_touch(PointerEventKind::Up, 103.0, 98.0, 250);
        handler(second_up.clone());

        assert_eq!(state.scale_non_reactive(), 1.0);
        assert_eq!(state.offset_non_reactive(), Point { x: 0.0, y: 0.0 });
        assert!(
            !state.is_transformed(),
            "double tap must reset the transform"
        );
        assert!(
            second_up.is_consumed(),
            "the resetting tap must not leak to click handlers"
        );
    });
}

#[test]
fn slow_second_tap_does_not_reset() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = ZoomState::new();
        let (handler, _chain) = pointer_handler_for(Modifier::empty().zoomable(state));
        state.set_scale(2.0);

        handler(timed_touch(PointerEventKind::Down, 100.0, 100.0, 0));
        handler(timed_touch(PointerEventKind::Up, 100.0, 100.0, 50));
        // Second tap lands after the double-tap timeout.
        handler(timed_touch(PointerEventKind::Down, 100.0, 100.0, 500));
        handler(timed_touch(PointerEventKind::Up, 100.0, 100.0, 550));

        assert_eq!(
            state.scale_non_reactive(),
            2.0,
            "taps slower than the double-tap timeout must not reset"
        );
    });
}

#[test]
fn double_tap_at_identity_is_not_consumed() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = ZoomState::new();
        let (handler, _chain) = pointer_handler_for(Modifier::empty().zoomable(state));

        handler(timed_touch(PointerEventKind::Down, 100.0, 100.0, 0));
        handler(timed_touch(PointerEventKind::Up, 100.0, 100.0, 50));
        handler(timed_touch(PointerEventKind::Down, 100.0, 100.0, 200));
        let second_up = timed_touch(PointerEventKind::Up, 100.0, 100.0, 250);
        handler(second_up.clone());

        assert!(
            !second_up.is_consumed(),
            "a double tap with nothing to reset must stay visible to click handlers"
        );
    });
}

#[test]
fn zoom_events_scale_about_the_cursor() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let state = ZoomState::with_scale_range(1.0, 8.0);
        let (handler, _chain) = pointer_handler_for(Modifier::empty().zoomable(state));

        let cursor = Point { x: 80.0, y: 60.0 };
        let zoom_event =
            PointerEvent::new(PointerEventKind::Zoom, cursor, cursor).with_zoom_delta(1.5);
        handler(zoom_event.clone());

        assert!(
            (state.scale_non_reactive() - 1.5).abs() < 1e-4,
            "ctrl+wheel step must multiply the scale, got {}",
            state.scale_non_reactive()
        );
        assert!(zoom_event.is_consumed(), "zoom steps must be consumed");

        let offset = state.offset_non_reactive();
        let display_x = cursor.x * 1.5 + offset.x;
        let display_y = cursor.y * 1.5 + offset.y;
        assert!(
            (display_x - cursor.x).abs() < 1e-3 && (display_y - cursor.y).abs() < 1e-3,
            "wheel zoom must anchor at the cursor, got ({display_x}, {display_y})"
        );
    });
}

#[test]
fn scroll_ignores_secondary_pointer_events() {
    // A second finger joining mid-drag must not jerk an enclosing scroll:
    // the scroll detector tracks the primary pointer only.
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let scroll_state = crate::ScrollState::new(100.0);
        scroll_state.set_max_value(400.0);
        let (handler, _chain) =
            pointer_handler_for(Modifier::empty().vertical_scroll(scroll_state, false));

        handler(touch(PointerEventKind::Down, 0, 0.0, 100.0));
        handler(touch(PointerEventKind::Move, 0, 0.0, 80.0)); // drag starts
        let before = scroll_state.value_non_reactive();

        // Secondary finger lands far away and moves wildly.
        handler(touch(PointerEventKind::Down, 1, 0.0, 300.0));
        handler(touch(PointerEventKind::Move, 1, 0.0, 500.0));
        handler(touch(PointerEventKind::Up, 1, 0.0, 500.0));

        assert_eq!(
            scroll_state.value_non_reactive(),
            before,
            "secondary pointer events must not scroll"
        );

        // The primary drag continues smoothly from its own last position.
        handler(touch(PointerEventKind::Move, 0, 0.0, 60.0));
        assert!(
            (scroll_state.value_non_reactive() - (before + 20.0)).abs() < 1e-3,
            "primary drag must continue from its own last position"
        );
        handler(touch(PointerEventKind::Up, 0, 0.0, 60.0));
    });
}
