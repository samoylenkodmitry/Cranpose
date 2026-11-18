//! Anchored dragging: Compose's `DraggableAnchors`, `AnchoredDraggableState`
//! and `Modifier.anchoredDraggable`.
//!
//! A control that a finger drags between rest positions, such as a list row
//! that opens on its swipe actions or a drawer, follows the finger along one
//! axis and settles at one of its anchors on release. A release faster than
//! 125 points a second goes to the next anchor in its direction; a slower one
//! goes to the closest anchor, so the drag has to pass half the distance.
//!
//! ```text
//! let state = remember(|| AnchoredDraggableState::new(Row::Closed, anchors)).with(Clone::clone);
//! Box(Modifier::empty().anchored_draggable(Axis::Horizontal, state.clone()), …);
//! ```

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{MutableState, RuntimeHandle};

use crate::fling_animation::{SettleAnimation, SpringParams};

// Compose's `AnchoredDraggableMinFlingVelocity` and default positional
// threshold: a release this fast goes on to the next anchor.
const VELOCITY_THRESHOLD: f32 = 125.0;

/// The positions a drag rests at, each named by a value of `T`: Compose's
/// `DraggableAnchors`.
#[derive(Clone, Debug, PartialEq)]
pub struct DraggableAnchors<T> {
    // Sorted by position.
    anchors: Vec<(T, f32)>,
}

impl<T: Copy + PartialEq> DraggableAnchors<T> {
    /// Anchors at the given positions, in logical pixels along the drag's
    /// axis.
    pub fn new(anchors: impl IntoIterator<Item = (T, f32)>) -> Self {
        let mut anchors: Vec<(T, f32)> = anchors.into_iter().collect();
        anchors.sort_by(|a, b| a.1.total_cmp(&b.1));
        Self { anchors }
    }

    /// Where `anchor` sits, or `None` when these anchors do not hold it.
    pub fn position_of(&self, anchor: T) -> Option<f32> {
        self.anchors
            .iter()
            .find(|(value, _)| *value == anchor)
            .map(|(_, position)| *position)
    }

    /// Whether these anchors hold `anchor`.
    pub fn has_position_for(&self, anchor: T) -> bool {
        self.position_of(anchor).is_some()
    }

    /// The anchor closest to `position`, or `None` when there are none.
    pub fn closest_anchor(&self, position: f32) -> Option<T> {
        self.anchors
            .iter()
            .min_by(|a, b| (a.1 - position).abs().total_cmp(&(b.1 - position).abs()))
            .map(|(value, _)| *value)
    }

    /// How many anchors there are.
    pub fn len(&self) -> usize {
        self.anchors.len()
    }

    /// Whether there are no anchors.
    pub fn is_empty(&self) -> bool {
        self.anchors.is_empty()
    }

    /// The lowest anchor position, or `None` when there are none.
    pub fn min_position(&self) -> Option<f32> {
        self.anchors.first().map(|(_, position)| *position)
    }

    /// The highest anchor position, or `None` when there are none.
    pub fn max_position(&self) -> Option<f32> {
        self.anchors.last().map(|(_, position)| *position)
    }

    // The first anchor past `position` in the direction of `velocity`, or
    // the last one that way when none lies past it.
    fn next_toward(&self, position: f32, velocity: f32) -> Option<T> {
        if velocity > 0.0 {
            self.anchors
                .iter()
                .find(|(_, at)| *at > position + 0.5)
                .or(self.anchors.last())
        } else {
            self.anchors
                .iter()
                .rev()
                .find(|(_, at)| *at < position - 0.5)
                .or(self.anchors.first())
        }
        .map(|(value, _)| *value)
    }
}

/// The state of a drag between [`DraggableAnchors`]: Compose's
/// `AnchoredDraggableState`. `Modifier::anchored_draggable` moves it with a
/// finger; a release settles it at an anchor with a spring.
///
/// Create it inside a composition and keep it with `remember`.
pub struct AnchoredDraggableState<T: Copy + 'static> {
    inner: Rc<AnchoredDraggableInner<T>>,
}

impl<T: Copy> Clone for AnchoredDraggableState<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

