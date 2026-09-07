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

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

use cranpose_animation::{Animatable, AnimationType, Spring, spring};
use cranpose_core::{
    NodeId, Owned, OwnedMutableState, RuntimeHandle, internal::FrameCallbackRegistration,
    with_current_composer,
};
use cranpose_foundation::DRAG_THRESHOLD;
use cranpose_ui_layout::{Measurable, MeasurePolicy, MeasureResult, MeasureScope, Placement};

use crate::{
    composable,
    layout::policies::BoxMeasurePolicy,
    modifier::{GraphicsLayer, Modifier, PointerEvent, PointerEventKind},
    subcompose_layout::Constraints,
    widgets::{
        box_widget::{Box, BoxSpec},
        layout::Layout,
    },
};

/// Offset (logical px) within which a dismiss animation counts as settled.
const DISMISS_SETTLE_EPSILON: f32 = 0.5;

/// Height-scale (fraction of the natural height) at or below which the
/// post-dismiss collapse is considered complete.
const COLLAPSE_SETTLE_EPSILON: f32 = 0.01;

/// Spring used both for the dismissal fling and the spring-back.
fn swipe_spring() -> AnimationType {
    spring(Spring::DampingRatioNoBouncy, Spring::StiffnessMediumLow)
}

/// Which edge the dismiss background is revealed on, i.e. the side the row is
/// being swiped away from. Passed to the [`SwipeToDismissSpec::with_background`]
/// closure so its label/icon can follow the swipe direction instead of being
/// pinned to one side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwipeDismissSide {
    /// The row is moving right (offset > 0); the background shows on the leading
    /// (start / left-in-LTR) edge.
    Start,
    /// The row is moving left (offset < 0); the background shows on the trailing
    /// (end / right-in-LTR) edge.
    End,
}

/// Directions accepted by a swipe-dismiss container.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SwipeDismissDirection {
    #[default]
    Both,
    StartToEnd,
    EndToStart,
}

/// Boxed background closure, invoked each frame with the current
/// [`SwipeDismissSide`] so the reveal can follow the swipe direction.
type BackgroundFn = Rc<RefCell<dyn FnMut(SwipeDismissSide)>>;

/// Configuration for [`SwipeToDismiss`].
#[derive(Clone)]
pub struct SwipeToDismissSpec {
    /// Fraction of the content width the offset must exceed on release for
    /// the row to dismiss (default `0.5`). Clamped to `(0, 1]`.
    pub threshold_fraction: f32,
    background: Option<BackgroundFn>,
    /// Identity of the wrapped content (e.g. the row's database id). When a
    /// composition slot is reused for a DIFFERENT item — unkeyed lazy-list
    /// rows all shift up after a removal — the remembered swipe state must
    /// not leak onto the new item: without a key, the next row inherits the
    /// dismissed row's displacement, revealed background and collapsed
    /// height ("items go shuffled, red boxes stick").
    ///
    /// Inside a keyed lazy list this is filled in from the item's own key, so
    /// a row states its identity once, where the list already states it. Set it
    /// explicitly only for a row that is not a lazy item, or whose identity is
    /// not the one the list is keyed by.
    pub key: Option<u64>,
    /// Caller-owned swipe state, from [`rememberSwipeDismissState`].
    ///
    /// Supply one to read the swipe as it happens — a label that fades in with
    /// it, a count of what is about to go, an undo bar after it lands — or to
    /// return a row to rest from elsewhere on the screen. Left unset, the row
    /// owns its own state.
    pub state: Option<SwipeDismissState>,
    pub direction: SwipeDismissDirection,
    pub edge_width: Option<f32>,
    pub collapse_after_dismiss: bool,
    /// Whether the content returns to rest once `on_dismiss` has fired
    /// (default `false`).
    ///
    /// A dismissed ROW is about to be removed by its host, so it stays off
    /// screen and the host drops it. A full-content NAVIGATION dismissal is
    /// not that: the gesture means "go up one level", and the host may
    /// legitimately answer by staying composed -- back out of a pause overlay
    /// and the game underneath resumes in the same root composable. Left off
    /// screen, that content never comes back, and since
    /// [`SwipeToDismissBox`] owns its state internally the application has no
    /// handle to call [`SwipeDismissState::reset`] on. The screen is then
    /// blank, taps land on nothing, and further back gestures neither redraw
    /// nor leave.
    pub reset_after_dismiss: bool,
    pub enabled: bool,
}

