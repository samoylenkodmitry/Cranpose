use std::rc::Rc;

use cranpose_ui_layout::Axis;

use super::{
    ScrollTarget,
    drag_gesture::{DragGesture, drag_gesture_input},
};
use crate::{
    anchored_draggable::AnchoredDraggableState,
    draggable::DraggableState,
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
        self.then(draggable_impl(axis, state.identity(), state, None))
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
            Some(Rc::new(guard)),
        ))
    }

    /// Drags `state` along `axis` between its anchors and settles it at one
    /// when the finger lifts: Compose's `Modifier.anchoredDraggable`. The
    /// drag takes the slop and the axis lock of a scroll, so a horizontal
    /// row inside a vertical list leaves the list its scroll.
    pub fn anchored_draggable<T: Copy + PartialEq + 'static>(
        self,
        axis: Axis,
        state: AnchoredDraggableState<T>,
    ) -> Self {
        self.then(draggable_impl(axis, state.identity(), state, None))
    }
}

// The drag of `state` along `axis`, keyed by its `identity`.
fn draggable_impl<S: ScrollTarget + 'static>(
    axis: Axis,
    identity: usize,
    state: S,
    guard: Option<Rc<dyn Fn() -> bool>>,
) -> Modifier {
    drag_gesture_input(
        (identity, axis),
        DragGesture {
            target: state,
            axis,
            reverse_input: false,
            motion_context: scroll_motion_context_for_key(ScrollMotionContextKey::Draggable {
                state_identity: identity,
                axis,
            }),
            guard,
        },
    )
}
