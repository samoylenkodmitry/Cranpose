//! Text field handler for O(1) focus dispatch.
//!
//! This module provides `TextFieldHandler` which implements `FocusedTextFieldHandler`
//! to enable O(1) keyboard and clipboard event dispatch to the focused text field,
//! avoiding O(N) tree scans.

use cranpose_foundation::text::{TextFieldLineLimits, TextFieldState};
use std::rc::Rc;

use crate::text_field_input::handle_key_event_impl;

/// Handler wrapper for O(1) focus dispatch.
/// Implements FocusedTextFieldHandler by delegating to TextFieldState operations.
pub(crate) struct TextFieldHandler {
    state: TextFieldState,
    /// Node ID for scoped layout invalidation (avoids O(app size) global invalidation)
    node_id: Option<cranpose_core::NodeId>,
    /// Line limits configuration
    line_limits: TextFieldLineLimits,
}

impl TextFieldHandler {
    pub(crate) fn new(
        state: TextFieldState,
        node_id: Option<cranpose_core::NodeId>,
        line_limits: TextFieldLineLimits,
    ) -> Rc<Self> {
        Rc::new(Self {
            state,
            node_id,
            line_limits,
        })
    }
}

impl crate::text_field_focus::FocusedTextFieldHandler for TextFieldHandler {
    fn handle_key(&self, event: &crate::key_event::KeyEvent) -> bool {
        use crate::key_event::KeyEventType;

        // Only handle key-down events
        if event.event_type != KeyEventType::KeyDown {
            return false;
        }

        // Delegate to shared implementation with line limits
        let consumed = handle_key_event_impl(&self.state, event, self.line_limits);

        if consumed {
            crate::cursor_animation::reset_cursor_blink();
            // Use scoped invalidation for O(subtree) instead of O(app size)
            if let Some(node_id) = self.node_id {
                crate::schedule_layout_repass(node_id);
            }
            crate::request_render_invalidation();
        }

        consumed
    }

    fn insert_text(&self, text: &str) {
        self.state.edit(|buffer| {
            buffer.insert(text);
        });
        crate::cursor_animation::reset_cursor_blink();
        // Text content changed - use scoped invalidation for O(subtree)
        if let Some(node_id) = self.node_id {
            crate::schedule_layout_repass(node_id);
        }
        crate::request_render_invalidation();
    }

    fn delete_surrounding(&self, before_bytes: usize, after_bytes: usize) {
        if before_bytes == 0 && after_bytes == 0 {
            return;
        }

        self.state.edit(|buffer| {
            buffer.delete_surrounding(before_bytes, after_bytes);
        });
        self.state.set_desired_column(None);
        crate::cursor_animation::reset_cursor_blink();
        if let Some(node_id) = self.node_id {
            crate::schedule_layout_repass(node_id);
        }
        crate::request_render_invalidation();
    }

    fn copy_selection(&self) -> Option<String> {
        let value = self.state.value();
        let selection = value.selection;

        if selection.collapsed() {
            return None;
        }

        let text = selection.safe_slice(&value.text);
        if text.is_empty() {
            return None;
        }

        Some(text.to_string())
    }

    fn cut_selection(&self) -> Option<String> {
        let value = self.state.value();
        let selection = value.selection;

        if selection.collapsed() {
            return None;
        }

        let text = selection.safe_slice(&value.text);
        if text.is_empty() {
            return None;
        }

        let text = text.to_string();
        self.state.edit(|buffer| {
            buffer.delete(selection);
        });
        crate::cursor_animation::reset_cursor_blink();
        crate::request_render_invalidation();
        Some(text)
    }

