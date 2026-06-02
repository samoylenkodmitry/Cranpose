//! Focus manager for text fields.
//!
//! This module tracks which text field currently has focus, ensuring only one
//! text field is focused at a time. When a new field requests focus, the
//! previously focused field is automatically unfocused.
//!
//! O(1) key dispatch: The focused field's handler is stored for direct invocation,
//! avoiding O(N) tree scans on every keystroke.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use crate::key_event::KeyEvent;

/// Handler trait for focused text field operations.
/// Stored in focus module for O(1) key/clipboard dispatch.
pub trait FocusedTextFieldHandler {
    /// Handle a key event. Returns true if consumed.
    fn handle_key(&self, event: &KeyEvent) -> bool;
    /// Insert pasted text.
    fn insert_text(&self, text: &str);
    /// Delete text surrounding the cursor or selection.
    fn delete_surrounding(&self, before_bytes: usize, after_bytes: usize);
    /// Copy current selection. Returns None if nothing selected.
    fn copy_selection(&self) -> Option<String>;
    /// Cut current selection (copy + delete). Returns None if nothing selected.
    fn cut_selection(&self) -> Option<String>;
    /// Set IME composition (preedit) state.
    /// - `text`: The composition text being typed (empty string to clear)
    /// - `cursor`: Optional cursor position within composition (start, end)
    fn set_composition(&self, text: &str, cursor: Option<(usize, usize)>);
}

pub(crate) struct TextFieldFocusState {
    focused_field: RefCell<Option<Weak<RefCell<bool>>>>,
    focused_handler: RefCell<Option<Rc<dyn FocusedTextFieldHandler>>>,
}

impl TextFieldFocusState {
    pub(crate) fn new() -> Self {
        Self {
            focused_field: RefCell::new(None),
            focused_handler: RefCell::new(None),
        }
    }

    fn request_focus(
        &self,
        is_focused: Rc<RefCell<bool>>,
        handler: Rc<dyn FocusedTextFieldHandler>,
    ) {
        let mut current = self.focused_field.borrow_mut();

        if let Some(ref weak) = *current {
            if let Some(old_focused) = weak.upgrade() {
                *old_focused.borrow_mut() = false;
            }
        }

        *is_focused.borrow_mut() = true;
        *current = Some(Rc::downgrade(&is_focused));
        *self.focused_handler.borrow_mut() = Some(handler);
    }

    fn clear_focus(&self) {
        let mut current = self.focused_field.borrow_mut();

        if let Some(ref weak) = *current {
            if let Some(focused) = weak.upgrade() {
                *focused.borrow_mut() = false;
            }
        }

        *current = None;
        *self.focused_handler.borrow_mut() = None;
    }

    fn has_focused_field(&self) -> bool {
        let mut current = self.focused_field.borrow_mut();
        if let Some(ref weak) = *current {
            if weak.upgrade().is_some() {
                return true;
            }
            *current = None;
            *self.focused_handler.borrow_mut() = None;
            crate::cursor_animation::stop_cursor_blink();
        }
        false
    }

    fn focused_handler(&self) -> Option<Rc<dyn FocusedTextFieldHandler>> {
        if !self.has_focused_field() {
            return None;
        }
        self.focused_handler.borrow().as_ref().cloned()
    }

    fn dispatch_key_event(&self, event: &KeyEvent) -> bool {
        if let Some(handler) = self.focused_handler() {
            handler.handle_key(event)
        } else {
            false
        }
    }

    fn dispatch_paste(&self, text: &str) -> bool {
        if let Some(handler) = self.focused_handler() {
            handler.insert_text(text);
            true
        } else {
            false
        }
    }

    fn dispatch_delete_surrounding(&self, before_bytes: usize, after_bytes: usize) -> bool {
        if let Some(handler) = self.focused_handler() {
            handler.delete_surrounding(before_bytes, after_bytes);
            true
        } else {
            false
        }
    }

    fn dispatch_copy(&self) -> Option<String> {
        self.focused_handler()
            .and_then(|handler| handler.copy_selection())
    }

    fn dispatch_cut(&self) -> Option<String> {
        self.focused_handler()
            .and_then(|handler| handler.cut_selection())
    }

    fn dispatch_ime_preedit(&self, text: &str, cursor: Option<(usize, usize)>) -> bool {
        if let Some(handler) = self.focused_handler() {
            handler.set_composition(text, cursor);
            true
        } else {
            false
        }
    }
}

