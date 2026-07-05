//! SwipeToDismiss composable
//!
//! Mirrors Jetpack Compose's `SwipeToDismissBox` (Material) in spirit: wraps
//! content that can be dragged horizontally; releasing past a threshold
//! animates the content off-screen and fires `on_dismiss`, otherwise the
//! content springs back into place.
//!
//! # Gesture disambiguation
//!
//! The drag capture mirrors the axis-locking rules of the scroll modifier so
//! a `SwipeToDismiss` row inside a vertical `LazyColumn` coexists with the
//! list scroll (children receive pointer events before ancestors):
//!
//! - the swipe captures the gesture only once the *horizontal* travel exceeds
//!   the drag slop while dominating the vertical travel; from then on events
//!   are consumed so the parent scroll abandons the gesture;
//! - a decisively *vertical* start locks the swipe out for the rest of the
//!   gesture, leaving every event unconsumed for the parent to scroll;
//! - taps (no travel beyond the slop) consume nothing, so clickable rows
//!   keep working.

#![allow(non_snake_case)]

use crate::composable;
use crate::modifier::{Modifier, PointerEvent, PointerEventKind};
use crate::widgets::box_widget::{Box, BoxSpec};
use crate::widgets::layout::BoxWithConstraints;
use crate::widgets::scopes::BoxWithConstraintsScope;
use cranpose_animation::{spring, Animatable, AnimationType, Spring};
use cranpose_core::internal::FrameCallbackRegistration;
use cranpose_core::{with_current_composer, Owned, RuntimeHandle};
use cranpose_foundation::DRAG_THRESHOLD;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Offset (logical px) within which a dismiss animation counts as settled.
const DISMISS_SETTLE_EPSILON: f32 = 0.5;

/// Spring used both for the dismissal fling and the spring-back.
fn swipe_spring() -> AnimationType {
    spring(Spring::DampingRatioNoBouncy, Spring::StiffnessMediumLow)
}

/// Configuration for [`SwipeToDismiss`].
#[derive(Clone)]
pub struct SwipeToDismissSpec {
    /// Fraction of the content width the offset must exceed on release for
    /// the row to dismiss (default `0.5`). Clamped to `(0, 1]`.
    pub threshold_fraction: f32,
    /// Optional content drawn behind the swiped row (e.g. a delete bin),
    /// revealed as the row slides away.
    background: Option<Rc<RefCell<dyn FnMut()>>>,
}

impl SwipeToDismissSpec {
    pub fn new() -> Self {
        Self {
            threshold_fraction: 0.5,
            background: None,
        }
    }

    /// Sets the dismiss threshold as a fraction of the content width.
    pub fn with_threshold_fraction(mut self, fraction: f32) -> Self {
        self.threshold_fraction = fraction;
        self
    }

    /// Sets the background content revealed behind the swiped row.
    pub fn with_background(mut self, background: impl FnMut() + 'static) -> Self {
        self.background = Some(Rc::new(RefCell::new(background)));
        self
    }
}

impl Default for SwipeToDismissSpec {
    fn default() -> Self {
        Self::new()
    }
}

/// Phase of the swipe gesture state machine.
#[derive(Clone, Copy, Debug, PartialEq)]
enum SwipePhase {
    /// No active pointer sequence.
    Idle,
    /// Pointer down, axis not decided yet.
    Tracking {
        down_x: f32,
        down_y: f32,
        start_offset: f32,
    },
    /// Horizontal axis won: the swipe owns the gesture and consumes events.
    Dragging { down_x: f32, start_offset: f32 },
    /// Vertical axis won decisively: never capture for this gesture.
    LockedOut,
}

/// Axis decision for the initial slop check. Mirrors the scroll modifier:
/// the main axis captures when it exceeds the slop *and* dominates; the
/// cross axis locks out when it exceeds the slop and strictly dominates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SwipeAxisDecision {
    Undecided,
    Horizontal,
    Vertical,
}

