use cranpose_ui_layout::Axis;

use super::{
    Modifier,
    scroll::{
        drag_gesture::{DragGesture, drag_gesture_input},
        scroll_target::ScrollTarget,
    },
};
use crate::{
    anchored_draggable::AnchoredDraggableState,
    scroll::{ScrollMotionContextKey, scroll_motion_context_for_key},
};

impl Modifier {
    /// Drags `state` along `axis` between its anchors and settles it at one
    /// when the finger lifts: Compose's `Modifier.anchoredDraggable`. The
    /// drag takes the slop and the axis lock of a scroll, so a horizontal
    /// row inside a vertical list leaves the list its scroll.
    pub fn anchored_draggable<T: Copy + PartialEq + 'static>(
        self,
        axis: Axis,
        state: AnchoredDraggableState<T>,
    ) -> Self {
        let identity = state.identity();
        self.then(drag_gesture_input(
            (identity, axis),
            DragGesture {
                target: state,
                axis,
                reverse_input: false,
                motion_context: scroll_motion_context_for_key(ScrollMotionContextKey::Draggable {
                    state_identity: identity,
                    axis,
                }),
                guard: None,
            },
        ))
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
        AnchoredDraggableState::set_dragging(self, dragging);
    }

    fn stop_animation(&self) -> bool {
        AnchoredDraggableState::stop_animation(self)
    }

    // The release settles at an anchor and takes the whole velocity, so no
    // fling follows it.
    fn pre_fling(&self, velocity: f32) -> f32 {
        self.settle_release(velocity)
    }
}