impl SwipeToDismissSpec {
    pub fn new() -> Self {
        Self {
            threshold_fraction: 0.5,
            background: None,
            key: None,
            state: None,
            direction: SwipeDismissDirection::Both,
            edge_width: None,
            collapse_after_dismiss: true,
            reset_after_dismiss: false,
            enabled: true,
        }
    }

    /// Declares the identity of the wrapped content. When the key changes,
    /// the swipe state resets to rest — the new item starts untouched.
    ///
    /// A row inside a keyed lazy list already has an identity and does not
    /// need this.
    pub fn with_key(mut self, key: u64) -> Self {
        self.key = Some(key);
        self
    }

    /// Uses caller-owned swipe state, so the swipe can be read and reset from
    /// outside the row.
    pub fn with_state(mut self, state: SwipeDismissState) -> Self {
        self.state = Some(state);
        self
    }

    /// Sets the dismiss threshold as a fraction of the content width.
    pub fn with_threshold_fraction(mut self, fraction: f32) -> Self {
        self.threshold_fraction = fraction;
        self
    }

    /// Sets the background content revealed behind the swiped row. The closure
    /// receives the [`SwipeDismissSide`] the row is currently being swiped
    /// toward, so it can align its label/icon to the revealed edge.
    pub fn with_background(mut self, background: impl FnMut(SwipeDismissSide) + 'static) -> Self {
        self.background = Some(Rc::new(RefCell::new(background)));
        self
    }

    pub fn with_direction(mut self, direction: SwipeDismissDirection) -> Self {
        self.direction = direction;
        self
    }

    pub fn from_edge(mut self, width: f32) -> Self {
        self.edge_width = Some(width.max(0.0));
        self
    }

    pub fn with_reset_after_dismiss(mut self, reset: bool) -> Self {
        self.reset_after_dismiss = reset;
        self
    }

    pub fn with_collapse_after_dismiss(mut self, collapse: bool) -> Self {
        self.collapse_after_dismiss = collapse;
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
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
    id: u64,
    runtime: RuntimeHandle,
    offset: RefCell<Animatable<f32>>,
    revealed: OwnedMutableState<bool>,
    collapse: RefCell<Animatable<f32>>,
    phase: Cell<SwipePhase>,
    width_px: Cell<f32>,
    threshold_fraction: Cell<f32>,
    on_dismiss: RefCell<Option<Rc<dyn Fn()>>>,
    dismissed: OwnedMutableState<bool>,
    node_id: Cell<Option<NodeId>>,
    settle_watcher: RefCell<Option<FrameCallbackRegistration>>,
    collapse_watcher: RefCell<Option<FrameCallbackRegistration>>,
    identity: Cell<Option<u64>>,
    active_pointer: Cell<Option<u64>>,
    direction: Cell<SwipeDismissDirection>,
    edge_width: Cell<Option<f32>>,
    collapse_after_dismiss: Cell<bool>,
    reset_after_dismiss: Cell<bool>,
    enabled: Cell<bool>,
}

impl SwipeToDismissController {
    fn new(runtime: RuntimeHandle) -> Rc<Self> {
        Rc::new(Self {
            id: NEXT_SWIPE_ID.fetch_add(1, Ordering::Relaxed),
            offset: RefCell::new(Animatable::new(0.0, runtime.clone())),
            revealed: OwnedMutableState::with_runtime(false, runtime.clone()),
            collapse: RefCell::new(Animatable::new(1.0, runtime.clone())),
            dismissed: OwnedMutableState::with_runtime(false, runtime.clone()),
            runtime,
            phase: Cell::new(SwipePhase::Idle),
            width_px: Cell::new(f32::NAN),
            threshold_fraction: Cell::new(0.5),
            on_dismiss: RefCell::new(None),
            node_id: Cell::new(None),
            settle_watcher: RefCell::new(None),
            collapse_watcher: RefCell::new(None),
            identity: Cell::new(None),
            active_pointer: Cell::new(None),
            direction: Cell::new(SwipeDismissDirection::Both),
            edge_width: Cell::new(None),
            collapse_after_dismiss: Cell::new(true),
            reset_after_dismiss: Cell::new(false),
            enabled: Cell::new(true),
        })
    }