    fn set_composition(&self, text: &str, cursor: Option<(usize, usize)>) {
        self.state.edit(|buffer| {
            if text.is_empty() {
                // Clear composition
                if let Some(range) = buffer.composition() {
                    buffer.delete(range);
                }
                buffer.set_composition(None);
            } else {
                // Set composition text and range
                // The composition range is relative to where the IME text will be inserted
                let insert_pos = buffer.selection().min();
                let comp_end = insert_pos + text.len();

                // Replace current selection with composition text
                buffer.replace(buffer.selection(), text);

                // Set composition range to highlight the preedit text
                let comp_range = cranpose_foundation::text::TextRange::new(insert_pos, comp_end);
                buffer.set_composition(Some(comp_range));

                // If cursor position within composition is specified, adjust cursor
                if let Some((cursor_start, _cursor_end)) = cursor {
                    let cursor_pos = insert_pos + cursor_start.min(text.len());
                    buffer.place_cursor_before_char(cursor_pos);
                }
            }
        });

        // Request redraw for composition underline
        crate::request_render_invalidation();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key_event::{KeyCode, KeyEvent, KeyEventType, Modifiers};
    use crate::text_field_focus;
    use cranpose_core::{DefaultScheduler, Runtime};
    use std::cell::RefCell;
    use std::sync::Arc;

    /// Sets up a test runtime and keeps it alive for the duration of the test.
    /// TextFieldState uses MutableState, which requires an active runtime.
    fn with_test_runtime<T>(f: impl FnOnce() -> T) -> T {
        let _runtime = Runtime::new(Arc::new(DefaultScheduler));
        f()
    }

    fn key_down(key_code: KeyCode, text: &str) -> KeyEvent {
        KeyEvent::new(key_code, text, Modifiers::NONE, KeyEventType::KeyDown)
    }

    /// Creates a focused text field state. The returned focus flag must stay
    /// alive for the duration of the test: the focus manager only holds a
    /// weak reference to it.
    fn focused_state(
        initial: &str,
        line_limits: TextFieldLineLimits,
    ) -> (TextFieldState, Rc<RefCell<bool>>) {
        let state = TextFieldState::new(initial);
        let handler = TextFieldHandler::new(state.clone(), None, line_limits);
        let focus = Rc::new(RefCell::new(false));
        text_field_focus::request_focus(focus.clone(), handler);
        (state, focus)
    }

    /// Headless commit path for soft-keyboard input on Android:
    /// `NativeActivity` IMEs without an `InputConnection` deliver characters
    /// as key events whose text comes from `KeyCharacterMap`. Framework key
    /// codes may be `Unknown` for layout-specific keys; the text must still
    /// commit.
    #[test]
    fn android_style_key_events_commit_text() {
        let _app_context = crate::render_state::app_context_test_scope();
        with_test_runtime(|| {
            let (state, _focus) = focused_state(
                "",
                TextFieldLineLimits::MultiLine {
                    min_lines: 1,
                    max_lines: usize::MAX,
                },
            );

            assert!(text_field_focus::dispatch_key_event(&key_down(
                KeyCode::H,
                "h"
            )));
            assert!(text_field_focus::dispatch_key_event(&key_down(
                KeyCode::I,
                "i"
            )));
            // Layout-specific key with no framework key code still commits its
            // KeyCharacterMap character.
            assert!(text_field_focus::dispatch_key_event(&key_down(
                KeyCode::Unknown,
                "\u{00f6}"
            )));
            assert_eq!(state.text(), "hi\u{00f6}");

            // Key-up must not double-commit.
            let key_up = KeyEvent::new(KeyCode::H, "h", Modifiers::NONE, KeyEventType::KeyUp);
            assert!(!text_field_focus::dispatch_key_event(&key_up));
            assert_eq!(state.text(), "hi\u{00f6}");

            // Backspace and Enter arrive without text (control keys).
            assert!(text_field_focus::dispatch_key_event(&key_down(
                KeyCode::Backspace,
                ""
            )));
            assert_eq!(state.text(), "hi");

            assert!(text_field_focus::dispatch_key_event(&key_down(
                KeyCode::Enter,
                ""
            )));
            assert_eq!(state.text(), "hi\n");

            text_field_focus::clear_focus();
        });
    }

    #[test]
    fn enter_on_single_line_field_is_not_consumed() {
        let _app_context = crate::render_state::app_context_test_scope();
        with_test_runtime(|| {
            let (state, _focus) = focused_state("abc", TextFieldLineLimits::SingleLine);

            assert!(!text_field_focus::dispatch_key_event(&key_down(
                KeyCode::Enter,
                ""
            )));
            assert_eq!(state.text(), "abc");

            text_field_focus::clear_focus();
        });
    }
}
