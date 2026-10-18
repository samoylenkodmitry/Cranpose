//! A value a copyable handle reaches in the runtime, without snapshots.

use std::{fmt, hash::Hash, marker::PhantomData, rc::Rc};

use crate::runtime::{self, RuntimeHandle, RuntimeId, StateId};

/// A value kept in a runtime, reached through a handle that copies like a
/// [`MutableState`](crate::MutableState).
///
/// A state holder that is `Copy` keeps its shared, interior-mutable parts
/// here: a read finds the value in the runtime's cells, with no snapshot
/// records to walk and no scope subscribed. Nothing recomposes when the value
/// changes, so what a composition reads goes in a `MutableState` instead.
///
/// The value is owned as a state is: made while a
/// [`remember`](crate::remember) builds its value, it belongs to that slot
/// and goes with it; made anywhere else, it lives as long as the runtime.
///
/// ```
/// use std::cell::Cell;
///
/// use cranpose_core::{DefaultScheduler, Runtime, RuntimeCell, scheduler_ref};
///
/// let runtime = Runtime::new(scheduler_ref(DefaultScheduler));
/// let count = RuntimeCell::with_runtime(Cell::new(0), runtime.handle());
/// let copy = count;
/// copy.with(|count| count.set(count.get() + 1));
/// assert_eq!(count.with(Cell::get), 1);
/// ```
pub struct RuntimeCell<T: 'static> {
    id: StateId,
    runtime_id: RuntimeId,
    _marker: PhantomData<fn() -> T>,
}

impl<T: 'static> RuntimeCell<T> {
    /// Keeps `value` in the current runtime. Panics without one.
    pub fn new(value: T) -> Self {
        Self::with_runtime(value, crate::hooks::current_runtime("RuntimeCell::new"))
    }

    /// Keeps `value` in `runtime`.
    pub fn with_runtime(value: T, runtime: RuntimeHandle) -> Self {
        Self {
            id: runtime.alloc_owned_plain(value),
            runtime_id: runtime.id(),
            _marker: PhantomData,
        }
    }

    /// Whether the value still lives: its owner and its runtime keep it.
    pub fn is_alive(&self) -> bool {
        self.try_with(|_| ()).is_some()
    }

    /// A handle to the value, or `None` once it is gone. The handle keeps the
    /// value alive.
    pub fn try_get(&self) -> Option<Rc<T>> {
        runtime::with_state_arena_by_id(self.runtime_id, |arena| arena.plain::<T>(self.id))?
    }

    /// A handle to the value. Panics once it is gone.
    pub fn get(&self) -> Rc<T> {
        self.try_get()
            .unwrap_or_else(|| panic!("runtime cell {:?} is no longer alive", self.id))
    }

    /// Runs `f` on the value, or returns `None` once it is gone.
    pub fn try_with<R>(&self, f: impl FnOnce(&T) -> R) -> Option<R> {
        self.try_get().map(|value| f(&value))
    }

    /// Runs `f` on the value. Panics once it is gone.
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        f(&self.get())
    }
}

impl<T: 'static> Clone for RuntimeCell<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: 'static> Copy for RuntimeCell<T> {}

impl<T: 'static> PartialEq for RuntimeCell<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.runtime_id == other.runtime_id
    }
}

impl<T: 'static> Eq for RuntimeCell<T> {}

impl<T: 'static> Hash for RuntimeCell<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self.runtime_id.hash(state);
    }
}

impl<T: 'static> fmt::Debug for RuntimeCell<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeCell")
            .field("id", &self.id)
            .field("runtime_id", &self.runtime_id)
            .finish()
    }
}
