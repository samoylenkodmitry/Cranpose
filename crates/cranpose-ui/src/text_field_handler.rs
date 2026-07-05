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

    /// Invalidations every text mutation needs.
    ///
    /// A text change can grow or shrink the field (wrapped lines added/removed),
    /// so the field must re-measure, not just redraw. The scoped
    /// `schedule_layout_repass` re-measures the field's subtree on the same
    /// frame the input is dispatched; without it a field only relayouts when
    /// something else forces a full rebuild (e.g. focus loss), which is why
    /// IME-composed edits appeared to "stick" until the field was blurred.
    fn on_text_mutated(&self) {
        crate::cursor_animation::reset_cursor_blink();
        // Scoped O(subtree) relayout instead of O(app size) global invalidation.
        if let Some(node_id) = self.node_id {
            crate::schedule_layout_repass(node_id);
        }
        crate::request_render_invalidation();
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
            self.on_text_mutated();
        }

        consumed
    }

    fn insert_text(&self, text: &str) {
        self.state.edit(|buffer| {
            buffer.insert(text);
        });
        self.on_text_mutated();
    }

    fn delete_surrounding(&self, before_bytes: usize, after_bytes: usize) {
        if before_bytes == 0 && after_bytes == 0 {
            return;
        }

        self.state.edit(|buffer| {
            buffer.delete_surrounding(before_bytes, after_bytes);
        });
        self.state.set_desired_column(None);
        self.on_text_mutated();
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
        self.on_text_mutated();
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
                // Successive preedit updates must *replace* the previous
                // composing text (IMEs resend the whole preedit on every
                // keystroke); without an active composition the preedit
                // replaces the current selection, like Android's
                // `setComposingText` and winit's `Ime::Preedit`.
                let target = buffer.composition().unwrap_or_else(|| buffer.selection());
                let insert_pos = target.min();
                let comp_end = insert_pos + text.len();

                buffer.replace(target, text);

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

        // Preedit changes the text (grows/shrinks the field), so it needs a
        // relayout, not just a redraw of the composition underline.
        self.on_text_mutated();
    }

    fn finish_composition(&self) {
        // Android `finishComposingText`: keep the composed text as committed
        // text, only drop the composing region (unlike `set_composition("")`,
        // which deletes the preedit text).
        if self.state.composition().is_none() {
            return;
        }
        self.state.edit(|buffer| {
            buffer.set_composition(None);
        });
        crate::request_render_invalidation();
    }

    fn set_composing_region(&self, start_bytes: usize, end_bytes: usize) {
        self.state.edit(|buffer| {
            let text = buffer.text();
            let start = floor_char_boundary(text, start_bytes.min(end_bytes));
            let end = floor_char_boundary(text, start_bytes.max(end_bytes));
            if start == end {
                // An empty region clears the composing state (keeping the
                // text), matching Android's setComposingRegion contract.
                buffer.set_composition(None);
            } else {
                buffer.set_composition(Some(cranpose_foundation::text::TextRange::new(start, end)));
            }
        });
        crate::request_render_invalidation();
    }

    fn editor_state(&self) -> Option<crate::text_field_focus::ImeEditorState> {
        let value = self.state.value();
        Some(crate::text_field_focus::ImeEditorState {
            text: value.text.clone(),
            selection_start: value.selection.min(),
            selection_end: value.selection.max(),
            composition: value.composition.map(|range| (range.min(), range.max())),
            single_line: matches!(self.line_limits, TextFieldLineLimits::SingleLine),
        })
    }
}

