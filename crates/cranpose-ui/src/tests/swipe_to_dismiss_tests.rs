//! Headless tests for the SwipeToDismiss gesture math and dismiss/restore
//! behavior. The controller state machine is driven with synthetic pointer
//! events; spring animations advance through manual frame-clock pumps.

use super::*;
use crate::collect_modifier_slices;
use crate::scroll::ScrollState;
use cranpose_core::{DefaultScheduler, Runtime};
use cranpose_foundation::{
    BasicModifierNodeContext, ModifierNodeChain, PointerButton, PointerButtons,
};
use cranpose_ui_graphics::Point;

const FRAME_NANOS: u64 = 16_666_667; // ~60 FPS

fn point(x: f32, y: f32) -> Point {
    Point { x, y }
}

fn primary_buttons() -> PointerButtons {
    PointerButtons::new().with(PointerButton::Primary)
}

fn down(x: f32, y: f32) -> PointerEvent {
    PointerEvent::new(PointerEventKind::Down, point(x, y), point(x, y))
        .with_buttons(primary_buttons())
}

fn move_to(x: f32, y: f32) -> PointerEvent {
    PointerEvent::new(PointerEventKind::Move, point(x, y), point(x, y))
        .with_buttons(primary_buttons())
}

fn up(x: f32, y: f32) -> PointerEvent {
    PointerEvent::new(PointerEventKind::Up, point(x, y), point(x, y))
        .with_buttons(primary_buttons())
}

fn cancel() -> PointerEvent {
    PointerEvent::new(PointerEventKind::Cancel, point(0.0, 0.0), point(0.0, 0.0))
}

struct Harness {
    _runtime: Runtime,
    controller: Rc<SwipeToDismissController>,
    dismiss_count: Rc<Cell<usize>>,
    frame_time: u64,
}

impl Harness {
    fn new(width: f32, threshold_fraction: f32) -> Self {
        let runtime = Runtime::new(std::sync::Arc::new(DefaultScheduler));
        let controller = SwipeToDismissController::new(runtime.handle());
        controller.width_px.set(width);
        controller.threshold_fraction.set(threshold_fraction);

        let dismiss_count = Rc::new(Cell::new(0usize));
        let count = Rc::clone(&dismiss_count);
        *controller.on_dismiss.borrow_mut() = Some(Rc::new(move || {
            count.set(count.get() + 1);
        }));

        Self {
            _runtime: runtime,
            controller,
            dismiss_count,
            frame_time: 0,
        }
    }

    fn send(&self, event: &PointerEvent) {
        handle_swipe_event(&self.controller, event);
    }

    fn offset(&self) -> f32 {
        self.controller.current_offset()
    }

    /// Advances the frame clock, driving springs and the settle watcher.
    fn pump_frames(&mut self, frames: usize) {
        let handle = self._runtime.handle();
        for _ in 0..frames {
            self.frame_time += FRAME_NANOS;
            handle.drain_frame_callbacks(self.frame_time);
        }
    }
}

// ---------------------------------------------------------------------------
// Pure gesture math
// ---------------------------------------------------------------------------

#[test]
fn axis_stays_undecided_within_slop() {
    assert_eq!(decide_axis(0.0, 0.0, 8.0), SwipeAxisDecision::Undecided);
    assert_eq!(decide_axis(8.0, 0.0, 8.0), SwipeAxisDecision::Undecided);
    assert_eq!(decide_axis(-7.9, 5.0, 8.0), SwipeAxisDecision::Undecided);
    assert_eq!(decide_axis(0.0, -8.0, 8.0), SwipeAxisDecision::Undecided);
}

#[test]
fn axis_capture_requires_horizontal_dominance() {
    assert_eq!(decide_axis(9.0, 0.0, 8.0), SwipeAxisDecision::Horizontal);
    assert_eq!(decide_axis(-9.0, 4.0, 8.0), SwipeAxisDecision::Horizontal);
    // Ties go to the swipe (main axis wins on >=, like the scroll modifier).
    assert_eq!(decide_axis(9.0, 9.0, 8.0), SwipeAxisDecision::Horizontal);
}

