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

mod range;
mod buffer;
mod state;
mod line_limits;

pub use range::TextRange;
pub use buffer::TextFieldBuffer;
pub use state::{TextFieldState, TextFieldValue};
pub use line_limits::{TextFieldLineLimits, filter_for_single_line};