/// Largest byte index `<= index` that lies on a `char` boundary of `text`.
fn floor_char_boundary(text: &str, index: usize) -> usize {
    let mut index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
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

    /// IMEs resend the whole preedit on every keystroke; successive preedit
    /// updates must replace the previous composing text, not accumulate.
    #[test]
    fn successive_preedit_updates_replace_previous_composition() {
        let _app_context = crate::render_state::app_context_test_scope();
        with_test_runtime(|| {
            let (state, _focus) = focused_state("ab", TextFieldLineLimits::SingleLine);
            state.edit(|buffer| buffer.place_cursor_at_end());

            assert!(text_field_focus::dispatch_ime_preedit("k", Some((1, 1))));
            assert_eq!(state.text(), "abk");
            assert!(text_field_focus::dispatch_ime_preedit("ka", Some((2, 2))));
            assert_eq!(state.text(), "abka");
            assert!(text_field_focus::dispatch_ime_preedit(
                "\u{304b}",
                Some((3, 3))
            ));
            assert_eq!(state.text(), "ab\u{304b}");
            let comp = state.composition().expect("composition active");
            assert_eq!((comp.min(), comp.max()), (2, 2 + "\u{304b}".len()));

            // Commit path: clear preedit (deletes it), then insert the final text.
            assert!(text_field_focus::dispatch_ime_preedit("", None));
            assert_eq!(state.text(), "ab");
            assert!(text_field_focus::dispatch_paste("\u{304b}\u{306a}"));
            assert_eq!(state.text(), "ab\u{304b}\u{306a}");
            assert_eq!(state.composition(), None);

            text_field_focus::clear_focus();
        });
    }

    /// `finishComposingText` keeps the composed text and only clears the
    /// composing region, unlike an empty preedit which deletes the text.
    #[test]
    fn finish_composition_keeps_text() {
        let _app_context = crate::render_state::app_context_test_scope();
        with_test_runtime(|| {
            let (state, _focus) = focused_state("", TextFieldLineLimits::SingleLine);

            assert!(text_field_focus::dispatch_ime_preedit("hello", None));
            assert_eq!(state.text(), "hello");
            assert!(state.composition().is_some());

            assert!(text_field_focus::dispatch_ime_finish_composing());
            assert_eq!(state.text(), "hello");
            assert_eq!(state.composition(), None);

            // Finishing again is a no-op.
            assert!(text_field_focus::dispatch_ime_finish_composing());
            assert_eq!(state.text(), "hello");

            text_field_focus::clear_focus();
        });
    }

    /// Autocorrect re-composes an already committed word via
    /// `setComposingRegion`; the text must not change, only the region.
    #[test]
    fn set_composing_region_marks_text_without_changing_it() {
        let _app_context = crate::render_state::app_context_test_scope();
        with_test_runtime(|| {
            let (state, _focus) = focused_state("hello world", TextFieldLineLimits::SingleLine);

            assert!(text_field_focus::dispatch_ime_set_composing_region(0, 5));
            assert_eq!(state.text(), "hello world");
            let comp = state.composition().expect("composition active");
            assert_eq!((comp.min(), comp.max()), (0, 5));

            // Replacing the composing region rewrites the marked word.
            assert!(text_field_focus::dispatch_ime_preedit("Hello", None));
            assert_eq!(state.text(), "Hello world");

            // Offsets that fall inside a multi-byte character are clamped to
            // the previous boundary instead of panicking.
            assert!(text_field_focus::dispatch_ime_preedit("", None));
            state.edit(|buffer| {
                buffer.clear();
                buffer.insert("\u{00e9}x");
            });
            assert!(text_field_focus::dispatch_ime_set_composing_region(1, 3));
            let comp = state.composition().expect("composition active");
            assert_eq!((comp.min(), comp.max()), (0, 3));

            // An empty region clears composing state but keeps the text.
            assert!(text_field_focus::dispatch_ime_set_composing_region(1, 1));
            assert_eq!(state.composition(), None);
            assert_eq!(state.text(), "\u{00e9}x");

            text_field_focus::clear_focus();
        });
    }

    #[test]
    fn editor_state_reflects_text_selection_and_composition() {
        let _app_context = crate::render_state::app_context_test_scope();
        with_test_runtime(|| {
            let (state, _focus) = focused_state("hi", TextFieldLineLimits::SingleLine);
            state.edit(|buffer| buffer.place_cursor_at_end());

            let snapshot = text_field_focus::focused_editor_state().expect("focused field");
            assert_eq!(snapshot.text, "hi");
            assert_eq!(
                (snapshot.selection_start, snapshot.selection_end),
                (2usize, 2usize)
            );
            assert_eq!(snapshot.composition, None);
            assert!(snapshot.single_line);

            assert!(text_field_focus::dispatch_ime_preedit("ab", None));
            let snapshot = text_field_focus::focused_editor_state().expect("focused field");
            assert_eq!(snapshot.text, "hiab");
            assert_eq!(snapshot.composition, Some((2, 4)));

            text_field_focus::clear_focus();
            assert_eq!(text_field_focus::focused_editor_state(), None);
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