#[test]
fn axis_locks_out_on_decisive_vertical_travel() {
    assert_eq!(decide_axis(0.0, 9.0, 8.0), SwipeAxisDecision::Vertical);
    assert_eq!(decide_axis(4.0, -12.0, 8.0), SwipeAxisDecision::Vertical);
}

#[test]
fn dismissal_target_uses_threshold_fraction_and_sign() {
    assert_eq!(dismissal_target(99.0, 200.0, 0.5), None);
    assert_eq!(dismissal_target(100.0, 200.0, 0.5), Some(200.0));
    assert_eq!(dismissal_target(-150.0, 200.0, 0.5), Some(-200.0));
    assert_eq!(dismissal_target(30.0, 200.0, 0.1), Some(200.0));
    // Unknown or degenerate widths never dismiss.
    assert_eq!(dismissal_target(500.0, f32::NAN, 0.5), None);
    assert_eq!(dismissal_target(500.0, 0.0, 0.5), None);
}

#[test]
fn clamp_offset_limits_travel_to_one_width() {
    assert_eq!(clamp_offset(250.0, 200.0), 200.0);
    assert_eq!(clamp_offset(-250.0, 200.0), -200.0);
    assert_eq!(clamp_offset(50.0, 200.0), 50.0);
    // Unknown width leaves the offset untouched.
    assert_eq!(clamp_offset(250.0, f32::NAN), 250.0);
}

// ---------------------------------------------------------------------------
// Gesture state machine
// ---------------------------------------------------------------------------

#[test]
fn horizontal_drag_captures_and_follows_the_finger() {
    let harness = Harness::new(200.0, 0.5);

    harness.send(&down(10.0, 10.0));

    // Within the slop: no capture, no consumption.
    let small = move_to(14.0, 10.0);
    harness.send(&small);
    assert!(!small.is_consumed());
    assert_eq!(harness.offset(), 0.0);

    // Crossing the slop horizontally captures and consumes.
    let capture = move_to(30.0, 12.0);
    harness.send(&capture);
    assert!(capture.is_consumed());
    assert_eq!(harness.offset(), 20.0);

    // Subsequent moves track the finger relative to the down position.
    let drag = move_to(140.0, 12.0);
    harness.send(&drag);
    assert!(drag.is_consumed());
    assert_eq!(harness.offset(), 130.0);

    // The offset clamps at one content width.
    let far = move_to(500.0, 12.0);
    harness.send(&far);
    assert_eq!(harness.offset(), 200.0);
}

#[test]
fn vertical_drag_locks_out_and_leaves_events_for_the_parent() {
    let harness = Harness::new(200.0, 0.5);

    harness.send(&down(10.0, 10.0));

    // Decisively vertical: the parent (LazyColumn) must win.
    let vertical = move_to(12.0, 40.0);
    harness.send(&vertical);
    assert!(!vertical.is_consumed());

    // Even large horizontal travel later in the gesture stays unconsumed.
    let late_horizontal = move_to(80.0, 40.0);
    harness.send(&late_horizontal);
    assert!(!late_horizontal.is_consumed());
    assert_eq!(harness.offset(), 0.0);

    let release = up(80.0, 40.0);
    harness.send(&release);
    assert!(!release.is_consumed());
}

#[test]
fn tap_consumes_nothing() {
    let harness = Harness::new(200.0, 0.5);

    let press = down(10.0, 10.0);
    harness.send(&press);
    assert!(!press.is_consumed());

    let release = up(11.0, 10.0);
    harness.send(&release);
    assert!(!release.is_consumed());
    assert_eq!(harness.offset(), 0.0);
}

