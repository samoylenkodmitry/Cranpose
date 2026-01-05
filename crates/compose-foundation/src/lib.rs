//! Foundation elements for Compose-RS: modifiers, input, and core functionality

#![allow(non_snake_case)]

pub mod gesture_constants;
pub mod lazy;
pub mod measurement_proxy;
pub mod modifier;
pub mod modifier_helpers;
pub mod nodes;
pub mod text;

// Re-export gesture constants at crate root for convenience
pub use gesture_constants::DRAG_THRESHOLD;

// Re-export commonly used items
pub use measurement_proxy::*;
pub use modifier::*;
#[allow(unused_imports)] // Module exists for API compatibility
pub use modifier_helpers::*;
pub use nodes::input::{
    PointerButton, PointerButtons, PointerEvent, PointerEventKind, PointerId, PointerPhase,
};

pub mod prelude {
    pub use crate::measurement_proxy::*;
    pub use crate::modifier::{
        BasicModifierNodeContext, Constraints, DrawModifierNode, InvalidationKind,
        LayoutModifierNode, Measurable, ModifierElement, ModifierNode, ModifierNodeChain,
        ModifierNodeContext, ModifierNodeElement, PointerInputNode, SemanticsNode, Size,
    };

    #[allow(unused_imports)] // Helper module exists for API compatibility
    pub use crate::modifier_helpers::*;

    pub use crate::nodes::input::prelude::*;
    // Re-export the helper macros for convenience
    pub use crate::{
        impl_draw_node, impl_focus_node, impl_modifier_node, impl_pointer_input_node,
        impl_semantics_node,
    };
}