/// Decides the gesture axis from the total travel since pointer down.
pub(crate) fn decide_axis(total_dx: f32, total_dy: f32, slop: f32) -> SwipeAxisDecision {
    let horizontal = total_dx.abs();
    let vertical = total_dy.abs();
    if horizontal > slop && horizontal >= vertical {
        SwipeAxisDecision::Horizontal
    } else if vertical > slop && vertical > horizontal {
        SwipeAxisDecision::Vertical
    } else {
        SwipeAxisDecision::Undecided
    }
}

/// Where the released row should animate to: `Some(±width)` when the offset
/// crossed the dismiss threshold, `None` to spring back to rest.
pub(crate) fn dismissal_target(offset: f32, width: f32, threshold_fraction: f32) -> Option<f32> {
    if !width.is_finite() || width <= 0.0 {
        return None;
    }
    let threshold = width * threshold_fraction.clamp(f32::EPSILON, 1.0);
    (offset.abs() >= threshold).then(|| width * offset.signum())
}

/// Clamps the dragged offset so the content cannot travel further than one
/// full width in either direction.
pub(crate) fn clamp_offset(offset: f32, width: f32) -> f32 {
    if width.is_finite() && width > 0.0 {
        offset.clamp(-width, width)
    } else {
        offset
    }
}

static NEXT_SWIPE_ID: AtomicU64 = AtomicU64::new(0);

/// State shared between the composable (which reads the animated offset) and
/// the pointer-input handler (which drives it). Main-thread only.
struct SwipeToDismissController {
    /// Stable key for the pointer-input modifier.
    id: u64,
    runtime: RuntimeHandle,
    /// Animated horizontal offset of the content in logical px.
    offset: RefCell<Animatable<f32>>,
    phase: Cell<SwipePhase>,
    /// Content width in logical px, refreshed from the incoming constraints.
    width_px: Cell<f32>,
    threshold_fraction: Cell<f32>,
    /// Refreshed every recomposition so the latest captured environment runs.
    on_dismiss: RefCell<Option<Rc<dyn Fn()>>>,
    /// `on_dismiss` fired for the current dismissal (fires at most once).
    dismissed: Cell<bool>,
    /// Frame callback watching the dismiss animation for completion.
    settle_watcher: RefCell<Option<FrameCallbackRegistration>>,
}

impl SwipeToDismissController {
    fn new(runtime: RuntimeHandle) -> Rc<Self> {
        Rc::new(Self {
            id: NEXT_SWIPE_ID.fetch_add(1, Ordering::Relaxed),
            offset: RefCell::new(Animatable::new(0.0, runtime.clone())),
            runtime,
            phase: Cell::new(SwipePhase::Idle),
            width_px: Cell::new(f32::NAN),
            threshold_fraction: Cell::new(0.5),
            on_dismiss: RefCell::new(None),
            dismissed: Cell::new(false),
            settle_watcher: RefCell::new(None),
        })
    }

    fn current_offset(&self) -> f32 {
        self.offset.borrow().state().value()
    }

    fn snap_to(&self, offset: f32) {
        self.offset.borrow_mut().snapTo(offset);
    }

    fn animate_to(&self, target: f32) {
        self.offset.borrow_mut().animateTo(target, swipe_spring());
    }
}

/// Appends the swipe pointer-input handler to `base`. Split out of the
/// composable so headless tests can drive the exact production modifier
/// through a manually built modifier chain.
fn swipe_gesture_modifier(base: Modifier, controller: Rc<SwipeToDismissController>) -> Modifier {
    let key = controller.id;
    base.pointer_input(key, move |scope| {
        let controller = Rc::clone(&controller);
        async move {
            scope
                .await_pointer_event_scope(|await_scope| async move {
                    loop {
                        let event = await_scope.await_pointer_event().await;
                        handle_swipe_event(&controller, &event);
                    }
                })
                .await;
        }
    })
}