/// Requests focus for a text field.
///
/// If another text field was previously focused, it will be unfocused first.
/// The provided `is_focused` handle should be the field's focus state.
/// The handler is stored for O(1) key dispatch.
pub fn request_focus(is_focused: Rc<RefCell<bool>>, handler: Rc<dyn FocusedTextFieldHandler>) {
    crate::render_state::with_text_field_focus(|state| state.request_focus(is_focused, handler));

    // Start cursor blink animation (timer-based, not continuous redraw)
    crate::cursor_animation::start_cursor_blink();

    // Only render invalidation needed - cursor is drawn via create_draw_closure()
    // which checks focus at draw time. No layout change occurs on focus.
    crate::request_render_invalidation();
}

/// Clears focus from the currently focused text field.
pub fn clear_focus() {
    crate::render_state::with_text_field_focus(|state| state.clear_focus());

    // Stop cursor blink animation
    crate::cursor_animation::stop_cursor_blink();

    crate::request_render_invalidation();
}

/// Returns true if any text field currently has focus.
/// Checks weak ref liveness and clears stale focus state.
pub fn has_focused_field() -> bool {
    crate::render_state::with_text_field_focus(|state| state.has_focused_field())
}

// ============================================================================
// O(1) Dispatch Functions - Bypass tree scan by using stored handler
// ============================================================================

/// Dispatches a key event to the focused text field. Returns true if consumed.
/// O(1) operation using stored handler.
pub fn dispatch_key_event(event: &KeyEvent) -> bool {
    crate::render_state::with_text_field_focus(|state| state.dispatch_key_event(event))
}

/// Inserts text into the focused text field (paste operation).
/// O(1) operation using stored handler.
pub fn dispatch_paste(text: &str) -> bool {
    crate::render_state::with_text_field_focus(|state| state.dispatch_paste(text))
}

/// Deletes text surrounding the cursor or selection.
/// O(1) operation using stored handler.
pub fn dispatch_delete_surrounding(before_bytes: usize, after_bytes: usize) -> bool {
    crate::render_state::with_text_field_focus(|state| {
        state.dispatch_delete_surrounding(before_bytes, after_bytes)
    })
}

/// Copies selection from focused text field.
/// O(1) operation using stored handler.
pub fn dispatch_copy() -> Option<String> {
    crate::render_state::with_text_field_focus(|state| state.dispatch_copy())
}

/// Cuts selection from focused text field (copy + delete).
/// O(1) operation using stored handler.
pub fn dispatch_cut() -> Option<String> {
    crate::render_state::with_text_field_focus(|state| state.dispatch_cut())
}

