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
    rc::{Rc, Weak},
};

use cranpose_core::OwnedMutableState;

use crate::fling_animation::{SettleAnimation, SpringParams};

// Compose's `AnchoredDraggableMinFlingVelocity`: a release this fast goes on
// to the next anchor.
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
                .or_else(|| self.anchors.last())
        } else {
            self.anchors
                .iter()
                .rev()
                .find(|(_, at)| *at < position - 0.5)
                .or_else(|| self.anchors.first())
        }
        .map(|(value, _)| *value)
    }

    // The anchor that sits at `position`.
    fn at(&self, position: f32) -> Option<T> {
        self.anchors
            .iter()
            .find(|(_, at)| (*at - position).abs() < 0.01)
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
    offset: OwnedMutableState<Option<f32>>,
    current: OwnedMutableState<T>,
    settled: OwnedMutableState<T>,
    // The anchor that a running settle moves toward.
    animation_target: OwnedMutableState<Option<T>>,
    dragging: Cell<bool>,
    last_velocity: Cell<f32>,
    settle: SettleAnimation,
}

impl<T: Copy + PartialEq + 'static> AnchoredDraggableInner<T> {
    // Moves the offset by `delta`, held within the anchors, and returns how
    // far it moved.
    fn dispatch_raw_delta(&self, delta: f32) -> f32 {
        let anchors = self.anchors.borrow();
        let (Some(min), Some(max), Some(offset)) = (
            anchors.min_position(),
            anchors.max_position(),
            self.offset.get_non_reactive(),
        ) else {
            return 0.0;
        };
        let moved = (offset + delta).clamp(min, max);
        if let Some(reached) = anchors.at(moved) {
            self.current.set(reached);
        }
        drop(anchors);
        self.offset.set(Some(moved));
        moved - offset
    }

    fn rest_at(&self, value: T, position: f32) {
        self.offset.set(Some(position));
        self.current.set(value);
        self.settled.set(value);
    }
}

impl<T: Copy + PartialEq + 'static> AnchoredDraggableState<T> {
    /// A state resting at `initial_value` among `anchors`. Its offset is
    /// `None` until the anchors hold `initial_value`.
    pub fn new(initial_value: T, anchors: DraggableAnchors<T>) -> Self {
        let runtime = cranpose_core::current_runtime_handle()
            .expect("AnchoredDraggableState::new requires an active runtime");
        let offset = anchors.position_of(initial_value);
        Self {
            inner: Rc::new(AnchoredDraggableInner {
                anchors: RefCell::new(anchors),
                offset: OwnedMutableState::with_runtime_structural_eq(offset, runtime.clone()),
                current: OwnedMutableState::with_runtime_structural_eq(
                    initial_value,
                    runtime.clone(),
                ),
                settled: OwnedMutableState::with_runtime_structural_eq(
                    initial_value,
                    runtime.clone(),
                ),
                animation_target: OwnedMutableState::with_runtime_structural_eq(
                    None,
                    runtime.clone(),
                ),
                dragging: Cell::new(false),
                last_velocity: Cell::new(0.0),
                settle: SettleAnimation::new(runtime, SpringParams::SETTLE_POLICY),
            }),
        }
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

    /// Replaces the anchors, as a layout that measured new rest positions
    /// does, and keeps the state at the new anchor closest to its offset. A
    /// running settle stops.
    pub fn update_anchors(&self, new_anchors: DraggableAnchors<T>) {
        if *self.inner.anchors.borrow() == new_anchors {
            return;
        }
        let target = self
            .inner
            .offset
            .get_non_reactive()
            .and_then(|offset| new_anchors.closest_anchor(offset))
            .unwrap_or_else(|| self.inner.settled.get_non_reactive());
        self.update_anchors_to(new_anchors, target);
    }

    /// Replaces the anchors and moves the state to `new_target` among them.
    /// A running settle stops.
    pub fn update_anchors_to(&self, new_anchors: DraggableAnchors<T>, new_target: T) {
        self.stop_animation();
        let position = new_anchors.position_of(new_target);
        *self.inner.anchors.borrow_mut() = new_anchors;
        if let Some(position) = position {
            self.inner.rest_at(new_target, position);
        }
    }

    /// Moves the offset by `delta`, held within the anchors, and returns how
    /// far it moved.
    pub fn dispatch_raw_delta(&self, delta: f32) -> f32 {
        self.inner.dispatch_raw_delta(delta)
    }

    /// Moves the state to `target` at once. A running settle stops.
    pub fn snap_to(&self, target: T) {
        self.stop_animation();
        let position = self.inner.anchors.borrow().position_of(target);
        if let Some(position) = position {
            self.inner.rest_at(target, position);
        }
    }

    /// Moves the state to `target` with the settle spring.
    pub fn animate_to(&self, target: T) {
        self.settle_to(target, 0.0);
    }

    // Settles the release of a drag at `velocity`: a release faster than
    // the threshold goes on to the next anchor its way, a slower one to the
    // closest.
    pub(crate) fn settle_release(&self, velocity: f32) {
        self.inner.last_velocity.set(velocity);
        let Some(offset) = self.inner.offset.get_non_reactive() else {
            return;
        };
        let target = {
            let anchors = self.inner.anchors.borrow();
            if velocity.abs() > VELOCITY_THRESHOLD {
                anchors.next_toward(offset, velocity)
            } else {
                anchors.closest_anchor(offset)
            }
        };
        if let Some(target) = target {
            self.settle_to(target, velocity);
        }
    }

    fn settle_to(&self, target: T, velocity: f32) {
        let position = self.inner.anchors.borrow().position_of(target);
        let (Some(position), Some(offset)) = (position, self.inner.offset.get_non_reactive())
        else {
            return;
        };
        if (position - offset).abs() < 0.01 {
            self.inner.rest_at(target, position);
            return;
        }
        self.inner.animation_target.set(Some(target));
        // The spring holds the state weakly: a row removed while it settles
        // ends the spring rather than outliving it.
        let (moving, landing) = (Rc::downgrade(&self.inner), Rc::downgrade(&self.inner));
        self.inner.settle.start_settle(
            offset,
            velocity,
            position,
            move |delta| {
                Weak::upgrade(&moving).map_or(0.0, |inner| inner.dispatch_raw_delta(delta))
            },
            move |_| {
                if let Some(inner) = Weak::upgrade(&landing) {
                    inner.animation_target.set(None);
                    inner.rest_at(target, position);
                }
            },
        );
    }

    // Stops a running settle where it is, and returns whether one ran.
    pub(crate) fn stop_animation(&self) -> bool {
        let running = self.inner.settle.is_running();
        self.inner.settle.cancel();
        self.inner.animation_target.set(None);
        running
    }

    pub(crate) fn set_dragging(&self, dragging: bool) {
        self.inner.dragging.set(dragging);
    }

    pub(crate) fn offset_non_reactive(&self) -> Option<f32> {
        self.inner.offset.get_non_reactive()
    }

    pub(crate) fn identity(&self) -> usize {
        Rc::as_ptr(&self.inner) as *const () as usize
    }
}