/// Handles one pointer event for the swipe gesture. Returns nothing; event
/// consumption communicates ownership to sibling/ancestor handlers.
fn handle_swipe_event(controller: &Rc<SwipeToDismissController>, event: &PointerEvent) {
    // Secondary fingers never participate: the swipe is a one-finger gesture
    // and multi-touch handlers (zoom) key off the extra pointers themselves.
    if event.id != 0 && event.kind != PointerEventKind::Cancel {
        return;
    }

    match event.kind {
        PointerEventKind::Down => {
            if event.is_consumed() {
                return;
            }
            // Catch an in-flight spring: pressing pins the content where it
            // currently is, like Compose's swipeable.
            let current = controller.current_offset();
            controller.snap_to(current);
            controller.phase.set(SwipePhase::Tracking {
                down_x: event.global_position.x,
                down_y: event.global_position.y,
                start_offset: current,
            });
            // Down is never consumed: taps must keep reaching clickables.
        }
        PointerEventKind::Move => {
            if event.is_consumed() {
                // Another handler owns this sequence (parent scroll started,
                // or a sibling captured); abandon and rest.
                if matches!(controller.phase.get(), SwipePhase::Dragging { .. }) {
                    controller.animate_to(0.0);
                }
                controller.phase.set(SwipePhase::Idle);
                return;
            }
            match controller.phase.get() {
                SwipePhase::Tracking {
                    down_x,
                    down_y,
                    start_offset,
                } => {
                    let total_dx = event.global_position.x - down_x;
                    let total_dy = event.global_position.y - down_y;
                    match decide_axis(total_dx, total_dy, DRAG_THRESHOLD) {
                        SwipeAxisDecision::Horizontal => {
                            controller.phase.set(SwipePhase::Dragging {
                                down_x,
                                start_offset,
                            });
                            let width = controller.width_px.get();
                            controller.snap_to(clamp_offset(start_offset + total_dx, width));
                            event.consume();
                        }
                        SwipeAxisDecision::Vertical => {
                            controller.phase.set(SwipePhase::LockedOut);
                        }
                        SwipeAxisDecision::Undecided => {}
                    }
                }
                SwipePhase::Dragging {
                    down_x,
                    start_offset,
                } => {
                    let total_dx = event.global_position.x - down_x;
                    let width = controller.width_px.get();
                    controller.snap_to(clamp_offset(start_offset + total_dx, width));
                    event.consume();
                }
                SwipePhase::Idle | SwipePhase::LockedOut => {}
            }
        }
        PointerEventKind::Up => {
            let phase = controller.phase.get();
            controller.phase.set(SwipePhase::Idle);
            if let SwipePhase::Dragging { .. } = phase {
                settle_release(controller);
                event.consume();
            }
        }
        PointerEventKind::Cancel => {
            if matches!(controller.phase.get(), SwipePhase::Dragging { .. }) {
                controller.animate_to(0.0);
            }
            controller.phase.set(SwipePhase::Idle);
        }
        PointerEventKind::Scroll
        | PointerEventKind::Zoom
        | PointerEventKind::Enter
        | PointerEventKind::Exit => {}
    }
}

/// Applies the release decision: animate off-screen and watch for completion
/// (firing `on_dismiss`), or spring back to rest.
fn settle_release(controller: &Rc<SwipeToDismissController>) {
    let offset = controller.current_offset();
    let width = controller.width_px.get();
    match dismissal_target(offset, width, controller.threshold_fraction.get()) {
        Some(target) => {
            controller.animate_to(target);
            watch_dismiss_settle(controller);
        }
        None => {
            controller.animate_to(0.0);
        }
    }
}