#[test]
fn release_past_threshold_animates_out_and_fires_on_dismiss_once() {
    let mut harness = Harness::new(200.0, 0.5);

    harness.send(&down(10.0, 10.0));
    harness.send(&move_to(30.0, 10.0));
    harness.send(&move_to(140.0, 10.0)); // offset 130 >= threshold 100

    let release = up(140.0, 10.0);
    harness.send(&release);
    assert!(release.is_consumed());
    assert_eq!(harness.dismiss_count.get(), 0, "fires only after settling");

    harness.pump_frames(300);
    assert_eq!(harness.dismiss_count.get(), 1);
    assert!(
        (harness.offset() - 200.0).abs() <= 0.5,
        "content settles off-screen, got offset {}",
        harness.offset()
    );

    // Further frames never re-fire the callback.
    harness.pump_frames(60);
    assert_eq!(harness.dismiss_count.get(), 1);
}

#[test]
fn release_below_threshold_springs_back_without_dismissing() {
    let mut harness = Harness::new(200.0, 0.5);

    harness.send(&down(10.0, 10.0));
    harness.send(&move_to(30.0, 10.0));
    harness.send(&move_to(70.0, 10.0)); // offset 60 < threshold 100
    harness.send(&up(70.0, 10.0));

    harness.pump_frames(300);
    assert_eq!(harness.dismiss_count.get(), 0);
    assert!(
        harness.offset().abs() <= 0.5,
        "content springs back to rest, got offset {}",
        harness.offset()
    );
}

#[test]
fn leftward_swipe_dismisses_to_the_left() {
    let mut harness = Harness::new(200.0, 0.5);

    harness.send(&down(150.0, 10.0));
    harness.send(&move_to(30.0, 10.0)); // offset -120

    harness.send(&up(30.0, 10.0));
    harness.pump_frames(300);
    assert_eq!(harness.dismiss_count.get(), 1);
    assert!((harness.offset() + 200.0).abs() <= 0.5);
}

#[test]
fn consumed_move_abandons_the_gesture_and_restores() {
    let mut harness = Harness::new(200.0, 0.5);

    harness.send(&down(10.0, 10.0));
    harness.send(&move_to(60.0, 10.0)); // captured, offset 50

    // A foreign handler consumed this sequence; the swipe must let go.
    let foreign = move_to(80.0, 10.0);
    foreign.consume();
    harness.send(&foreign);

    harness.pump_frames(300);
    assert_eq!(harness.dismiss_count.get(), 0);
    assert!(harness.offset().abs() <= 0.5);
}

#[test]
fn cancel_springs_back() {
    let mut harness = Harness::new(200.0, 0.5);

    harness.send(&down(10.0, 10.0));
    harness.send(&move_to(90.0, 10.0)); // captured, offset 80
    harness.send(&cancel());

    harness.pump_frames(300);
    assert_eq!(harness.dismiss_count.get(), 0);
    assert!(harness.offset().abs() <= 0.5);
}

// ---------------------------------------------------------------------------
// Nested with a vertical scroll parent (modifier-chain dispatch, mirroring
// the shell's leaf-first hit-path order like the nested scroll tests)
// ---------------------------------------------------------------------------

/// Builds the production pointer handler for a modifier by driving it
/// through a real modifier-node chain (same approach as zoom/scroll tests).
fn pointer_handler_for(modifier: crate::Modifier) -> (Rc<dyn Fn(PointerEvent)>, ModifierNodeChain) {
    let elements = modifier.elements();
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();
    chain.update_from_slice(&elements, &mut context);
    let slices = collect_modifier_slices(&chain);
    let handler = slices
        .pointer_inputs()
        .first()
        .cloned()
        .expect("swipe modifier should provide a pointer input handler");
    (handler, chain)
}

/// Dispatches an event leaf-first (swipe child, then scroll parent),
/// mirroring the shell's hit-path dispatch order.
fn dispatch_nested(
    child: &Rc<dyn Fn(PointerEvent)>,
    parent: &Rc<dyn Fn(PointerEvent)>,
    event: PointerEvent,
) -> PointerEvent {
    child(event.clone());
    parent(event.clone());
    event
}

