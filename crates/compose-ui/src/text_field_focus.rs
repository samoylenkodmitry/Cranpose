//! Global focus manager for text fields.
//!
//! This module tracks which text field currently has focus, ensuring only one
//! text field is focused at a time. When a new field requests focus, the
//! previously focused field is automatically unfocused.
//!
//! O(1) key dispatch: The focused field's handler is stored for direct invocation,
//! avoiding O(N) tree scans on every keystroke.
//!
//! ARCHITECTURE: Uses thread-local storage as the single source of truth for focus
//! state. This is correct for single-threaded UI frameworks like this one.

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
    /// Copy current selection. Returns None if nothing selected.
    fn copy_selection(&self) -> Option<String>;
    /// Cut current selection (copy + delete). Returns None if nothing selected.
    fn cut_selection(&self) -> Option<String>;
    /// Set IME composition (preedit) state.
    /// - `text`: The composition text being typed (empty string to clear)
    /// - `cursor`: Optional cursor position within composition (start, end)
    fn set_composition(&self, text: &str, cursor: Option<(usize, usize)>);
}

// Thread-local for focus state - the SINGLE source of truth for focus.
// This is correct for single-threaded UI frameworks.
thread_local! {
    static FOCUSED_FIELD: RefCell<Option<Weak<RefCell<bool>>>> = const { RefCell::new(None) };
    // O(1) handler for key dispatch - avoids tree scan
    static FOCUSED_HANDLER: RefCell<Option<Rc<dyn FocusedTextFieldHandler>>> = const { RefCell::new(None) };
}

/// Requests focus for a text field.
///
/// If another text field was previously focused, it will be unfocused first.
/// The provided `is_focused` handle should be the field's focus state.
/// The handler is stored for O(1) key dispatch.
pub fn request_focus(is_focused: Rc<RefCell<bool>>, handler: Rc<dyn FocusedTextFieldHandler>) {
    FOCUSED_FIELD.with(|current| {
        let mut current = current.borrow_mut();
        
        // Unfocus the previously focused field (if any and still alive)
        if let Some(ref weak) = *current {
            if let Some(old_focused) = weak.upgrade() {
                *old_focused.borrow_mut() = false;
            }
        }
        
        // Set the new field as focused
        *is_focused.borrow_mut() = true;
        *current = Some(Rc::downgrade(&is_focused));
    });
    
    // Store handler for O(1) dispatch
    FOCUSED_HANDLER.with(|h| {
        *h.borrow_mut() = Some(handler);
    });
    
    // Start cursor blink animation (timer-based, not continuous redraw)
    crate::cursor_animation::start_cursor_blink();
    
    // Only render invalidation needed - cursor is drawn via create_draw_closure()
    // which checks focus at draw time. No layout change occurs on focus.
    crate::request_render_invalidation();
}

/// Clears focus from the currently focused text field.
#[allow(dead_code)]
pub fn clear_focus() {
    FOCUSED_FIELD.with(|current| {
        let mut current = current.borrow_mut();
        
        if let Some(ref weak) = *current {
            if let Some(focused) = weak.upgrade() {
                *focused.borrow_mut() = false;
            }
        }
        
        *current = None;
    });
    
    // Clear handler
    FOCUSED_HANDLER.with(|h| {
        *h.borrow_mut() = None;
    });
    
    // Stop cursor blink animation
    crate::cursor_animation::stop_cursor_blink();
    
    crate::request_render_invalidation();
}

/// Returns true if any text field currently has focus.
/// Checks weak ref liveness and clears stale focus state.
pub fn has_focused_field() -> bool {
    FOCUSED_FIELD.with(|current| {
        let mut borrow = current.borrow_mut();
        if let Some(ref weak) = *borrow {
            if weak.upgrade().is_some() {
                return true;
            }
            // Weak ref is dead - clean up to prevent stuck-true
            *borrow = None;
            // Clear handler too
            FOCUSED_HANDLER.with(|h| {
                *h.borrow_mut() = None;
            });
            // Also stop cursor blink since focus is lost
            crate::cursor_animation::stop_cursor_blink();
        }
        false
    })
}

// ============================================================================
// O(1) Dispatch Functions - Bypass tree scan by using stored handler
// ============================================================================

/// Dispatches a key event to the focused text field. Returns true if consumed.
/// O(1) operation using stored handler.
pub fn dispatch_key_event(event: &KeyEvent) -> bool {
    FOCUSED_HANDLER.with(|h| {
        if let Some(handler) = h.borrow().as_ref() {
            handler.handle_key(event)
        } else {
            false
        }
    })
}

/// Inserts text into the focused text field (paste operation).
/// O(1) operation using stored handler.
pub fn dispatch_paste(text: &str) -> bool {
    FOCUSED_HANDLER.with(|h| {
        if let Some(handler) = h.borrow().as_ref() {
            handler.insert_text(text);
            true
        } else {
            false
        }
    })
}

/// Copies selection from focused text field.
/// O(1) operation using stored handler.
pub fn dispatch_copy() -> Option<String> {
    FOCUSED_HANDLER.with(|h| {
        if let Some(handler) = h.borrow().as_ref() {
            handler.copy_selection()
        } else {
            None
        }
    })
}

/// Cuts selection from focused text field (copy + delete).
/// O(1) operation using stored handler.
pub fn dispatch_cut() -> Option<String> {
    FOCUSED_HANDLER.with(|h| {
        if let Some(handler) = h.borrow().as_ref() {
            handler.cut_selection()
        } else {
            None
        }
    })
}

/// Dispatches IME preedit (composition) state to the focused text field.
/// O(1) operation using stored handler.
/// Returns true if a text field was focused and received the event.
pub fn dispatch_ime_preedit(text: &str, cursor: Option<(usize, usize)>) -> bool {
    FOCUSED_HANDLER.with(|h| {
        if let Some(handler) = h.borrow().as_ref() {
            handler.set_composition(text, cursor);
            true
        } else {
            false
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock handler for testing
    struct MockHandler;
    impl FocusedTextFieldHandler for MockHandler {
        fn handle_key(&self, _: &KeyEvent) -> bool { false }
        fn insert_text(&self, _: &str) {}
        fn copy_selection(&self) -> Option<String> { None }
        fn cut_selection(&self) -> Option<String> { None }
        fn set_composition(&self, _: &str, _: Option<(usize, usize)>) {}
    }
    
    fn mock_handler() -> Rc<dyn FocusedTextFieldHandler> {
        Rc::new(MockHandler)
    }

    #[test]
    fn request_focus_sets_flag() {
        let focus = Rc::new(RefCell::new(false));
        request_focus(focus.clone(), mock_handler());
        assert!(*focus.borrow());
    }

    #[test]
    fn request_focus_clears_previous() {
        let focus1 = Rc::new(RefCell::new(false));
        let focus2 = Rc::new(RefCell::new(false));
        
        request_focus(focus1.clone(), mock_handler());
        assert!(*focus1.borrow());
        
        request_focus(focus2.clone(), mock_handler());
        assert!(!*focus1.borrow()); // First should be unfocused
        assert!(*focus2.borrow());  // Second should be focused
    }

    #[test]
    fn clear_focus_unfocuses_current() {
        let focus = Rc::new(RefCell::new(false));
        request_focus(focus.clone(), mock_handler());
        assert!(*focus.borrow());
        
        clear_focus();
        assert!(!*focus.borrow());
    }
}
