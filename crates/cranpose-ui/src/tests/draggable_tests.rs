//! `Modifier::draggable` driven through real pointer events.
//!
//! The unit tests beside `DraggableState` cover the state; these cover the
//! gesture around it — that a drag has to clear the touch slop, that it follows
//! the finger sign-for-sign (unlike a scroll, where content moves against the
//! offset), that a cross-axis drag never reaches it, that an event another
//! modifier already consumed ends the gesture, and that a release places the
//! control rather than flinging it onwards.

use crate::draggable::DraggableState;
use crate::{collect_modifier_slices, Modifier};
use cranpose_core::{DefaultScheduler, Runtime};
use cranpose_foundation::{
    BasicModifierNodeContext, ModifierNodeChain, PointerButton, PointerButtons, PointerEvent,
    PointerEventKind, DRAG_THRESHOLD,
};
use cranpose_ui_graphics::Point;
use cranpose_ui_layout::Axis;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

fn with_test_runtime<T>(body: impl FnOnce() -> T) -> T {
    let _runtime = Runtime::new(Arc::new(DefaultScheduler));
    body()
}

/// Returns (handler, chain). The chain must outlive the handler: dropping it
/// cancels the pointer input task the gesture runs in.
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
        .expect("draggable modifier should provide a pointer input handler");
    (handler, chain)
}

fn event(kind: PointerEventKind, x: f32, y: f32) -> PointerEvent {
    PointerEvent::new(kind, Point { x, y }, Point { x, y })
        .with_buttons(PointerButtons::new().with(PointerButton::Primary))
}

/// A drag state that records everything it is told, and the total.
fn recording_state() -> (DraggableState, Rc<Cell<f32>>) {
    let total = Rc::new(Cell::new(0.0));
    let recorder = Rc::clone(&total);
    let state = DraggableState::new(move |delta| recorder.set(recorder.get() + delta));
    (state, total)
}

#[test]
fn a_vertical_drag_follows_the_finger() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let (state, total) = recording_state();
        let (handler, _chain) =
            pointer_handler_for(Modifier::empty().draggable(Axis::Vertical, state.clone()));

        handler(event(PointerEventKind::Down, 0.0, 100.0));
        handler(event(PointerEventKind::Move, 0.0, 160.0));
        handler(event(PointerEventKind::Up, 0.0, 160.0));

        // Down the screen is a positive delta, unlike a scroll offset, which
        // moves against the finger.
        assert!(
            (total.get() - 60.0).abs() < 0.001,
            "a finger moved 60 down must report +60, got {}",
            total.get()
        );
        assert_eq!(state.offset(), total.get());
    });
}

#[test]
fn a_horizontal_drag_reports_the_other_axis() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let (state, total) = recording_state();
        let (handler, _chain) =
            pointer_handler_for(Modifier::empty().draggable(Axis::Horizontal, state));

        handler(event(PointerEventKind::Down, 100.0, 0.0));
        handler(event(PointerEventKind::Move, 40.0, 0.0));
        handler(event(PointerEventKind::Up, 40.0, 0.0));

        assert!(
            (total.get() + 60.0).abs() < 0.001,
            "a finger moved 60 left must report -60, got {}",
            total.get()
        );
    });
}

#[test]
fn a_movement_inside_the_touch_slop_is_not_a_drag() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let (state, total) = recording_state();
        let (handler, _chain) =
            pointer_handler_for(Modifier::empty().draggable(Axis::Vertical, state.clone()));

        handler(event(PointerEventKind::Down, 0.0, 100.0));
        handler(event(
            PointerEventKind::Move,
            0.0,
            100.0 + DRAG_THRESHOLD - 1.0,
        ));

        assert_eq!(total.get(), 0.0, "a finger inside the slop has not dragged");
        assert!(!state.is_dragging());
    });
}

#[test]
fn a_drag_along_the_other_axis_never_reaches_this_one() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let (state, total) = recording_state();
        let (handler, _chain) =
            pointer_handler_for(Modifier::empty().draggable(Axis::Vertical, state));

        handler(event(PointerEventKind::Down, 100.0, 100.0));
        // Decisively horizontal: this detector locks out for the gesture.
        handler(event(PointerEventKind::Move, 200.0, 104.0));
        handler(event(PointerEventKind::Move, 240.0, 180.0));

        assert_eq!(
            total.get(),
            0.0,
            "a vertical draggable must not steal a horizontal gesture"
        );
    });
}