    fn reset_to_rest(&self) {
        self.settle_watcher.borrow_mut().take();
        self.collapse_watcher.borrow_mut().take();
        self.offset.borrow_mut().snapTo(0.0);
        self.collapse.borrow_mut().snapTo(1.0);
        self.phase.set(SwipePhase::Idle);
        self.active_pointer.set(None);
        self.set_dismissed(false);
        self.set_revealed(false);
    }

    fn current_offset(&self) -> f32 {
        self.offset.borrow().state().value()
    }

    fn revealed_side(&self) -> SwipeDismissSide {
        if self.current_offset() >= 0.0 {
            SwipeDismissSide::Start
        } else {
            SwipeDismissSide::End
        }
    }

    fn collapse_fraction(&self) -> f32 {
        self.collapse.borrow().state().value()
    }

    fn revealed(&self) -> bool {
        self.revealed.value()
    }

    fn set_revealed(&self, revealed: bool) {
        if self.revealed.get_non_reactive() != revealed {
            self.revealed.set_value(revealed);
        }
    }

    fn set_dismissed(&self, dismissed: bool) {
        if self.dismissed.get_non_reactive() != dismissed {
            self.dismissed.set_value(dismissed);
        }
    }

    fn snap_to(&self, offset: f32) {
        self.offset.borrow_mut().snapTo(offset);
        self.set_revealed(offset != 0.0);
    }

    fn animate_to(&self, target: f32) {
        self.offset.borrow_mut().animateTo(target, swipe_spring());
        if target != 0.0 {
            self.set_revealed(true);
        }
    }
}

/// A row's swipe, as the application can see it.
///
/// A dismissable row is not only a callback: an application shows a delete
/// label that fades in with the swipe, a counter of what is about to go, or an
/// undo bar once a row has left. All of that needs the swipe itself, not just
/// its ending, so the same state the widget drives is readable from outside it.
///
/// Cloning shares one row's state. Hold it with [`rememberSwipeDismissState`]
/// and hand it to [`SwipeToDismissSpec::with_state`].
#[derive(Clone)]
pub struct SwipeDismissState {
    controller: Rc<SwipeToDismissController>,
}

impl PartialEq for SwipeDismissState {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.controller, &other.controller)
    }
}

impl SwipeDismissState {
    fn new(runtime: RuntimeHandle) -> Self {
        Self {
            controller: SwipeToDismissController::new(runtime),
        }
    }

    /// How far the row is displaced, in logical pixels: positive towards the
    /// start edge, negative towards the end. Reactive.
    pub fn offset(&self) -> f32 {
        self.controller.current_offset()
    }

    /// How far through a dismissal the row is, in `0..=1`, against the same
    /// threshold a release is judged by. `1.0` means letting go now dismisses.
    ///
    /// Zero before the row has been measured — a fraction of an unknown width
    /// would be a guess.
    pub fn progress(&self) -> f32 {
        let width = self.controller.width_px.get();
        if !width.is_finite() || width <= 0.0 {
            return 0.0;
        }
        let threshold = width * self.controller.threshold_fraction.get();
        if threshold <= 0.0 {
            return 0.0;
        }
        (self.offset().abs() / threshold).clamp(0.0, 1.0)
    }

    /// The edge the background is revealed on, or `None` while the row is at
    /// rest and no edge is showing.
    pub fn side(&self) -> Option<SwipeDismissSide> {
        (self.offset() != 0.0).then(|| self.controller.revealed_side())
    }

    /// Whether the row is away from rest — dragged, springing back, or leaving.
    /// Reactive.
    pub fn is_displaced(&self) -> bool {
        self.controller.revealed()
    }

    /// Whether this row has been dismissed. Reactive.
    pub fn is_dismissed(&self) -> bool {
        self.controller.dismissed.value()
    }

    /// Returns the row to rest without dismissing it — an undo, or a screen
    /// closing a menu the swipe opened.
    pub fn reset(&self) {
        self.controller.reset_to_rest();
    }
}

