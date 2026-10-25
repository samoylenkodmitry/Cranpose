#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

pub mod gesture_constants;
pub mod lazy;
pub mod modifier;
pub mod modifier_helpers;
/// Modifier nodes that handle input.
pub mod nodes;
pub mod text;
pub mod velocity_tracker;

pub use gesture_constants::{DRAG_THRESHOLD, MAX_FLING_VELOCITY, MINIMUM_TOUCH_TARGET_SIZE};
pub use modifier::*;
pub use nodes::input::{
    DEFAULT_ROTARY_SCROLL_FACTOR_DP, Modifiers, PointerButton, PointerButtons, PointerEvent,
    PointerEventKind, PointerId, PointerPhase, PointerSource, RotaryScrollEvent,
    RotaryStepAccumulator, rotary_scroll_pixels_from_detents,
};
pub use velocity_tracker::VelocityTracker1D;

/// The traits, macros and types needed to implement a modifier node.
pub mod prelude {
    pub use crate::{
        impl_draw_node, impl_focus_node, impl_modifier_node, impl_pointer_input_node,
        impl_semantics_node,
        modifier::{
            BasicModifierNodeContext, Constraints, DrawModifierNode, InvalidationKind,
            LayoutModifierNode, Measurable, ModifierNode, ModifierNodeChain, ModifierNodeContext,
            ModifierNodeElement, PointerInputNode, SemanticsNode, Size,
        },
        nodes::input::prelude::*,
    };
}