struct AnchoredDraggableInner<T: Copy + 'static> {
    anchors: RefCell<DraggableAnchors<T>>,
    offset: MutableState<Option<f32>>,
    current: MutableState<T>,
    settled: MutableState<T>,
    // The anchor that a running settle moves toward.
    animation_target: MutableState<Option<T>>,
    dragging: Cell<bool>,
    last_velocity: Cell<f32>,
    runtime: RuntimeHandle,
    settle: RefCell<SettleAnimation>,
}

impl<T: Copy + PartialEq + 'static> AnchoredDraggableState<T> {
    /// A state resting at `initial_value` among `anchors`. Its offset is
    /// `None` until the anchors hold `initial_value`.
    pub fn new(initial_value: T, anchors: DraggableAnchors<T>) -> Self {
        let runtime = cranpose_core::current_runtime_handle()
            .expect("AnchoredDraggableState::new requires an active runtime");
        Self::with_runtime(initial_value, anchors, runtime)
    }

    fn with_runtime(
        initial_value: T,
        anchors: DraggableAnchors<T>,
        runtime: RuntimeHandle,
    ) -> Self {
        let offset = anchors.position_of(initial_value);
        Self {
            inner: Rc::new(AnchoredDraggableInner {
                anchors: RefCell::new(anchors),
                offset: MutableState::with_runtime(offset, runtime.clone()),
                current: MutableState::with_runtime(initial_value, runtime.clone()),
                settled: MutableState::with_runtime(initial_value, runtime.clone()),
                animation_target: MutableState::with_runtime(None, runtime.clone()),
                dragging: Cell::new(false),
                last_velocity: Cell::new(0.0),
                settle: RefCell::new(SettleAnimation::new(
                    runtime.clone(),
                    SpringParams::SETTLE_POLICY,
                )),
                runtime,
            }),
        }
    }

    /// Returns the state with `spring` as the motion that settles it.
    pub fn with_snap_spring(self, spring: SpringParams) -> Self {
        self.stop_animation();
        *self.inner.settle.borrow_mut() = SettleAnimation::new(self.inner.runtime.clone(), spring);
        self
    }

    /// The anchor the state last passed through or rests at.
    pub fn current_value(&self) -> T {
        self.inner.current.get()
    }

    /// Where the state is going: the anchor a settle moves toward, the
    /// anchor closest to a drag, or the current value at rest.
    pub fn target_value(&self) -> T {
        if let Some(target) = self.inner.animation_target.get() {
            return target;
        }
        if self.inner.dragging.get()
            && let Some(offset) = self.inner.offset.get()
            && let Some(closest) = self.inner.anchors.borrow().closest_anchor(offset)
        {
            return closest;
        }
        self.current_value()
    }

    /// The anchor the state last came to rest at.
    pub fn settled_value(&self) -> T {
        self.inner.settled.get()
    }

    /// The offset in logical pixels along the drag's axis, or `None` until
    /// the anchors hold the current value.
    pub fn offset(&self) -> Option<f32> {
        self.inner.offset.get()
    }

    /// The offset, which the anchors must have set.
    ///
    /// # Panics
    ///
    /// Panics while the offset is `None`.
    pub fn require_offset(&self) -> f32 {
        self.offset()
            .expect("AnchoredDraggableState::require_offset before the anchors were set")
    }

    /// Whether a settle or an `animate_to` moves the state now.
    pub fn is_animation_running(&self) -> bool {
        self.inner.animation_target.get().is_some()
    }

    /// The velocity of the last release, in logical pixels a second.
    pub fn last_velocity(&self) -> f32 {
        self.inner.last_velocity.get()
    }

    /// How far the offset lies from `from` toward `to`, from 0 to 1.
    pub fn progress(&self, from: T, to: T) -> f32 {
        let anchors = self.inner.anchors.borrow();
        let (Some(start), Some(end), Some(offset)) = (
            anchors.position_of(from),
            anchors.position_of(to),
            self.offset(),
        ) else {
            return 0.0;
        };
        if (end - start).abs() < f32::EPSILON {
            return 1.0;
        }
        ((offset - start) / (end - start)).clamp(0.0, 1.0)
    }

    /// Replaces the anchors and moves the state to `new_target` among them,
    /// as a layout that measured new rest positions does. A running settle
    /// stops.
    pub fn update_anchors(&self, new_anchors: DraggableAnchors<T>, new_target: T) {
        if *self.inner.anchors.borrow() == new_anchors
            && self.inner.current.get_non_reactive() == new_target
        {
            return;
        }
        self.stop_animation();
        let position = new_anchors.position_of(new_target);
        *self.inner.anchors.borrow_mut() = new_anchors;
        if let Some(position) = position {
            self.rest_at(new_target, position);
        }
    }

    /// Moves the offset by `delta`, held within the anchors, and returns how
    /// far it moved.
    pub fn dispatch_raw_delta(&self, delta: f32) -> f32 {
        let anchors = self.inner.anchors.borrow();
        let (Some(min), Some(max), Some(offset)) = (
            anchors.min_position(),
            anchors.max_position(),
            self.inner.offset.get_non_reactive(),
        ) else {
            return 0.0;
        };
        let moved = (offset + delta).clamp(min, max);
        if let Some(reached) = anchors
            .anchors
            .iter()
            .find(|(_, at)| (*at - moved).abs() < 0.01)
            .map(|(value, _)| *value)
            && self.inner.current.get_non_reactive() != reached
        {
            self.inner.current.set(reached);
        }
        drop(anchors);
        self.inner.offset.set(Some(moved));
        moved - offset
    }

    /// Moves the state to `target` at once. A running settle stops.
    pub fn snap_to(&self, target: T) {
        self.stop_animation();
        if let Some(position) = self.inner.anchors.borrow().position_of(target) {
            self.rest_at(target, position);
        }
    }

    /// Moves the state to `target` with the settle spring.
    pub fn animate_to(&self, target: T) {
        self.settle_to(target, 0.0);
    }

    // Where a release at `velocity` settles.
    fn release_target(&self, offset: f32, velocity: f32) -> Option<T> {
        let anchors = self.inner.anchors.borrow();
        if velocity.abs() > VELOCITY_THRESHOLD {
            anchors.next_toward(offset, velocity)
        } else {
            anchors.closest_anchor(offset)
        }
    }

    // Settles a release at `velocity` and returns the velocity it took.
    pub(crate) fn settle_release(&self, velocity: f32) -> f32 {
        self.inner.last_velocity.set(velocity);
        let Some(offset) = self.inner.offset.get_non_reactive() else {
            return 0.0;
        };
        if let Some(target) = self.release_target(offset, velocity) {
            self.settle_to(target, velocity);
        }
        velocity
    }

    fn settle_to(&self, target: T, velocity: f32) {
        let (Some(position), Some(offset)) = (
            self.inner.anchors.borrow().position_of(target),
            self.inner.offset.get_non_reactive(),
        ) else {
            return;
        };
        if (position - offset).abs() < 0.01 {
            self.rest_at(target, position);
            return;
        }
        self.inner.animation_target.set(Some(target));
        let settle = self.inner.settle.borrow().clone();
        let (moving, landing) = (self.clone(), self.clone());
        settle.start_settle(
            offset,
            velocity,
            position,
            move |delta| moving.dispatch_raw_delta(delta),
            move |_| {
                landing.inner.animation_target.set(None);
                landing.rest_at(target, position);
            },
        );
    }

    fn rest_at(&self, value: T, position: f32) {
        self.inner.offset.set(Some(position));
        if self.inner.current.get_non_reactive() != value {
            self.inner.current.set(value);
        }
        if self.inner.settled.get_non_reactive() != value {
            self.inner.settled.set(value);
        }
    }

    // Stops a running settle where it is, and returns whether one ran.
    pub(crate) fn stop_animation(&self) -> bool {
        let settle = self.inner.settle.borrow().clone();
        let running = settle.is_running();
        settle.cancel();
        if self.inner.animation_target.get_non_reactive().is_some() {
            self.inner.animation_target.set(None);
        }
        running
    }

    pub(crate) fn set_dragging(&self, dragging: bool) {
        self.inner.dragging.set(dragging);
        if dragging {
            self.stop_animation();
        }
    }

    pub(crate) fn offset_non_reactive(&self) -> Option<f32> {
        self.inner.offset.get_non_reactive()
    }

    pub(crate) fn identity(&self) -> usize {
        Rc::as_ptr(&self.inner) as *const () as usize
    }
}