/// Remembers the swipe state for one row.
///
/// Inside a keyed lazy list the state is keyed by the item, so a row removed
/// from the middle does not leave its displacement on the row that moves up
/// into its slot.
#[allow(non_snake_case)]
#[track_caller]
pub fn rememberSwipeDismissState() -> SwipeDismissState {
    let caller = cranpose_core::caller_location_key();
    let state = with_current_composer(|composer| {
        let runtime = composer.runtime_handle();
        let owned: Owned<SwipeDismissState> =
            composer.remember_at(caller, || SwipeDismissState::new(runtime));
        owned.with(SwipeDismissState::clone)
    });
    let identity = crate::lazy_item::lazy_item_key();
    if state.controller.identity.get() != identity {
        state.controller.reset_to_rest();
        state.controller.identity.set(identity);
    }
    state
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
    if event.kind != PointerEventKind::Down
        && event.kind != PointerEventKind::Cancel
        && controller.active_pointer.get() != Some(event.id)
    {
        return;
    }

    match event.kind {
        PointerEventKind::Down => {
            if event.is_consumed()
                || !controller.enabled.get()
                || controller.active_pointer.get().is_some()
            {
                return;
            }
            let width = controller.width_px.get();
            let inside_edge = match (controller.edge_width.get(), controller.direction.get()) {
                (None, _) => true,
                (Some(edge), SwipeDismissDirection::StartToEnd) => event.global_position.x <= edge,
                (Some(edge), SwipeDismissDirection::EndToStart) => {
                    width.is_finite() && event.global_position.x >= width - edge
                }
                (Some(edge), SwipeDismissDirection::Both) => {
                    event.global_position.x <= edge
                        || (width.is_finite() && event.global_position.x >= width - edge)
                }
            };
            if !inside_edge {
                return;
            }
            controller.active_pointer.set(Some(event.id));
            let current = controller.current_offset();
            controller.snap_to(current);
            controller.phase.set(SwipePhase::Tracking {
                down_x: event.global_position.x,
                down_y: event.global_position.y,
                start_offset: current,
            });
        }
        PointerEventKind::Move => {
            if event.is_consumed() {
                if matches!(controller.phase.get(), SwipePhase::Dragging { .. }) {
                    animate_spring_back(controller);
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
                            controller.snap_to(constrain_direction(
                                clamp_offset(start_offset + total_dx, width),
                                controller.direction.get(),
                            ));
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
                    controller.snap_to(constrain_direction(
                        clamp_offset(start_offset + total_dx, width),
                        controller.direction.get(),
                    ));
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
            controller.active_pointer.set(None);
        }
        PointerEventKind::Cancel => {
            if matches!(controller.phase.get(), SwipePhase::Dragging { .. }) {
                animate_spring_back(controller);
            }
            controller.phase.set(SwipePhase::Idle);
            controller.active_pointer.set(None);
        }
        PointerEventKind::Scroll
        | PointerEventKind::Zoom
        | PointerEventKind::RotaryScrollPre
        | PointerEventKind::RotaryScroll
        | PointerEventKind::Enter
        | PointerEventKind::Exit => {}
    }
}

fn constrain_direction(offset: f32, direction: SwipeDismissDirection) -> f32 {
    match direction {
        SwipeDismissDirection::Both => offset,
        SwipeDismissDirection::StartToEnd => offset.max(0.0),
        SwipeDismissDirection::EndToStart => offset.min(0.0),
    }
}

/// Applies the release decision: animate off-screen and watch for completion
/// (firing `on_dismiss`), or spring back to rest.
fn settle_release(controller: &Rc<SwipeToDismissController>) {
    let offset = controller.current_offset();
    let width = controller.width_px.get();
    match dismissal_target(offset, width, controller.threshold_fraction.get()) {
        Some(target) => animate_dismiss(controller, target),
        None => animate_spring_back(controller),
    }
}

/// Flings the row off-screen and watches for `on_dismiss` to fire once settled.
fn animate_dismiss(controller: &Rc<SwipeToDismissController>, target: f32) {
    controller.animate_to(target);
    watch_settle(controller, true);
}

/// Springs the row back to rest and watches so the reveal is hidden again once
/// the content actually returns to offset 0.
fn animate_spring_back(controller: &Rc<SwipeToDismissController>) {
    controller.animate_to(0.0);
    watch_settle(controller, false);
}

/// Watches the settle animation frame-by-frame. Once the offset reaches its
/// target it syncs the reveal flag to the resting displacement and, for a
/// dismissal, fires `on_dismiss` exactly once. Runs in frame callbacks
/// (outside composition), so the callback may freely mutate state.
fn watch_settle(controller: &Rc<SwipeToDismissController>, dismissing: bool) {
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
                if dismissing && controller.dismissed.get_non_reactive() {
                    return;
                }
                if matches!(controller.phase.get(), SwipePhase::Dragging { .. }) {
                    return;
                }
                let target = controller.offset.borrow().target();
                let value = controller.current_offset();
                if (value - target).abs() <= DISMISS_SETTLE_EPSILON {
                    controller.set_revealed(false);
                    if dismissing && !controller.dismissed.get_non_reactive() {
                        controller.set_dismissed(true);
                        if controller.collapse_after_dismiss.get() {
                            start_collapse(&controller);
                        }
                        let on_dismiss = controller.on_dismiss.borrow().clone();
                        if let Some(on_dismiss) = on_dismiss {
                            on_dismiss();
                        }
                        if controller.reset_after_dismiss.get() {
                            controller.reset_to_rest();
                        }
                    }
                } else {
                    watch_settle(&controller, dismissing);
                }
            });
    *controller.settle_watcher.borrow_mut() = Some(registration);
}

