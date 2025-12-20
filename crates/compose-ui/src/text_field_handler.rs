//! Text field handler for O(1) focus dispatch.
//!
//! This module provides `TextFieldHandler` which implements `FocusedTextFieldHandler`
//! to enable O(1) keyboard and clipboard event dispatch to the focused text field,
//! avoiding O(N) tree scans.

use compose_foundation::text::{TextFieldLineLimits, TextFieldState};
use std::rc::Rc;

use crate::text_field_input::handle_key_event_impl;

/// Handler wrapper for O(1) focus dispatch.
/// Implements FocusedTextFieldHandler by delegating to TextFieldState operations.
pub(crate) struct TextFieldHandler {
    state: TextFieldState,
    /// Node ID for scoped layout invalidation (avoids O(app size) global invalidation)
    node_id: Option<compose_core::NodeId>,
    /// Line limits configuration
    line_limits: TextFieldLineLimits,
}

impl TextFieldHandler {
    pub(crate) fn new(
        state: TextFieldState, 
        node_id: Option<compose_core::NodeId>,
        line_limits: TextFieldLineLimits,
    ) -> Rc<Self> {
        Rc::new(Self { state, node_id, line_limits })
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
                buffer.set_composition(None);
            } else {
                // Set composition text and range
                // The composition range is relative to where the IME text will be inserted
                let insert_pos = buffer.selection().min();
                let comp_end = insert_pos + text.len();
                
                // Replace current selection with composition text
                buffer.replace(buffer.selection(), text);
                
                // Set composition range to highlight the preedit text
                let comp_range = compose_foundation::text::TextRange::new(insert_pos, comp_end);
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
