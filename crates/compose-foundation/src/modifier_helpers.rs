//! Helper macros to reduce boilerplate when implementing modifier nodes.
//!
//! These helpers follow the Jetpack Compose pattern where setting capability
//! bits + implementing the specialized trait is enough for the node to participate
//! in the corresponding pipeline stage.

/// Implements the as_draw_node methods for a type that implements DrawModifierNode.
///
/// # Example
///
/// ```text
/// impl ModifierNode for MyNode {
///     impl_draw_node!();
/// }
/// ```
#[macro_export]
macro_rules! impl_draw_node {
    () => {
        fn as_draw_node(&self) -> Option<&dyn $crate::DrawModifierNode> {
            Some(self)
        }

        fn as_draw_node_mut(&mut self) -> Option<&mut dyn $crate::DrawModifierNode> {
            Some(self)
        }
    };
}

/// Implements the as_pointer_input_node methods for a type that implements PointerInputNode.
///
/// # Example
///
/// ```text
/// impl ModifierNode for MyNode {
///     impl_pointer_input_node!();
/// }
/// ```
#[macro_export]
macro_rules! impl_pointer_input_node {
    () => {
        fn as_pointer_input_node(&self) -> Option<&dyn $crate::PointerInputNode> {
            Some(self)
        }

        fn as_pointer_input_node_mut(&mut self) -> Option<&mut dyn $crate::PointerInputNode> {
            Some(self)
        }
    };
}

/// Implements the as_semantics_node methods for a type that implements SemanticsNode.
///
/// # Example
///
/// ```text
/// impl ModifierNode for MyNode {
///     impl_semantics_node!();
/// }
/// ```
#[macro_export]
macro_rules! impl_semantics_node {
    () => {
        fn as_semantics_node(&self) -> Option<&dyn $crate::SemanticsNode> {
            Some(self)
        }

        fn as_semantics_node_mut(&mut self) -> Option<&mut dyn $crate::SemanticsNode> {
            Some(self)
        }
    };
}

/// Implements the as_focus_node methods for a type that implements FocusNode.
///
/// # Example
///
/// ```text
/// impl ModifierNode for MyNode {
///     impl_focus_node!();
/// }
/// ```
#[macro_export]
macro_rules! impl_focus_node {
    () => {
        fn as_focus_node(&self) -> Option<&dyn $crate::FocusNode> {
            Some(self)
        }

        fn as_focus_node_mut(&mut self) -> Option<&mut dyn $crate::FocusNode> {
            Some(self)
        }
    };
}

/// Comprehensive macro that implements all capability-based methods for a modifier node.
///
/// This macro reduces boilerplate by automatically implementing the as_* methods
/// for all specialized traits that the type implements. Use this as the primary
/// way to declare which traits your node implements.
///
/// # Example
///
/// ```text
/// impl ModifierNode for MyDrawNode {
///     impl_modifier_node!(draw);
/// }
///
/// impl ModifierNode for MyPointerNode {
///     impl_modifier_node!(pointer_input);
/// }
///
/// impl ModifierNode for MyComplexNode {
///     impl_modifier_node!(draw, pointer_input, semantics);
/// }
/// ```
#[macro_export]
macro_rules! impl_modifier_node {
    (draw) => {
        $crate::impl_draw_node!();
    };
    (pointer_input) => {
        $crate::impl_pointer_input_node!();
    };
    (semantics) => {
        $crate::impl_semantics_node!();
    };
    (focus) => {
        $crate::impl_focus_node!();
    };
    (draw, $($rest:tt)*) => {
        $crate::impl_draw_node!();
        $crate::impl_modifier_node!($($rest)*);
    };
    (pointer_input, $($rest:tt)*) => {
        $crate::impl_pointer_input_node!();
        $crate::impl_modifier_node!($($rest)*);
    };
    (semantics, $($rest:tt)*) => {
        $crate::impl_semantics_node!();
        $crate::impl_modifier_node!($($rest)*);
    };
    (focus, $($rest:tt)*) => {
        $crate::impl_focus_node!();
        $crate::impl_modifier_node!($($rest)*);
    };
}