/// Dispatches IME preedit (composition) state to the focused text field.
/// O(1) operation using stored handler.
/// Returns true if a text field was focused and received the event.
pub fn dispatch_ime_preedit(text: &str, cursor: Option<(usize, usize)>) -> bool {
    crate::render_state::with_text_field_focus(|state| state.dispatch_ime_preedit(text, cursor))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    // Mock handler for testing
    struct MockHandler;
    impl FocusedTextFieldHandler for MockHandler {
        fn handle_key(&self, _: &KeyEvent) -> bool {
            false
        }
        fn insert_text(&self, _: &str) {}
        fn delete_surrounding(&self, _: usize, _: usize) {}
        fn copy_selection(&self) -> Option<String> {
            None
        }
        fn cut_selection(&self) -> Option<String> {
            None
        }
        fn set_composition(&self, _: &str, _: Option<(usize, usize)>) {}
    }

    fn mock_handler() -> Rc<dyn FocusedTextFieldHandler> {
        Rc::new(MockHandler)
    }

    #[test]
    fn request_focus_sets_flag() {
        let _app_context = crate::render_state::app_context_test_scope();
        let focus = Rc::new(RefCell::new(false));
        request_focus(focus.clone(), mock_handler());
        assert!(*focus.borrow());
        clear_focus();
    }

    #[test]
    fn request_focus_clears_previous() {
        let _app_context = crate::render_state::app_context_test_scope();
        let focus1 = Rc::new(RefCell::new(false));
        let focus2 = Rc::new(RefCell::new(false));

        request_focus(focus1.clone(), mock_handler());
        assert!(*focus1.borrow());

        request_focus(focus2.clone(), mock_handler());
        assert!(!*focus1.borrow()); // First should be unfocused
        assert!(*focus2.borrow()); // Second should be focused
        clear_focus();
    }

    #[test]
    fn clear_focus_unfocuses_current() {
        let _app_context = crate::render_state::app_context_test_scope();
        let focus = Rc::new(RefCell::new(false));
        request_focus(focus.clone(), mock_handler());
        assert!(*focus.borrow());

        clear_focus();
        assert!(!*focus.borrow());
    }

    #[derive(Default)]
    struct DispatchRecordingHandler {
        key_count: Cell<usize>,
        insert_count: Cell<usize>,
        delete_count: Cell<usize>,
        copy_count: Cell<usize>,
        cut_count: Cell<usize>,
        preedit_count: Cell<usize>,
        last_delete: Cell<Option<(usize, usize)>>,
    }

    impl DispatchRecordingHandler {
        fn bump(cell: &Cell<usize>) {
            cell.set(cell.get() + 1);
        }

        fn total_calls(&self) -> usize {
            self.key_count.get()
                + self.insert_count.get()
                + self.delete_count.get()
                + self.copy_count.get()
                + self.cut_count.get()
                + self.preedit_count.get()
        }
    }

    impl FocusedTextFieldHandler for DispatchRecordingHandler {
        fn handle_key(&self, _: &KeyEvent) -> bool {
            Self::bump(&self.key_count);
            true
        }

        fn insert_text(&self, _: &str) {
            Self::bump(&self.insert_count);
        }

        fn delete_surrounding(&self, before_bytes: usize, after_bytes: usize) {
            Self::bump(&self.delete_count);
            self.last_delete.set(Some((before_bytes, after_bytes)));
        }

        fn copy_selection(&self) -> Option<String> {
            Self::bump(&self.copy_count);
            Some("copy".to_string())
        }

        fn cut_selection(&self) -> Option<String> {
            Self::bump(&self.cut_count);
            Some("cut".to_string())
        }

        fn set_composition(&self, _: &str, _: Option<(usize, usize)>) {
            Self::bump(&self.preedit_count);
        }
    }

    #[test]
    fn dispatch_delete_surrounding_calls_handler() {
        let _app_context = crate::render_state::app_context_test_scope();
        let focus = Rc::new(RefCell::new(false));
        let handler = Rc::new(DispatchRecordingHandler::default());

        request_focus(Rc::clone(&focus), handler.clone());
        assert!(dispatch_delete_surrounding(3, 1));
        assert_eq!(handler.last_delete.get(), Some((3, 1)));

        clear_focus();
    }

    #[test]
    fn dispatch_clears_stale_focus_owner_before_invoking_handler() {
        let _app_context = crate::render_state::app_context_test_scope();
        let handler = Rc::new(DispatchRecordingHandler::default());

        {
            let focus = Rc::new(RefCell::new(false));
            request_focus(Rc::clone(&focus), handler.clone());
            assert!(has_focused_field());
        }

        let key_event = KeyEvent::key_down(crate::key_event::KeyCode::A, "a");

        assert!(!dispatch_key_event(&key_event));
        assert!(!dispatch_paste("stale paste"));
        assert!(!dispatch_delete_surrounding(2, 1));
        assert_eq!(dispatch_copy(), None);
        assert_eq!(dispatch_cut(), None);
        assert!(!dispatch_ime_preedit("preedit", Some((1, 1))));
        assert!(!has_focused_field());
        assert_eq!(
            handler.total_calls(),
            0,
            "stale focused-field handlers must not receive input"
        );
    }

    #[test]
    fn text_field_focus_is_scoped_by_app_context() {
        let _app_context = crate::render_state::app_context_test_scope();
        let first = crate::render_state::AppContext::new_with_density(1.0);
        let second = crate::render_state::AppContext::new_with_density(1.0);
        let first_focus = Rc::new(RefCell::new(false));
        let second_focus = Rc::new(RefCell::new(false));

        first.enter(|| {
            request_focus(first_focus.clone(), mock_handler());
            assert!(has_focused_field());
            assert!(*first_focus.borrow());
        });

        second.enter(|| {
            assert!(!has_focused_field());
            request_focus(second_focus.clone(), mock_handler());
            assert!(has_focused_field());
            assert!(*second_focus.borrow());
        });

        first.enter(|| {
            assert!(has_focused_field());
            assert!(*first_focus.borrow());
            clear_focus();
            assert!(!has_focused_field());
            assert!(!*first_focus.borrow());
        });

        second.enter(|| {
            assert!(has_focused_field());
            assert!(*second_focus.borrow());
            clear_focus();
        });
    }
}
