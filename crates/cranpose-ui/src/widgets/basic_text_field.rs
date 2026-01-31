//! BasicTextField widget for editable text input.
//!
//! This module provides the `BasicTextField` composable following Jetpack Compose's
//! `BasicTextField` pattern from `compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/text/BasicTextField.kt`.

#![allow(non_snake_case)]

use crate::composable;
use crate::layout::policies::EmptyMeasurePolicy;
use crate::modifier::Modifier;
use crate::text_field_modifier_node::TextFieldElement;
use crate::widgets::Layout;
use cranpose_core::NodeId;
use cranpose_foundation::modifier_element;
use cranpose_foundation::text::{TextFieldLineLimits, TextFieldState};
use cranpose_ui_graphics::Color;

/// A primitive editable text field.
///
/// # When to use
/// Use this when you need an editable text input but want full control over the
/// styling (no built-in borders or labels).
///
/// # Arguments
///
/// * `state` - The observable text field state that holds text content and cursor position.
/// * `modifier` - Modifiers for styling and layout.
///
/// # Example
///
/// ```rust,ignore
/// let text = remember_text_field_state("Initial text");
/// BasicTextField(text, Modifier::padding(8.0));
/// ```
#[composable]
pub fn BasicTextField(state: TextFieldState, modifier: Modifier) -> NodeId {
    BasicTextFieldWithOptions(state, modifier, BasicTextFieldOptions::default())
}

/// Options for customizing BasicTextField appearance and behavior.
#[derive(Debug, Clone, PartialEq)]
pub struct BasicTextFieldOptions {
    /// Cursor color
    pub cursor_color: Color,
    /// Line limits: SingleLine or MultiLine with optional min/max
    pub line_limits: TextFieldLineLimits,
}

impl Default for BasicTextFieldOptions {
    fn default() -> Self {
        Self {
            cursor_color: Color(0.0, 0.0, 0.0, 1.0), // Black
            line_limits: TextFieldLineLimits::default(),
        }
    }
}

/// Creates an editable text field with custom options.
///
/// This is the full version of `BasicTextField` with all configuration options.
#[composable]
pub fn BasicTextFieldWithOptions(
    state: TextFieldState,
    modifier: Modifier,
    options: BasicTextFieldOptions,
) -> NodeId {
    // Read text to create composition dependency.
    // TextFieldState now uses mutableStateOf internally, so this read
    // automatically creates composition dependency via the snapshot system.
    let _text = state.text();

    // Build the text field element with line limits
    let text_field_element = TextFieldElement::new(state)
        .with_cursor_color(options.cursor_color)
        .with_line_limits(options.line_limits);

    // Wrap it in a modifier
    let text_field_modifier = modifier_element(text_field_element);
    let final_modifier = Modifier::from_parts(vec![text_field_modifier]);
    let combined_modifier = modifier.then(final_modifier);

    // Use EmptyMeasurePolicy - TextFieldModifierNode handles all measurement
    // This matches Jetpack Compose's BasicTextField architecture
    Layout(
        combined_modifier,
        EmptyMeasurePolicy,
        || {}, // No children
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranpose_core::{location_key, Composition, DefaultScheduler, MemoryApplier, Runtime};
    use std::sync::Arc;

    /// Sets up a test runtime and keeps it alive for the duration of the test.
    fn with_test_runtime<T>(f: impl FnOnce() -> T) -> T {
        let _runtime = Runtime::new(Arc::new(DefaultScheduler));
        f()
    }

    #[test]
    fn basic_text_field_creates_node() {
        let mut composition = Composition::new(MemoryApplier::new());
        let state = TextFieldState::new("Test content");

        let result = composition.render(location_key(file!(), line!(), column!()), {
            let state = state.clone();
            move || {
                BasicTextField(state.clone(), Modifier::empty());
            }
        });

        assert!(result.is_ok());
        assert!(composition.root().is_some());
    }

    #[test]
    fn basic_text_field_state_updates() {
        with_test_runtime(|| {
            let state = TextFieldState::new("Hello");
            assert_eq!(state.text(), "Hello");

            state.edit(|buffer| {
                buffer.place_cursor_at_end();
                buffer.insert("!");
            });

            assert_eq!(state.text(), "Hello!");
        });
    }
}
