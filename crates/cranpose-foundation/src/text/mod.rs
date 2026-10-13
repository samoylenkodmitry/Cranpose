//! Text input module for editable text fields.
//!
//! This module provides the core types for text editing, following Jetpack Compose's
//! text input architecture from `compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/text/input/`.
//!
//! # Core Types
//!
//! - [`TextRange`] - Represents cursor position or text selection range
//! - [`TextFieldBuffer`] - Mutable buffer for editing text with change tracking
//! - [`TextFieldState`] - Observable state holder for text field content
//! - [`TextFieldLineLimits`] - Controls single-line vs multi-line input
//!
//! # Example
//!
//! ```text
//! let state = TextFieldState::new("Hello");
//! state.edit(|buffer| {
//!     buffer.place_cursor_at_end();
//!     buffer.insert(", World!");
//! });
//! assert_eq!(state.text(), "Hello, World!");
//! ```

mod buffer;
mod graphemes;
mod input_transformation;
mod line_limits;
mod range;
mod state;

pub use buffer::TextFieldBuffer;
pub use graphemes::{floor_grapheme_boundary, next_grapheme_boundary, previous_grapheme_boundary};
pub use input_transformation::InputTransformation;
pub use line_limits::{TextFieldLineLimits, filter_for_single_line};
pub use range::TextRange;
pub use state::{TextFieldState, TextFieldValue};
