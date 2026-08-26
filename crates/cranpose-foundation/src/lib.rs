//! Foundation elements for Cranpose: modifiers, input, and core functionality

#![allow(non_snake_case)]

pub mod gesture_constants;
pub mod lazy;
pub mod modifier;
pub mod modifier_helpers;
pub mod nodes;
pub mod text;
pub mod velocity_tracker;

// Re-export gesture constants at crate root for convenience
pub use gesture_constants::{DRAG_THRESHOLD, MAX_FLING_VELOCITY};
pub use modifier::*;
#[allow(unused_imports)]
pub use modifier_helpers::*;
pub use nodes::input::{
    DEFAULT_ROTARY_SCROLL_FACTOR_DP, Modifiers, PointerButton, PointerButtons, PointerEvent,
    PointerEventKind, PointerId, PointerPhase, PointerSource, RotaryScrollEvent,
    RotaryStepAccumulator, rotary_scroll_pixels_from_detents,
};
pub use velocity_tracker::VelocityTracker1D;

pub mod prelude {
    #[allow(unused_imports)]
    pub use crate::modifier_helpers::*;
    // Re-export the helper macros for convenience
    pub use crate::{
        impl_draw_node, impl_focus_node, impl_modifier_node, impl_pointer_input_node,
        impl_semantics_node,
    };
    pub use crate::{
        modifier::{
            BasicModifierNodeContext, Constraints, DrawModifierNode, InvalidationKind,
            LayoutModifierNode, Measurable, ModifierNode, ModifierNodeChain, ModifierNodeContext,
            ModifierNodeElement, PointerInputNode, SemanticsNode, Size,
        },
        nodes::input::prelude::*,
    };
}