#[test]
fn an_event_another_modifier_consumed_ends_the_drag() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let (state, total) = recording_state();
        let (handler, _chain) =
            pointer_handler_for(Modifier::empty().draggable(Axis::Vertical, state.clone()));

        handler(event(PointerEventKind::Down, 0.0, 100.0));
        handler(event(PointerEventKind::Move, 0.0, 160.0));
        assert!((total.get() - 60.0).abs() < 0.001);

        let claimed = event(PointerEventKind::Move, 0.0, 220.0);
        claimed.consume();
        handler(claimed);
        handler(event(PointerEventKind::Move, 0.0, 280.0));

        assert!(
            (total.get() - 60.0).abs() < 0.001,
            "a consumed event ends the gesture rather than being applied twice, got {}",
            total.get()
        );
        assert!(!state.is_dragging());
    });
}

#[test]
fn dragging_is_reported_for_the_length_of_the_gesture() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let (state, _total) = recording_state();
        let (handler, _chain) =
            pointer_handler_for(Modifier::empty().draggable(Axis::Vertical, state.clone()));

        assert!(!state.is_dragging());
        handler(event(PointerEventKind::Down, 0.0, 100.0));
        assert!(!state.is_dragging(), "a press alone is not a drag");
        handler(event(PointerEventKind::Move, 0.0, 160.0));
        assert!(state.is_dragging());
        handler(event(PointerEventKind::Up, 0.0, 160.0));
        assert!(!state.is_dragging());
    });
}

#[test]
fn a_cancelled_drag_stops_reporting() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let (state, total) = recording_state();
        let (handler, _chain) =
            pointer_handler_for(Modifier::empty().draggable(Axis::Vertical, state.clone()));

        handler(event(PointerEventKind::Down, 0.0, 100.0));
        handler(event(PointerEventKind::Move, 0.0, 160.0));
        handler(event(PointerEventKind::Cancel, 0.0, 160.0));
        handler(event(PointerEventKind::Move, 0.0, 220.0));

        assert!(
            (total.get() - 60.0).abs() < 0.001,
            "a cancelled gesture reports nothing further, got {}",
            total.get()
        );
        assert!(!state.is_dragging());
    });
}

#[test]
fn a_guard_that_says_no_declines_the_gesture() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let (state, total) = recording_state();
        let enabled = Rc::new(Cell::new(false));
        let guard = Rc::clone(&enabled);
        let (handler, _chain) = pointer_handler_for(Modifier::empty().draggable_guarded(
            Axis::Vertical,
            state,
            move || guard.get(),
        ));

        handler(event(PointerEventKind::Down, 0.0, 100.0));
        handler(event(PointerEventKind::Move, 0.0, 160.0));
        assert_eq!(total.get(), 0.0, "a declined gesture reports nothing");

        enabled.set(true);
        handler(event(PointerEventKind::Down, 0.0, 100.0));
        handler(event(PointerEventKind::Move, 0.0, 160.0));
        assert!((total.get() - 60.0).abs() < 0.001);
    });
}

#[test]
fn releasing_a_fast_drag_places_the_control_rather_than_flinging_it() {
    // A scroll throws its content onwards on release. A control the user placed
    // must stop where it was let go: a knob that keeps turning after the finger
    // lifts reads as the control slipping out of the user's hand.
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let (state, total) = recording_state();
        let (handler, _chain) =
            pointer_handler_for(Modifier::empty().draggable(Axis::Vertical, state));

        handler(
            PointerEvent::new(
                PointerEventKind::Down,
                Point { x: 0.0, y: 0.0 },
                Point { x: 0.0, y: 0.0 },
            )
            .with_buttons(PointerButtons::new().with(PointerButton::Primary))
            .with_time_ms(Some(0)),
        );
        for step in 1..=6 {
            let y = step as f32 * 40.0;
            handler(
                PointerEvent::new(
                    PointerEventKind::Move,
                    Point { x: 0.0, y },
                    Point { x: 0.0, y },
                )
                .with_buttons(PointerButtons::new().with(PointerButton::Primary))
                .with_time_ms(Some(step * 8)),
            );
        }
        let placed = total.get();
        handler(
            PointerEvent::new(
                PointerEventKind::Up,
                Point { x: 0.0, y: 240.0 },
                Point { x: 0.0, y: 240.0 },
            )
            .with_buttons(PointerButtons::new().with(PointerButton::Primary))
            .with_time_ms(Some(48)),
        );

        assert_eq!(
            total.get(),
            placed,
            "releasing a drag must not carry the control any further"
        );
    });
}
