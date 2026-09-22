//! The stack of modal surfaces that are open right now.
//!
//! Modality is a user-interface fact — a dialog is open, everything behind it
//! is inert — while the back gesture is a platform one. This registry is where
//! they meet: dialogs push themselves on while they are composed, and the
//! application shell asks the innermost one to close when the platform reports
//! a back request. Nothing outside the framework registers or dispatches.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{
    CompositionLocal, MutableState, OwnedMutableState, compositionLocalOf, try_mutableStateOf,
};

struct ModalEntry {
    id: u64,
    on_back: Rc<dyn Fn()>,
}

pub(crate) struct ModalState {
    entries: RefCell<Vec<ModalEntry>>,
    next_id: Cell<u64>,
    depth: RefCell<Option<OwnedMutableState<usize>>>,
}

impl ModalState {
    pub(crate) fn new() -> Self {
        Self {
            entries: RefCell::new(Vec::new()),
            next_id: Cell::new(1),
            depth: RefCell::new(None),
        }
    }

    fn depth_state(&self) -> Option<MutableState<usize>> {
        let mut state = self.depth.borrow_mut();
        if state.is_none() {
            *state = try_mutableStateOf(self.entries.borrow().len())
                .map(|depth| MutableState::retain(&depth));
        }
        state.as_ref().map(OwnedMutableState::handle)
    }

    fn publish_depth(&self) {
        let depth = self.entries.borrow().len();
        let state = self.depth.borrow().as_ref().map(OwnedMutableState::handle);
        if let Some(state) = state
            && state.get() != depth
        {
            state.set(depth);
        }
    }
}

/// Keeps a modal surface on the stack until it is dropped.
pub struct ModalRegistration {
    id: u64,
    depth: usize,
    app_context: crate::render_state::AppContextId,
}

impl Drop for ModalRegistration {
    fn drop(&mut self) {
        crate::render_state::enter_app_context_by_id(self.app_context, || {
            crate::render_state::with_modal_state(|state| {
                state
                    .entries
                    .borrow_mut()
                    .retain(|entry| entry.id != self.id);
                state.publish_depth();
            });
            crate::text_field_focus::clear_focus_for_closed_modal(self.depth);
        });
    }
}

/// Pushes a modal surface onto the stack. The innermost registration is the one
/// [`dispatch_modal_back`] asks to close.
/// The registration belongs to the current [`crate::AppContext`].
pub fn register_modal(on_back: Rc<dyn Fn()>) -> ModalRegistration {
    let app_context = crate::render_state::current_app_context_id();
    crate::render_state::with_modal_state(|state| {
        let id = state.next_id.get();
        state.next_id.set(id + 1);
        state.entries.borrow_mut().push(ModalEntry { id, on_back });
        let depth = state.entries.borrow().len();
        state.publish_depth();
        ModalRegistration {
            id,
            depth,
            app_context,
        }
    })
}

/// How many modal surfaces are open. Reading this in a composable subscribes to
/// it, so the reader recomposes when a modal opens or closes.
pub fn modal_depth() -> usize {
    crate::render_state::with_modal_state(|state| {
        state
            .depth_state()
            .map_or_else(|| state.entries.borrow().len(), |depth| depth.get())
    })
}

pub(crate) fn current_modal_depth() -> usize {
    crate::render_state::with_modal_state(|state| state.entries.borrow().len())
}

/// CompositionLocal carrying the modal depth at which content is being
/// composed: zero outside any dialog, or the depth of the innermost dialog
/// that encloses the current composition.
///
/// A modal provides this to its own content, set to the depth its own
/// [`register_modal`] registration occupies (see [`ModalRegistration`]).
/// Text fields read it and hand it to their focus request, so a field behind
/// an open dialog — whose read of this local stays at a shallower depth —
/// can be told apart from one inside it.
///
/// The same `CompositionLocal` instance is returned on every call (cached per
/// thread), so the modal that provides it and the field that reads it observe
/// one shared local.
pub fn local_modal_depth() -> CompositionLocal<usize> {
    thread_local! {
        static LOCAL: RefCell<Option<CompositionLocal<usize>>> = const { RefCell::new(None) };
    }
    LOCAL.with(|cell| {
        cell.borrow_mut()
            .get_or_insert_with(|| compositionLocalOf(|| 0usize))
            .clone()
    })
}

/// Asks the innermost modal surface to close.
///
/// Returns whether a modal took the request. A modal that chooses not to close
/// still takes it, because the screen behind a modal must not react to a back
/// gesture aimed at the modal.
pub fn dispatch_modal_back() -> bool {
    let innermost = crate::render_state::with_modal_state(|state| {
        state
            .entries
            .borrow()
            .last()
            .map(|entry| Rc::clone(&entry.on_back))
    });
    match innermost {
        Some(on_back) => {
            on_back();
            true
        }
        None => false,
    }
}

/// Clears every registration in the current [`crate::AppContext`].
pub fn clear_modals() {
    crate::render_state::with_modal_state(|state| {
        state.entries.borrow_mut().clear();
        state.publish_depth();
    });
}

#[cfg(test)]
#[path = "tests/modal_tests.rs"]
mod tests;
