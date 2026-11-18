use cranpose_ui_layout::Axis;

use super::drag_gesture::draggable_impl;
use crate::{anchored_draggable::AnchoredDraggableState, modifier::Modifier};

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
        self.then(draggable_impl(axis, state.identity(), state, None))
    }
}
