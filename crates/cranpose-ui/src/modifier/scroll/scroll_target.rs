//! The state that the drag of a scroller moves. A nested scroll container
//! around the scroller takes its part of each move first.

use cranpose_foundation::{ScrollId, lazy::LazyListState};
use cranpose_ui_layout::Axis;

use crate::{
    anchored_draggable::AnchoredDraggableState,
    draggable::{DraggableState, ScrollableState},
    nested_scroll::NestedScrollSource,
    scroll::{ScrollSettlePolicy, ScrollState},
};

pub(super) trait ScrollTarget: Clone {
    fn apply_delta(&self, delta: f32) -> f32;

    fn apply_wheel_delta(&self, delta: f32) -> f32 {
        self.apply_delta(delta)
    }

    fn apply_fling_delta(&self, delta: f32) -> f32;

    fn invalidate(&self);

    fn current_offset(&self) -> f32;

    fn can_scroll(&self) -> bool {
        true
    }

    fn can_consume(&self, gesture_delta: f32) -> bool {
        let _ = gesture_delta;
        self.can_scroll()
    }

    fn settle_policy(&self) -> Option<ScrollSettlePolicy> {
        None
    }

    fn allows_fling(&self) -> bool {
        true
    }

    fn set_dragging(&self, dragging: bool) {
        let _ = dragging;
    }

    /// Stops a scroll animation the app started, which a press takes over,
    /// and returns whether one ran.
    fn stop_animation(&self) -> bool {
        false
    }

    /// The part of a release's `velocity`, in content units per second, the
    /// containers around the scroller take before it flings.
    fn pre_fling(&self, velocity: f32) -> f32 {
        let _ = velocity;
        0.0
    }

    // Settles the release of a drag at `velocity`, in content units per
    // second, on the target's own rest positions, and returns whether it
    // did. The scroll's fling and settle then stay out.
    fn settle_release(&self, _velocity: f32) -> bool {
        false
    }

    // Makes the motion that `stop` stops the target's running scroll. A jump
    // or a press stops it as it stops an animation. `None` for a target that
    // keeps no running scroll.
    fn start_scroll(&self, _stop: impl FnOnce() + 'static) -> Option<ScrollId> {
        None
    }

    // Forgets the running scroll `id` once its motion ended.
    fn end_scroll(&self, _id: ScrollId) {}
}

/// A scroller inside [`crate::NestedScroll`] containers: every move goes
/// through them, before and after the scroller takes its part.
#[derive(Clone)]
pub(super) struct NestedTarget<S> {
    pub(super) target: S,
    pub(super) parent: crate::nested_scroll::NestedScrollParent,
    pub(super) axis: Axis,
}

impl<S: ScrollTarget> ScrollTarget for NestedTarget<S> {
    fn apply_delta(&self, delta: f32) -> f32 {
        self.parent
            .dispatch(self.axis, delta, NestedScrollSource::Drag, |left| {
                self.target.apply_delta(left)
            })
    }

    fn apply_wheel_delta(&self, delta: f32) -> f32 {
        self.parent
            .dispatch(self.axis, delta, NestedScrollSource::Wheel, |left| {
                self.target.apply_wheel_delta(left)
            })
    }

    /// A fling moves the scroll value, the opposite way to the content.
    fn apply_fling_delta(&self, delta: f32) -> f32 {
        -self
            .parent
            .dispatch(self.axis, -delta, NestedScrollSource::Fling, |left| {
                -self.target.apply_fling_delta(-left)
            })
    }

    fn invalidate(&self) {
        self.target.invalidate();
    }

    fn current_offset(&self) -> f32 {
        self.target.current_offset()
    }

    fn can_scroll(&self) -> bool {
        self.target.can_scroll()
    }

    fn can_consume(&self, gesture_delta: f32) -> bool {
        self.target.can_consume(gesture_delta)
    }

    fn settle_policy(&self) -> Option<ScrollSettlePolicy> {
        self.target.settle_policy()
    }

    fn allows_fling(&self) -> bool {
        self.target.allows_fling()
    }

    fn set_dragging(&self, dragging: bool) {
        self.target.set_dragging(dragging);
    }

    fn settle_release(&self, velocity: f32) -> bool {
        self.target.settle_release(velocity)
    }

    fn stop_animation(&self) -> bool {
        self.target.stop_animation()
    }

    fn pre_fling(&self, velocity: f32) -> f32 {
        self.parent.pre_fling(self.axis, velocity)
    }

    fn start_scroll(&self, stop: impl FnOnce() + 'static) -> Option<ScrollId> {
        self.target.start_scroll(stop)
    }

