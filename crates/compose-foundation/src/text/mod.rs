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
mod line_limits;
mod range;
mod state;

pub use buffer::TextFieldBuffer;
pub use line_limits::{filter_for_single_line, TextFieldLineLimits};
pub use range::TextRange;
pub use state::{TextFieldState, TextFieldValue};
