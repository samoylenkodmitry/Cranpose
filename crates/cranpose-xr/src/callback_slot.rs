//! A callback that the content of a spatial composable sets and the backend calls.

use std::cell::RefCell;

// A callback that takes `Argument`.
pub(crate) type Callback<Argument> = Box<dyn FnMut(Argument)>;

// Calls the callback in `slot` with `argument`. The callback runs with no borrow held, so it may
// set a new callback.
pub(crate) fn invoke<Argument>(slot: &RefCell<Option<Callback<Argument>>>, argument: Argument) {
    let taken = slot.borrow_mut().take();
    if let Some(mut callback) = taken {
        callback(argument);
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            *slot = Some(callback);
        }
    }
}