    fn end_scroll(&self, id: ScrollId) {
        self.target.end_scroll(id);
    }
}

macro_rules! delegate_scroll_animation {
    () => {
        fn stop_animation(&self) -> bool {
            self.stop_scroll_animation()
        }

        fn start_scroll(&self, stop: impl FnOnce() + 'static) -> Option<ScrollId> {
            self.start_scroll_animation(stop)
        }

        fn end_scroll(&self, id: ScrollId) {
            self.end_scroll_animation(id);
        }
    };
}

impl ScrollTarget for ScrollState {
    fn apply_delta(&self, delta: f32) -> f32 {
        -self.dispatch_raw_delta(-delta)
    }

    fn apply_fling_delta(&self, delta: f32) -> f32 {
        self.dispatch_raw_delta(delta)
    }

    fn invalidate(&self) {}

    fn current_offset(&self) -> f32 {
        self.try_value_non_reactive().unwrap_or(0.0)
    }

    fn can_scroll(&self) -> bool {
        self.max_value() > 0.0
    }

    fn can_consume(&self, gesture_delta: f32) -> bool {
        let value = self.value_non_reactive();
        let raw = -gesture_delta;
        if raw > 0.0 {
            value < self.max_value()
        } else {
            value > 0.0
        }
    }

    fn settle_policy(&self) -> Option<ScrollSettlePolicy> {
        Self::settle_policy(self)
    }

    delegate_scroll_animation!();
}

impl ScrollTarget for LazyListState {
    fn apply_delta(&self, delta: f32) -> f32 {
        self.dispatch_scroll_delta(delta)
    }

    fn apply_wheel_delta(&self, delta: f32) -> f32 {
        if delta.abs() <= 0.001 {
            0.0
        } else {
            self.dispatch_scroll_delta(delta)
        }
    }

    fn apply_fling_delta(&self, delta: f32) -> f32 {
        -self.dispatch_scroll_delta(-delta)
    }

    fn invalidate(&self) {}

    fn current_offset(&self) -> f32 {
        self.first_visible_item_scroll_offset()
    }

    fn can_scroll(&self) -> bool {
        self.total_items_count() == 0
            || self.can_scroll_forward_non_reactive()
            || self.can_scroll_backward_non_reactive()
    }

    fn can_consume(&self, gesture_delta: f32) -> bool {
        if self.total_items_count() == 0 {
            return true;
        }
        if gesture_delta < 0.0 {
            self.can_scroll_forward_non_reactive()
        } else {
            self.can_scroll_backward_non_reactive()
        }
    }

    fn settle_policy(&self) -> Option<ScrollSettlePolicy> {
        Self::settle_policy(self)
    }

    fn set_dragging(&self, dragging: bool) {
        self.set_dragged(dragging);
    }

    delegate_scroll_animation!();
}

impl ScrollTarget for DraggableState {
    fn apply_delta(&self, delta: f32) -> f32 {
        self.drag_by(delta);
        delta
    }

    fn apply_fling_delta(&self, delta: f32) -> f32 {
        self.drag_by(delta);
        delta
    }

    fn invalidate(&self) {}

    fn current_offset(&self) -> f32 {
        self.offset()
    }

    fn allows_fling(&self) -> bool {
        false
    }

    fn set_dragging(&self, dragging: bool) {
        Self::set_dragging(self, dragging);
    }
}

impl<T: Copy + PartialEq + 'static> ScrollTarget for AnchoredDraggableState<T> {
    fn apply_delta(&self, delta: f32) -> f32 {
        self.dispatch_raw_delta(delta)
    }

    fn apply_fling_delta(&self, delta: f32) -> f32 {
        self.dispatch_raw_delta(delta)
    }

    fn invalidate(&self) {}

    fn current_offset(&self) -> f32 {
        self.offset_non_reactive().unwrap_or(0.0)
    }

    fn set_dragging(&self, dragging: bool) {
        Self::set_dragging(self, dragging);
    }

    fn stop_animation(&self) -> bool {
        Self::stop_animation(self)
    }

    fn settle_release(&self, velocity: f32) -> bool {
        Self::settle_release(self, velocity);
        true
    }
}

impl ScrollTarget for ScrollableState {
    fn apply_delta(&self, delta: f32) -> f32 {
        self.dispatch_raw_delta(delta)
    }

    fn apply_fling_delta(&self, delta: f32) -> f32 {
        -self.dispatch_raw_delta(-delta)
    }

    fn invalidate(&self) {}

    fn current_offset(&self) -> f32 {
        -self.offset()
    }

    fn set_dragging(&self, dragging: bool) {
        Self::set_dragging(self, dragging);
    }
}