/// Watches the dismiss animation frame-by-frame; once the offset settles at
/// the off-screen target, fires `on_dismiss` exactly once. Runs in frame
/// callbacks (outside composition), so the callback may freely mutate state.
fn watch_dismiss_settle(controller: &Rc<SwipeToDismissController>) {
    let weak = Rc::downgrade(controller);
    let registration =
        controller
            .runtime
            .frame_clock()
            .with_frame_nanos(move |_frame_time_nanos| {
                let Some(controller) = weak.upgrade() else {
                    return;
                };
                controller.settle_watcher.borrow_mut().take();
                if controller.dismissed.get() {
                    return;
                }
                // A new gesture retargeted the animation; stop watching.
                if matches!(controller.phase.get(), SwipePhase::Dragging { .. }) {
                    return;
                }
                let target = controller.offset.borrow().target();
                let value = controller.current_offset();
                if (value - target).abs() <= DISMISS_SETTLE_EPSILON {
                    controller.dismissed.set(true);
                    let on_dismiss = controller.on_dismiss.borrow().clone();
                    if let Some(on_dismiss) = on_dismiss {
                        on_dismiss();
                    }
                } else {
                    watch_dismiss_settle(&controller);
                }
            });
    *controller.settle_watcher.borrow_mut() = Some(registration);
}

/// Wraps `content` so it can be swiped away horizontally.
///
/// Dragging moves the content with the finger (clamped to one content width
/// in either direction). Releasing past `spec.threshold_fraction` of the
/// width animates the content off-screen with a spring and then fires
/// `on_dismiss` (exactly once); releasing earlier springs the content back.
/// An optional [`SwipeToDismissSpec::with_background`] content (delete bin,
/// archive icon, ...) is revealed behind the row while it is displaced.
///
/// Vertical drags are handed to ancestor scroll containers untouched, so
/// rows inside a `LazyColumn` still scroll the list (see the module docs for
/// the axis-locking rules).
#[composable(no_skip)]
pub fn SwipeToDismiss<D, F>(
    modifier: Modifier,
    spec: SwipeToDismissSpec,
    on_dismiss: D,
    content: F,
) -> cranpose_core::NodeId
where
    D: Fn() + 'static,
    F: FnMut() + 'static,
{
    let controller: Rc<SwipeToDismissController> = with_current_composer(|composer| {
        let runtime = composer.runtime_handle();
        let owned: Owned<Rc<SwipeToDismissController>> =
            composer.remember(|| SwipeToDismissController::new(runtime));
        owned.with(Rc::clone)
    });

    // Refresh the per-recomposition inputs so the pointer handler (which
    // lives across recompositions) observes the latest values.
    controller
        .threshold_fraction
        .set(spec.threshold_fraction.clamp(f32::EPSILON, 1.0));
    *controller.on_dismiss.borrow_mut() = Some(Rc::new(on_dismiss));

    let background = spec.background.clone();
    let content = Rc::new(RefCell::new(content));

    // The gesture area is the whole (un-offset) row: like a scroll
    // container, the handler sits on the ancestor node so descendants
    // (clickables) see events first, and the hit region stays put while the
    // content slides away.
    let gesture_modifier = swipe_gesture_modifier(modifier, Rc::clone(&controller));

    let controller_for_layout = Rc::clone(&controller);
    BoxWithConstraints(gesture_modifier, move |scope| {
        controller_for_layout
            .width_px
            .set(scope.constraints().max_width);

        if let Some(background) = &background {
            let background = Rc::clone(background);
            Box(Modifier::empty(), BoxSpec::new(), move || {
                (background.borrow_mut())();
            });
        }

        // Reading the animated offset subscribes this scope: every spring
        // frame recomposes and re-places the content at the new offset.
        let offset_x = controller_for_layout.current_offset();
        let content = Rc::clone(&content);
        Box(
            Modifier::empty().offset(offset_x, 0.0),
            BoxSpec::new(),
            move || {
                (content.borrow_mut())();
            },
        );
    })
}

#[cfg(test)]
#[path = "../tests/swipe_to_dismiss_tests.rs"]
mod tests;
