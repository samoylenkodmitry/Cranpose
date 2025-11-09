//! Foundation elements for Compose-RS: modifiers, input, and core functionality

#![allow(non_snake_case)]

pub mod modifier;
pub mod nodes;

// Re-export commonly used items
pub use modifier::*;
pub use nodes::input::{
    PointerButton, PointerButtons, PointerEvent, PointerEventKind, PointerId, PointerPhase,
};

pub mod prelude {
    pub use crate::modifier::{
        BasicModifierNodeContext, Constraints, DrawModifierNode, InvalidationKind,
        LayoutModifierNode, Measurable, ModifierElement, ModifierNode, ModifierNodeChain,
        ModifierNodeContext, ModifierNodeElement, PointerInputNode, SemanticsNode, Size,
    };
    pub use crate::nodes::input::prelude::*;
}