struct NestedHarness {
    harness: Harness,
    swipe: Rc<dyn Fn(PointerEvent)>,
    _swipe_chain: ModifierNodeChain,
    scroll_state: ScrollState,
    scroll: Rc<dyn Fn(PointerEvent)>,
    _scroll_chain: ModifierNodeChain,
    _app_context: crate::render_state::TestAppContextScope,
}

impl NestedHarness {
    /// A swipeable row nested in a vertical scroll container.
    fn new() -> Self {
        let app_context = crate::render_state::app_context_test_scope();
        let harness = Harness::new(200.0, 0.5);

        let (swipe, swipe_chain) = pointer_handler_for(swipe_gesture_modifier(
            crate::Modifier::empty(),
            Rc::clone(&harness.controller),
        ));

        let scroll_state = ScrollState::new(100.0);
        scroll_state.set_max_value(400.0);
        let (scroll, scroll_chain) = pointer_handler_for(
            crate::Modifier::empty().vertical_scroll(scroll_state.clone(), false),
        );

        Self {
            harness,
            swipe,
            _swipe_chain: swipe_chain,
            scroll_state,
            scroll,
            _scroll_chain: scroll_chain,
            _app_context: app_context,
        }
    }

    fn send(&self, event: PointerEvent) -> PointerEvent {
        dispatch_nested(&self.swipe, &self.scroll, event)
    }
}

#[test]
fn vertical_drag_on_row_scrolls_parent_list() {
    let nested = NestedHarness::new();

    nested.send(down(100.0, 100.0));
    // Decisively vertical: the swipe locks itself out, the parent scrolls.
    nested.send(move_to(102.0, 60.0));
    nested.send(move_to(104.0, 20.0));
    nested.send(up(104.0, 20.0));

    assert_eq!(nested.harness.offset(), 0.0, "row must not move sideways");
    assert_eq!(nested.harness.dismiss_count.get(), 0);
    assert!(
        (nested.scroll_state.value_non_reactive() - 180.0).abs() < 1.0,
        "vertical drag must scroll the parent list (finger up 80 => offset +80), got {}",
        nested.scroll_state.value_non_reactive()
    );
}

#[test]
fn horizontal_swipe_on_row_does_not_scroll_parent_list() {
    let mut nested = NestedHarness::new();

    nested.send(down(20.0, 100.0));
    // Mostly horizontal with vertical jitter: the swipe captures, so the
    // consumed moves never scroll the parent.
    let capture = nested.send(move_to(40.0, 104.0));
    assert!(capture.is_consumed(), "swipe must capture horizontal drags");
    nested.send(move_to(160.0, 98.0));
    let release = nested.send(up(160.0, 98.0));
    assert!(release.is_consumed());

    assert_eq!(
        nested.scroll_state.value_non_reactive(),
        100.0,
        "parent list must not scroll during a captured swipe"
    );

    nested.harness.pump_frames(300);
    assert_eq!(
        nested.harness.dismiss_count.get(),
        1,
        "the released swipe crossed the threshold and must dismiss"
    );
}

#[test]
fn pressing_mid_flight_pins_the_content() {
    let mut harness = Harness::new(200.0, 0.5);

    // Swipe below threshold and release: spring-back starts.
    harness.send(&down(10.0, 10.0));
    harness.send(&move_to(90.0, 10.0));
    harness.send(&up(90.0, 10.0));
    harness.pump_frames(3);
    let mid_flight = harness.offset();
    assert!(mid_flight > 0.0 && mid_flight < 80.0);

    // Pressing again stops the animation where it is.
    harness.send(&down(50.0, 10.0));
    harness.pump_frames(5);
    assert_eq!(harness.offset(), mid_flight);

    // And the new drag continues from the pinned offset.
    let drag = move_to(70.0, 10.0);
    harness.send(&drag);
    assert!(drag.is_consumed());
    assert_eq!(harness.offset(), mid_flight + 20.0);
}
