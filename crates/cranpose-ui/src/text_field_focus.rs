//! Focus manager for text fields.
//!
//! This module tracks which text field currently has focus, ensuring only one
//! text field is focused at a time. When a new field requests focus, the
//! previously focused field is automatically unfocused.
//!
//! O(1) key dispatch: The focused field's handler is stored for direct invocation,
//! avoiding O(N) tree scans on every keystroke.

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use crate::key_event::KeyEvent;

/// Snapshot of the focused text field's editable state for platform IMEs.
///
/// All offsets are UTF-8 byte offsets into `text`. Platform layers that talk
/// to UTF-16 based IMEs (Android's `InputConnection`, web composition events)
/// convert on their side of the boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImeEditorState {
    /// Full text content of the field.
    pub text: String,
    /// Selection start (min) in bytes. Equal to `selection_end` for a caret.
    pub selection_start: usize,
    /// Selection end (max) in bytes.
    pub selection_end: usize,
    /// Active composing (preedit) region in bytes, if any.
    pub composition: Option<(usize, usize)>,
    /// Whether the field is single-line (platforms use this to pick the
    /// keyboard action, e.g. Android `IME_ACTION_DONE` vs newline).
    pub single_line: bool,
}

/// Window-space caret geometry for the focused field, used by platforms whose
/// native text input positions the caret by coordinates rather than key events
/// (iOS `UITextInput`: spacebar-trackpad cursor movement and tap-to-position).
/// All values are logical pixels in window space.
#[derive(Clone, Debug, PartialEq)]
pub struct ImeCaretGeometry {
    /// Caret x for each character-boundary offset, in text order: entry `k` is
    /// the caret after `k` characters, so `caret_xs.len() == chars + 1`.
    pub caret_xs: Vec<f32>,
    /// Top y of the (single) caret line.
    pub top: f32,
    /// Height of one line (the caret's height).
    pub line_height: f32,
}

/// Handler trait for focused text field operations.
/// Stored in focus module for O(1) key/clipboard dispatch.
pub trait FocusedTextFieldHandler {
    /// The composition node whose recorded draws show this field's caret and
    /// selection. Focus changes and caret-blink flips schedule a scoped draw
    /// repass on it — that state lives outside the draw-observation system,
    /// so nothing else can name the stale node. `None` only for handlers with
    /// no scene presence (test doubles, the platform no-op handler).
    fn node_id(&self) -> Option<cranpose_core::NodeId> {
        None
    }
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
    /// Selects all the field's text (contextual-menu "Select all").
    fn select_all(&self) {}
    /// Set IME composition (preedit) state.
    /// - `text`: The composition text being typed (empty string to clear,
    ///   which *deletes* the preedit text)
    /// - `cursor`: Optional cursor position within composition (start, end)
    fn set_composition(&self, text: &str, cursor: Option<(usize, usize)>);
    /// Finish the active composition, keeping the composed text as regular
    /// committed text (Android `finishComposingText` semantics). No-op when
    /// no composition is active.
    fn finish_composition(&self) {}
    /// Mark existing text as the composing region without changing it
    /// (Android `setComposingRegion` semantics, used by autocorrect to
    /// re-compose an already committed word). Offsets are UTF-8 bytes;
    /// implementations clamp them to valid character boundaries.
    fn set_composing_region(&self, start_bytes: usize, end_bytes: usize) {
        let _ = (start_bytes, end_bytes);
    }
    /// Move the selection/caret to `[start_bytes, end_bytes)` without editing
    /// text (Android `InputConnection.setSelection` semantics). Used by
    /// Gboard's spacebar-swipe cursor control, which scrubs the caret by
    /// repeatedly setting the selection. Offsets are UTF-8 bytes; implementations
    /// clamp them to valid character boundaries.
    fn set_selection(&self, start_bytes: usize, end_bytes: usize) {
        let _ = (start_bytes, end_bytes);
    }
    /// Snapshot of the current editable state for platform IMEs
    /// (`InputConnection` text queries, session seeding). `None` when the
    /// handler cannot expose its state.
    fn editor_state(&self) -> Option<ImeEditorState> {
        None
    }
    /// Window-space caret geometry (see [`ImeCaretGeometry`]) for coordinate-based
    /// platform text input. `None` when the handler cannot expose it (e.g. the
    /// field has not been laid out yet).
    fn caret_geometry(&self) -> Option<ImeCaretGeometry> {
        None
    }
}

pub(crate) struct TextFieldFocusState {
    focused_field: RefCell<Option<Weak<RefCell<bool>>>>,
    focused_handler: RefCell<Option<Rc<dyn FocusedTextFieldHandler>>>,
    focused_modal_depth: Cell<usize>,
}

