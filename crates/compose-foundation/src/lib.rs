//! Foundation elements for Compose-RS: modifiers, input, and core functionality

#![allow(non_snake_case)]

pub mod modifier;
pub mod modifier_helpers;
pub mod measurement_proxy;
pub mod nodes;

// Re-export commonly used items
pub use modifier::*;
pub use measurement_proxy::*;
pub use nodes::input::{
    PointerButton, PointerButtons, PointerEvent, PointerEventKind, PointerId, PointerPhase,
};

pub mod prelude {
    pub use crate::modifier::{
        BasicModifierNodeContext, Constraints, DrawModifierNode, InvalidationKind,
        LayoutModifierNode, Measurable, ModifierElement, ModifierNode, ModifierNodeChain,
        ModifierNodeContext, ModifierNodeElement, PointerInputNode, SemanticsNode, Size,
    };
    pub use crate::measurement_proxy::*;
    
    pub use crate::nodes::input::prelude::*;
    // Re-export the helper macros for convenience
    pub use crate::{
        impl_draw_node, impl_focus_node, impl_modifier_node, impl_pointer_input_node,
        impl_semantics_node,
    };
}
