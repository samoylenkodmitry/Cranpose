// Finds a runtime of this thread by its id.

use super::{RuntimeHandle, RuntimeId, StateArena};
use crate::thread_state;

pub(crate) fn runtime_handle_by_id(id: RuntimeId) -> Option<RuntimeHandle> {
    with_registered_runtime(id, |handle| Some(handle.clone()))
}

// Runs `f` on this thread's handle of the runtime `id`: the active
// runtime's, which most lookups name, without hashing the id, or else the
// registered one.
fn with_registered_runtime<R>(
    id: RuntimeId,
    f: impl FnOnce(&RuntimeHandle) -> Option<R>,
) -> Option<R> {
    thread_state::try_runtimes(|state| {
        if let Some(handle) = state.active_runtimes.borrow().last()
            && handle.id == id
        {
            return f(handle);
        }
        state.registered_runtimes.borrow().get(&id).and_then(f)
    })
    .flatten()
}

pub(crate) fn with_state_arena_by_id<R>(
    id: RuntimeId,
    f: impl FnOnce(&StateArena) -> R,
) -> Option<R> {
    let inner = with_registered_runtime(id, |handle| handle.inner.upgrade())?;
    Some(f(&inner.state_arena))
}