impl TextFieldFocusState {
    pub(crate) fn new() -> Self {
        Self {
            focused_field: RefCell::new(None),
            focused_handler: RefCell::new(None),
            focused_modal_depth: Cell::new(0),
        }
    }

    fn request_focus(
        &self,
        is_focused: Rc<RefCell<bool>>,
        handler: Rc<dyn FocusedTextFieldHandler>,
        modal_depth: usize,
    ) {
        let mut current = self.focused_field.borrow_mut();

        if let Some(ref weak) = *current
            && let Some(old_focused) = weak.upgrade()
        {
            *old_focused.borrow_mut() = false;
        }

        *is_focused.borrow_mut() = true;
        *current = Some(Rc::downgrade(&is_focused));
        *self.focused_handler.borrow_mut() = Some(handler);
        self.focused_modal_depth.set(modal_depth);
    }

    fn clear_focus(&self) {
        let mut current = self.focused_field.borrow_mut();

        if let Some(ref weak) = *current
            && let Some(focused) = weak.upgrade()
        {
            *focused.borrow_mut() = false;
        }

        *current = None;
        *self.focused_handler.borrow_mut() = None;
        self.focused_modal_depth.set(0);
    }

    fn focused_at_depth(&self, depth: usize) -> bool {
        self.has_focused_field() && self.focused_modal_depth.get() == depth
    }

    fn has_focused_field(&self) -> bool {
        if self.focused_field_is_live() {
            return true;
        }
        self.clear_stale_focus();
        false
    }

    fn focused_field_is_live(&self) -> bool {
        self.focused_field
            .borrow()
            .as_ref()
            .is_some_and(|weak| weak.upgrade().is_some())
    }

    fn clear_stale_focus(&self) {
        let stale_node = self
            .focused_handler
            .borrow()
            .as_ref()
            .and_then(|handler| handler.node_id());
        let had_entry = self.focused_field.borrow_mut().take().is_some();
        if !had_entry {
            return;
        }
        self.focused_handler.borrow_mut().take();
        if let Some(node_id) = stale_node {
            crate::schedule_draw_repass(node_id);
        }
        crate::cursor_animation::stop_cursor_blink();
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

    fn dispatch_select_all(&self) -> bool {
        if let Some(handler) = self.focused_handler() {
            handler.select_all();
            true
        } else {
            false
        }
    }

    fn dispatch_ime_preedit(&self, text: &str, cursor: Option<(usize, usize)>) -> bool {
        if let Some(handler) = self.focused_handler() {
            handler.set_composition(text, cursor);
            true
        } else {
            false
        }
    }

    fn dispatch_ime_finish_composing(&self) -> bool {
        if let Some(handler) = self.focused_handler() {
            handler.finish_composition();
            true
        } else {
            false
        }
    }

    fn dispatch_ime_set_composing_region(&self, start_bytes: usize, end_bytes: usize) -> bool {
        if let Some(handler) = self.focused_handler() {
            handler.set_composing_region(start_bytes, end_bytes);
            true
        } else {
            false
        }
    }

    fn dispatch_ime_set_selection(&self, start_bytes: usize, end_bytes: usize) -> bool {
        if let Some(handler) = self.focused_handler() {
            handler.set_selection(start_bytes, end_bytes);
            true
        } else {
            false
        }
    }

    fn focused_editor_state(&self) -> Option<ImeEditorState> {
        self.focused_handler()
            .and_then(|handler| handler.editor_state())
    }

    fn focused_caret_geometry(&self) -> Option<ImeCaretGeometry> {
        self.focused_handler()
            .and_then(|handler| handler.caret_geometry())
    }
}

/// Requests focus for a text field composed at [`crate::modal::local_modal_depth`]
/// `modal_depth`.
///
/// Refused (a no-op) when `modal_depth` is shallower than the modal depth
/// that is open right now — a field behind an open dialog must not be
/// able to steal focus from it. A field inside the innermost dialog (or
/// outside any dialog, while none is open) has `modal_depth` equal to the
/// current modal depth and is granted focus normally.
///
/// If another text field was previously focused, it will be unfocused first.
/// The provided `is_focused` handle should be the field's focus state.
/// The handler is stored for O(1) key dispatch.
pub fn request_focus(
    is_focused: Rc<RefCell<bool>>,
    handler: Rc<dyn FocusedTextFieldHandler>,
    modal_depth: usize,
) {
    if modal_depth < crate::modal::current_modal_depth() {
        return;
    }

    let previous_field = focused_field_node();
    let gaining_field = handler.node_id();

    crate::render_state::with_text_field_focus(|state| {
        state.request_focus(is_focused, handler, modal_depth);
    });

    for node_id in [previous_field, gaining_field].into_iter().flatten() {
        crate::schedule_draw_repass(node_id);
    }

    crate::cursor_animation::start_cursor_blink();

    crate::text_input_session::notify_text_input_focus_gained();

    crate::request_render_invalidation();
}

pub(crate) fn clear_focus_for_closed_modal(depth: usize) {
    let owns_focus =
        crate::render_state::with_text_field_focus(|state| state.focused_at_depth(depth));
    if owns_focus {
        clear_focus();
    }
}

/// Clears focus from the currently focused text field.
pub fn clear_focus() {
    let previous = focused_field_node();
    if let Some(node_id) = previous {
        crate::schedule_draw_repass(node_id);
    }
    crate::render_state::with_text_field_focus(TextFieldFocusState::clear_focus);
    if previous.is_some() && crate::focus_dispatch::active_focus_target() == previous {
        crate::focus_dispatch::clear_active_focus();
    }

    crate::cursor_animation::stop_cursor_blink();

    crate::text_input_session::notify_text_input_focus_lost();

    crate::request_render_invalidation();
}

/// Returns the composition node of the currently focused text field, if a
/// field is focused and its handler knows its node.
pub fn focused_field_node() -> Option<cranpose_core::NodeId> {
    crate::render_state::with_text_field_focus(|state| {
        state
            .focused_handler()
            .and_then(|handler| handler.node_id())
    })
}

/// Returns true if any text field currently has focus.
/// Checks weak ref liveness and clears stale focus state.
pub fn has_focused_field() -> bool {
    let has_focus =
        crate::render_state::with_text_field_focus(TextFieldFocusState::has_focused_field);
    if !has_focus {
        crate::text_input_session::notify_text_input_focus_lost();
    }
    has_focus
}

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
    crate::render_state::with_text_field_focus(TextFieldFocusState::dispatch_copy)
}

