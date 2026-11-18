use std::rc::Rc;

use cranpose_ui_layout::Axis;

use super::{
    ScrollTarget,
    drag_gesture::{DragGesture, drag_gesture_input},
};
use crate::{
    anchored_draggable::AnchoredDraggableState,
    draggable::DraggableState,
    local_layout_direction,
    modifier::Modifier,
    scroll::{ScrollMotionContextKey, scroll_motion_context_for_key},
};

impl Modifier {
    /// Drags along `axis`, reporting each delta to `state`.
    ///
    /// The gesture is the one the scroll containers run — touch slop before a
    /// drag begins, axis locking so a mostly-vertical drag never steals a
    /// horizontal one, the primary pointer only, and yielding to whoever
    /// consumed the event first — so a dragged control and a scrolled list feel
    /// the same under a finger. Unlike a scroll, a release does not fling: a
    /// control the user placed stays where it was let go.
    ///
    /// Deltas are logical pixels, positive to the right for [`Axis::Horizontal`]
    /// and downwards for [`Axis::Vertical`].
    pub fn draggable(self, axis: Axis, state: DraggableState) -> Self {
        self.then(draggable_impl(axis, state.identity(), state, false, None))
    }

    /// Drags along `axis` only while `guard` says so.
    ///
    /// The guard is asked per event, so a control can stop accepting a drag
    /// half way through one and the gesture ends rather than being applied to
    /// a control that has since been disabled.
    pub fn draggable_guarded(
        self,
        axis: Axis,
        state: DraggableState,
        guard: impl Fn() -> bool + 'static,
    ) -> Self {
        self.then(draggable_impl(
            axis,
            state.identity(),
            state,
            false,
            Some(Rc::new(guard)),
        ))
    }

    /// Drags `state` along `axis` between its anchors and settles it at one
    /// when the finger lifts: Compose's `Modifier.anchoredDraggable`. The
    /// drag takes the slop and the axis lock of a scroll, so a horizontal
    /// row inside a vertical list leaves the list its scroll. In a
    /// right-to-left layout a horizontal drag runs from the right edge, so
    /// the anchors of `state` measure from the start edge as
    /// [`Modifier::offset_with`] places them.
    pub fn anchored_draggable<T: Copy + PartialEq + 'static>(
        self,
        axis: Axis,
        state: AnchoredDraggableState<T>,
    ) -> Self {
        let reverse = axis == Axis::Horizontal && composed_right_to_left();
        self.then(draggable_impl(axis, state.identity(), state, reverse, None))
    }
}

// Whether the composition building this modifier lays out right to left.
fn composed_right_to_left() -> bool {
    cranpose_core::composer_context::current_composer().is_some()
        && local_layout_direction().current().is_rtl()
}

// The drag of `state` along `axis`, keyed by its `identity`. A
// `reverse_input` drag hands `state` the opposite of the pointer's deltas.
fn draggable_impl<S: ScrollTarget + 'static>(
    axis: Axis,
    identity: usize,
    state: S,
    reverse_input: bool,
    guard: Option<Rc<dyn Fn() -> bool>>,
) -> Modifier {
    drag_gesture_input(
        (identity, axis),
        DragGesture {
            target: state,
            axis,
            reverse_input,
            motion_context: scroll_motion_context_for_key(ScrollMotionContextKey::Draggable {
                state_identity: identity,
                axis,
            }),
            guard,
        },
    )
}
