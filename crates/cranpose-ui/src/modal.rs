//! The stack of modal surfaces that are open right now.
//!
//! Modality is a user-interface fact — a dialog is open, everything behind it
//! is inert — while the back gesture is a platform one. This registry is where
//! they meet: dialogs push themselves on while they are composed, and the
//! application shell asks the innermost one to close when the platform reports
//! a back request. Nothing outside the framework registers or dispatches.

use cranpose_core::{try_mutableStateOf, MutableState};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

struct ModalEntry {
    id: u64,
    on_back: Rc<dyn Fn()>,
}

thread_local! {
    static MODALS: RefCell<Vec<ModalEntry>> = const { RefCell::new(Vec::new()) };
    static NEXT_ID: Cell<u64> = const { Cell::new(1) };
    /// The count, always accurate whether or not a runtime exists.
    static DEPTH_COUNT: Cell<usize> = const { Cell::new(0) };
    /// Observable depth, so a composable that reads it recomposes when a modal
    /// opens or closes. Allocated on the first read that has a runtime to
    /// allocate from; a unit test with no runtime still sees the count.
    static DEPTH: RefCell<Option<MutableState<usize>>> = const { RefCell::new(None) };
}

fn depth_state() -> Option<MutableState<usize>> {
    DEPTH.with(|cell| {
        let mut cell = cell.borrow_mut();
        if cell.is_none() {
            *cell = try_mutableStateOf(DEPTH_COUNT.with(Cell::get));
        }
        *cell
    })
}

fn publish_depth() {
    let depth = MODALS.with(|modals| modals.borrow().len());
    DEPTH_COUNT.with(|count| count.set(depth));
    if let Some(state) = DEPTH.with(|cell| *cell.borrow()) {
        if state.get() != depth {
            state.set(depth);
        }
    }
}

/// Keeps a modal surface on the stack until it is dropped.
pub struct ModalRegistration {
    id: u64,
}

impl Drop for ModalRegistration {
    fn drop(&mut self) {
        MODALS.with(|modals| modals.borrow_mut().retain(|entry| entry.id != self.id));
        publish_depth();
    }
}

/// Pushes a modal surface onto the stack. The innermost registration is the one
/// [`dispatch_modal_back`] asks to close.
pub fn register_modal(on_back: Rc<dyn Fn()>) -> ModalRegistration {
    let id = NEXT_ID.with(|next| {
        let id = next.get();
        next.set(id + 1);
        id
    });
    MODALS.with(|modals| modals.borrow_mut().push(ModalEntry { id, on_back }));
    publish_depth();
    ModalRegistration { id }
}

/// How many modal surfaces are open. Reading this in a composable subscribes to
/// it, so the reader recomposes when a modal opens or closes.
pub fn modal_depth() -> usize {
    match depth_state() {
        Some(state) => state.get(),
        None => DEPTH_COUNT.with(Cell::get),
    }
}

/// Asks the innermost modal surface to close.
///
/// Returns whether a modal took the request. A modal that chooses not to close
/// still takes it, because the screen behind a modal must not react to a back
/// gesture aimed at the modal.
pub fn dispatch_modal_back() -> bool {
    let innermost = MODALS.with(|modals| {
        modals
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

/// Clears every registration. Used by tests and by host teardown so one
/// composition's modals never outlive it.
pub fn clear_modals() {
    MODALS.with(|modals| modals.borrow_mut().clear());
    publish_depth();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_innermost_modal_takes_the_back_request() {
        clear_modals();
        let outer = Rc::new(Cell::new(0u32));
        let inner = Rc::new(Cell::new(0u32));
        let outer_counter = Rc::clone(&outer);
        let inner_counter = Rc::clone(&inner);

        let _outer = register_modal(Rc::new(move || outer_counter.set(outer_counter.get() + 1)));
        let inner_registration =
            register_modal(Rc::new(move || inner_counter.set(inner_counter.get() + 1)));

        assert!(dispatch_modal_back());
        assert_eq!(inner.get(), 1);
        assert_eq!(outer.get(), 0);

        drop(inner_registration);
        assert!(dispatch_modal_back());
        assert_eq!(outer.get(), 1);
        clear_modals();
    }

    #[test]
    fn a_back_request_with_no_modal_open_is_not_taken() {
        clear_modals();
        assert!(!dispatch_modal_back());
    }

    #[test]
    fn the_depth_counts_what_is_open_and_falls_back_to_zero() {
        clear_modals();
        assert_eq!(modal_depth(), 0);

        let outer = register_modal(Rc::new(|| {}));
        assert_eq!(modal_depth(), 1);
        let inner = register_modal(Rc::new(|| {}));
        assert_eq!(modal_depth(), 2);

        // Dropping a registration is what closes a modal, so the depth the
        // shell's back handler reads has to follow the drop rather than a call.
        drop(inner);
        assert_eq!(modal_depth(), 1);
        drop(outer);
        assert_eq!(modal_depth(), 0);
    }

    #[test]
    fn registrations_leave_the_stack_when_dropped() {
        clear_modals();
        {
            let _first = register_modal(Rc::new(|| {}));
            let _second = register_modal(Rc::new(|| {}));
            assert_eq!(MODALS.with(|modals| modals.borrow().len()), 2);
        }
        assert_eq!(MODALS.with(|modals| modals.borrow().len()), 0);
        assert!(!dispatch_modal_back());
    }
}
