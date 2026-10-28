use std::rc::Rc;

use super::StateHandleLease;
use crate::thread_state;

struct StateOwnerFrame;

impl Drop for StateOwnerFrame {
    fn drop(&mut self) {
        thread_state::runtimes(|state| state.state_owners.borrow_mut().pop());
    }
}

/// Runs `build` with every state it creates owned by the caller, and returns
/// those states beside its value.
///
/// This is what makes the Jetpack Compose shape safe here:
///
/// ```rust,ignore
/// remember(|| Holder {
///     count: mutableStateOf(0),
/// })
/// ```
///
/// Kotlin leaves the state to the garbage collector, which frees it with the
/// object that holds it. Nothing collects here, so a state with no owner has
/// to be kept by the runtime for as long as the runtime lives, and a holder
/// built once per screen would pile up cells nobody can reach. Handing the
/// states to whoever is building the value puts them back on the object's
/// lifetime: the slot drops the value, the value drops the states.
pub(crate) fn collecting_states<T>(build: impl FnOnce() -> T) -> (T, Vec<Rc<StateHandleLease>>) {
    thread_state::runtimes(|state| state.state_owners.borrow_mut().push(Vec::new()));
    let frame = StateOwnerFrame;
    let value = build();
    let states = thread_state::runtimes(|state| {
        state
            .state_owners
            .borrow_mut()
            .last_mut()
            .map(std::mem::take)
            .unwrap_or_default()
    });
    drop(frame);
    (value, states)
}

pub(super) fn hand_to_current_owner(lease: &Rc<StateHandleLease>) -> bool {
    thread_state::runtimes(|state| match state.state_owners.borrow_mut().last_mut() {
        Some(owner) => {
            owner.push(Rc::clone(lease));
            true
        }
        None => false,
    })
}
