// Finds a runtime of this thread by its id.

use std::rc::Rc;

use super::{RuntimeHandle, RuntimeId, RuntimeInner, StateArena};
use crate::thread_state::{self, RuntimeThreadState};

pub(crate) fn runtime_handle_by_id(id: RuntimeId) -> Option<RuntimeHandle> {
    with_registered_runtime(id, |handle| Some(handle.clone()))
}

// Runs `f` on this thread's handle of the runtime `id`: the last runtime's,
// which most lookups name, without hashing the id, or else the registered
// one. The last runtime is the one that last started its work: the runtime
// composing or polling, and the thread's only runtime while the thread lays
// out or draws.
fn with_registered_runtime<R>(
    id: RuntimeId,
    f: impl FnOnce(&RuntimeHandle) -> Option<R>,
) -> Option<R> {
    thread_state::try_runtimes(|state| {
        if let Some(handle) = state.last_runtime.borrow().as_ref()
            && handle.id == id
        {
            return f(handle);
        }
        registered_runtime(state, id, f)
    })
    .flatten()
}

// Out of line: a thread with one runtime never looks a runtime up by its id.
#[cold]
#[inline(never)]
fn registered_runtime<R>(
    state: &RuntimeThreadState,
    id: RuntimeId,
    f: impl FnOnce(&RuntimeHandle) -> Option<R>,
) -> Option<R> {
    state.registered_runtimes.borrow().get(&id).and_then(f)
}

pub(crate) fn with_state_arena_by_id<R>(
    id: RuntimeId,
    f: impl FnOnce(&StateArena) -> R,
) -> Option<R> {
    let inner = runtime_inner(id)?;
    Some(f(&inner.state_arena))
}

// The runtime `id` names, while it lives. Not generic: every state access
// shares the one lookup by id it falls back to.
#[inline]
fn runtime_inner(id: RuntimeId) -> Option<Rc<RuntimeInner>> {
    with_registered_runtime(id, |handle| handle.inner.upgrade())
}