/// Animates the row's height scale to `0.0` and watches the animation so the
/// list re-measures the shrinking row each frame. Runs after a dismiss settles.
fn start_collapse(controller: &Rc<SwipeToDismissController>) {
    controller
        .collapse
        .borrow_mut()
        .animateTo(0.0, swipe_spring());
    watch_collapse(controller);
}

/// Frame-by-frame watcher for the post-dismiss collapse: the row height is a
/// layout output, so a plain animated value would not reach the parent list on
/// its own — each frame this forces a scoped re-measure of the row and a redraw
/// until the height scale reaches zero.
fn watch_collapse(controller: &Rc<SwipeToDismissController>) {
    let weak = Rc::downgrade(controller);
    let registration =
        controller
            .runtime
            .frame_clock()
            .with_frame_nanos(move |_frame_time_nanos| {
                let Some(controller) = weak.upgrade() else {
                    return;
                };
                controller.collapse_watcher.borrow_mut().take();
                if let Some(node_id) = controller.node_id.get() {
                    crate::schedule_measure_repass(node_id);
                }
                crate::request_render_invalidation();
                if controller.collapse_fraction() > COLLAPSE_SETTLE_EPSILON {
                    watch_collapse(&controller);
                }
            });
    *controller.collapse_watcher.borrow_mut() = Some(registration);
}

#[derive(Clone, Copy, PartialEq)]
enum SwipeLayoutPhase {
    Row,
    Collapse,
}

#[derive(Clone)]
struct SwipeMeasurePolicy {
    controller: Rc<SwipeToDismissController>,
    phase: SwipeLayoutPhase,
}

impl PartialEq for SwipeMeasurePolicy {
    fn eq(&self, other: &Self) -> bool {
        self.phase == other.phase && Rc::ptr_eq(&self.controller, &other.controller)
    }
}

impl MeasurePolicy for SwipeMeasurePolicy {
    fn measure(
        &self,
        scope: &dyn MeasureScope,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        if self.phase == SwipeLayoutPhase::Row {
            self.controller.width_px.set(constraints.max_width);
            return BoxMeasurePolicy::new(crate::Alignment::TOP_START, false).measure(
                scope,
                measurables,
                constraints,
            );
        }
        let child_constraints = Constraints {
            min_height: 0.0,
            ..constraints
        };
        let mut placements = Vec::with_capacity(measurables.len());
        let mut width = 0.0_f32;
        let mut natural_height = 0.0_f32;
        for measurable in measurables {
            let placeable = measurable.measure(child_constraints);
            width = width.max(placeable.width());
            natural_height = natural_height.max(placeable.height());
            placeable.place(0.0, 0.0);
            placements.push(Placement::new(placeable.node_id(), 0.0, 0.0, 0));
        }
        let width = width.clamp(constraints.min_width, constraints.max_width);
        let height = (natural_height * self.controller.collapse_fraction().clamp(0.0, 1.0))
            .clamp(0.0, constraints.max_height);
        MeasureResult::new(crate::modifier::Size { width, height }, placements)
    }