/// Cuts selection from focused text field (copy + delete).
/// O(1) operation using stored handler.
pub fn dispatch_cut() -> Option<String> {
    crate::render_state::with_text_field_focus(TextFieldFocusState::dispatch_cut)
}

/// Selects all the text in the focused text field (contextual-menu "Select
/// all"). Returns true if a text field was focused.
pub fn dispatch_select_all() -> bool {
    crate::render_state::with_text_field_focus(TextFieldFocusState::dispatch_select_all)
}

/// Dispatches IME preedit (composition) state to the focused text field.
/// O(1) operation using stored handler.
/// Returns true if a text field was focused and received the event.
pub fn dispatch_ime_preedit(text: &str, cursor: Option<(usize, usize)>) -> bool {
    crate::render_state::with_text_field_focus(|state| state.dispatch_ime_preedit(text, cursor))
}

/// Finishes the active composition in the focused text field, keeping the
/// composed text (Android `finishComposingText` semantics).
/// Returns true if a text field was focused and received the event.
pub fn dispatch_ime_finish_composing() -> bool {
    crate::render_state::with_text_field_focus(TextFieldFocusState::dispatch_ime_finish_composing)
}

/// Marks existing text in the focused field as the composing region without
/// changing it (Android `setComposingRegion` semantics).
/// Returns true if a text field was focused and received the event.
pub fn dispatch_ime_set_composing_region(start_bytes: usize, end_bytes: usize) -> bool {
    crate::render_state::with_text_field_focus(|state| {
        state.dispatch_ime_set_composing_region(start_bytes, end_bytes)
    })
}

/// Moves the focused field's selection/caret to `[start_bytes, end_bytes)`
/// without editing text (Android `setSelection` semantics; the path Gboard's
/// spacebar-swipe uses to scrub the cursor).
/// Returns true if a text field was focused and received the event.
pub fn dispatch_ime_set_selection(start_bytes: usize, end_bytes: usize) -> bool {
    crate::render_state::with_text_field_focus(|state| {
        state.dispatch_ime_set_selection(start_bytes, end_bytes)
    })
}

/// Returns a snapshot of the focused text field's editable state for
/// platform IMEs, or `None` when no field is focused (or the handler does
/// not expose its state).
pub fn focused_editor_state() -> Option<ImeEditorState> {
    crate::render_state::with_text_field_focus(TextFieldFocusState::focused_editor_state)
}

/// Window-space caret geometry of the focused field (see [`ImeCaretGeometry`]),
/// or `None` when no field is focused or it exposes no geometry.
pub fn focused_caret_geometry() -> Option<ImeCaretGeometry> {
    crate::render_state::with_text_field_focus(TextFieldFocusState::focused_caret_geometry)
}

#[cfg(test)]
#[path = "tests/text_field_focus_tests.rs"]
mod tests;