    fn min_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        BoxMeasurePolicy::new(crate::Alignment::TOP_START, false)
            .min_intrinsic_width(measurables, height)
    }

    fn max_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        BoxMeasurePolicy::new(crate::Alignment::TOP_START, false)
            .max_intrinsic_width(measurables, height)
    }

    fn min_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        BoxMeasurePolicy::new(crate::Alignment::TOP_START, false)
            .min_intrinsic_height(measurables, width)
    }

    fn max_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        BoxMeasurePolicy::new(crate::Alignment::TOP_START, false)
            .max_intrinsic_height(measurables, width)
    }
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
    let owned_controller: Rc<SwipeToDismissController> = with_current_composer(|composer| {
        let runtime = composer.runtime_handle();
        let owned: Owned<Rc<SwipeToDismissController>> =
            composer.remember(|| SwipeToDismissController::new(runtime));
        owned.with(Rc::clone)
    });
    let controller = match &spec.state {
        Some(state) => Rc::clone(&state.controller),
        None => owned_controller,
    };

    let identity = spec.key.or_else(crate::lazy_item::lazy_item_key);
    if controller.identity.get() != identity {
        controller.reset_to_rest();
        controller.identity.set(identity);
    }

    controller
        .threshold_fraction
        .set(spec.threshold_fraction.clamp(f32::EPSILON, 1.0));
    controller.direction.set(spec.direction);
    controller.edge_width.set(spec.edge_width);
    controller
        .collapse_after_dismiss
        .set(spec.collapse_after_dismiss);
    controller.reset_after_dismiss.set(spec.reset_after_dismiss);
    controller.enabled.set(spec.enabled);
    *controller.on_dismiss.borrow_mut() = Some(Rc::new(on_dismiss));

    let background = spec.background.clone();
    let content = Rc::new(RefCell::new(content));

    let gesture_modifier = swipe_gesture_modifier(modifier, Rc::clone(&controller));

    let controller_for_layout = Rc::clone(&controller);
    let node = Layout(
        Modifier::empty(),
        SwipeMeasurePolicy {
            phase: SwipeLayoutPhase::Collapse,
            controller: Rc::clone(&controller_for_layout),
        },
        move || {
            let background = background.clone();
            let content = Rc::clone(&content);
            let controller_for_row = Rc::clone(&controller_for_layout);
            let gesture_modifier = gesture_modifier.clone();
            Layout(
                gesture_modifier,
                SwipeMeasurePolicy {
                    phase: SwipeLayoutPhase::Row,
                    controller: Rc::clone(&controller_for_row),
                },
                move || {
                    if controller_for_row.revealed() {
                        if let Some(background) = &background {
                            let background = Rc::clone(background);
                            let side = controller_for_row.revealed_side();
                            Box(Modifier::empty(), BoxSpec::new(), move || {
                                (background.borrow_mut())(side);
                            });
                        }
                    }
                    let content = Rc::clone(&content);
                    let controller_for_layer = Rc::clone(&controller_for_row);
                    Box(
                        Modifier::empty().graphics_layer(move || GraphicsLayer {
                            translation_x: controller_for_layer.current_offset(),
                            ..GraphicsLayer::default()
                        }),
                        BoxSpec::new(),
                        move || {
                            (content.borrow_mut())();
                        },
                    );
                },
            );
        },
    );
    controller.node_id.set(Some(node));
    node
}

/// Full-content navigation dismissal. The gesture starts at the leading edge,
/// moves only toward the end edge, and leaves sizing to the navigation owner.
#[composable]
pub fn SwipeToDismissBox<D, F>(modifier: Modifier, on_dismiss: D, content: F) -> NodeId
where
    D: Fn() + 'static,
    F: FnMut() + 'static,
{
    SwipeToDismiss(
        modifier,
        SwipeToDismissSpec::new()
            .with_threshold_fraction(0.35)
            .with_direction(SwipeDismissDirection::StartToEnd)
            .from_edge(32.0)
            .with_collapse_after_dismiss(false)
            .with_reset_after_dismiss(true),
        on_dismiss,
        content,
    )
}

#[cfg(test)]
#[path = "../tests/swipe_to_dismiss_tests.rs"]
mod tests;
