use std::{
    any::Any,
    cell::RefCell,
    rc::{Rc, Weak},
};

use super::{RuntimeHandle, StateHandleLease};
use crate::{
    MutableStateInner, ScopeId,
    collections::map::HashMap,
    state::{MutationPolicy, NeverEqual},
};

struct TypedStateCell<T: Clone + 'static> {
    inner: MutableStateInner<T>,
}

trait ScopeWatchCell {
    fn unregister_scope(&self, scope_id: ScopeId);
    fn forget_scope(&self, scope_id: ScopeId);
}

impl<T: Clone + 'static> ScopeWatchCell for TypedStateCell<T> {
    fn unregister_scope(&self, scope_id: ScopeId) {
        self.inner.unregister_scope(scope_id);
    }

    fn forget_scope(&self, scope_id: ScopeId) {
        self.inner.forget_scope(scope_id);
    }
}

struct StateArenaSlot {
    generation: u32,
    cell: Option<Rc<dyn Any>>,
    watcher_cell: Option<Rc<dyn ScopeWatchCell>>,
    lease: Option<Weak<StateHandleLease>>,
}

#[derive(Default)]
struct StateArenaInner {
    cells: Vec<StateArenaSlot>,
    free: Vec<u32>,
    /// The derived states among the cells, by slot.
    derived: HashMap<u32, Weak<dyn DerivedRefresh>>,
}

/// A derived state that brings its value up to date before a read.
pub(crate) trait DerivedRefresh {
    /// Derives the value again when a state it read changed since the
    /// recomposition that last derived it.
    fn refresh(&self);
}

/// Sizes of a runtime's state arena, for leak and memory checks. A `_len` field counts entries and
/// a `_cap` field the capacity reserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StateArenaDebugStats {
    /// State cells, both in use and free.
    pub cells_len: usize,
    /// Capacity reserved for the state cells.
    pub cells_cap: usize,
    /// State cells free for reuse.
    pub free_len: usize,
    /// Capacity reserved for the free list.
    pub free_cap: usize,
}

#[derive(Default)]
pub(crate) struct StateArena {
    inner: RefCell<StateArenaInner>,
}

impl StateArena {
    pub(crate) fn alloc<T: Clone + 'static>(&self, value: T, runtime: RuntimeHandle) -> StateId {
        self.alloc_with_policy(value, runtime, Rc::new(NeverEqual))
    }

    /// A free cell's id, for a new value to take.
    fn claim_slot(&self) -> StateId {
        let mut inner = self.inner.borrow_mut();
        loop {
            let Some(slot) = inner.free.pop() else {
                let slot = inner.cells.len() as u32;
                inner.cells.push(StateArenaSlot {
                    generation: 0,
                    cell: None,
                    watcher_cell: None,
                    lease: None,
                });
                return StateId::new(slot, 0);
            };

            let Some(entry) = inner.cells.get_mut(slot as usize) else {
                continue;
            };
            if entry.cell.is_some() {
                continue;
            }

            entry.watcher_cell = None;
            entry.lease = None;
            entry.generation = entry.generation.wrapping_add(1);
            return StateId::new(slot, entry.generation);
        }
    }

    pub(crate) fn alloc_with_policy<T: Clone + 'static>(
        &self,
        value: T,
        runtime: RuntimeHandle,
        policy: Rc<dyn MutationPolicy<T>>,
    ) -> StateId {
        let id = self.claim_slot();
        let slot = id.slot();
        let inner = MutableStateInner::new_with_policy(value, runtime, policy);
        inner.install_snapshot_observer(id);
        let typed_cell = Rc::new(TypedStateCell { inner });
        let cell: Rc<dyn Any> = Rc::<TypedStateCell<T>>::clone(&typed_cell);
        let watcher_cell: Rc<dyn ScopeWatchCell> = typed_cell;
        let mut arena = self.inner.borrow_mut();
        let slot_entry = &mut arena.cells[slot as usize];
        slot_entry.cell = Some(cell);
        slot_entry.watcher_cell = Some(watcher_cell);
        id
    }

    /// Stores `value` in a cell of its own, which keeps no snapshot records
    /// and no readers: see [`crate::RuntimeCell`].
    pub(crate) fn alloc_plain<T: 'static>(&self, value: T) -> StateId {
        let id = self.claim_slot();
        let cell: Rc<dyn Any> = Rc::new(value);
        self.inner.borrow_mut().cells[id.slot_index()].cell = Some(cell);
        id
    }

    /// The value [`Self::alloc_plain`] stored for `id`, while it lives.
    pub(crate) fn plain<T: 'static>(&self, id: StateId) -> Option<Rc<T>> {
        Rc::downcast::<T>(self.get_cell_opt(id)?).ok()
    }

    fn get_cell_opt(&self, id: StateId) -> Option<Rc<dyn Any>> {
        self.inner
            .borrow()
            .cells
            .get(id.slot_index())
            .filter(|cell| cell.generation == id.generation())
            .and_then(|cell| cell.cell.as_ref())
            .cloned()
    }

    fn get_typed<T: Clone + 'static>(&self, id: StateId) -> Rc<TypedStateCell<T>> {
        match self.get_cell_opt(id) {
            None => panic!(
                "state cell missing: slot={}, gen={}, expected={}",
                id.slot(),
                id.generation(),
                std::any::type_name::<T>(),
            ),
            Some(cell) => Rc::downcast::<TypedStateCell<T>>(cell).unwrap_or_else(|_| {
                panic!(
                    "state cell type mismatch: slot={}, gen={}, expected={}",
                    id.slot(),
                    id.generation(),
                    std::any::type_name::<T>(),
                )
            }),
        }
    }

    fn get_typed_opt<T: Clone + 'static>(&self, id: StateId) -> Option<Rc<TypedStateCell<T>>> {
        Rc::downcast::<TypedStateCell<T>>(self.get_cell_opt(id)?).ok()
    }

    pub(crate) fn with_typed<T: Clone + 'static, R>(
        &self,
        id: StateId,
        f: impl FnOnce(&MutableStateInner<T>) -> R,
    ) -> R {
        let cell = self.get_typed::<T>(id);
        f(&cell.inner)
    }

    pub(crate) fn with_typed_opt<T: Clone + 'static, R>(
        &self,
        id: StateId,
        f: impl FnOnce(&MutableStateInner<T>) -> R,
    ) -> Option<R> {
        let cell = self.get_typed_opt::<T>(id)?;
        Some(f(&cell.inner))
    }

    pub(crate) fn release(&self, id: StateId) {
        let cell = {
            let mut inner = self.inner.borrow_mut();
            let Some(slot) = inner.cells.get_mut(id.slot_index()) else {
                return;
            };
            if slot.generation != id.generation() {
                return;
            }
            slot.lease = None;
            slot.watcher_cell = None;
            let cell = slot.cell.take();
            if cell.is_some() {
                inner.free.push(id.slot());
                if !inner.derived.is_empty() {
                    inner.derived.remove(&id.slot());
                }
            }
            cell
        };
        drop(cell);
    }

    /// Makes `refresh` what brings the derived state `id` up to date before
    /// each read, until the state is released.
    pub(crate) fn register_derived(&self, id: StateId, refresh: Weak<dyn DerivedRefresh>) {
        self.inner.borrow_mut().derived.insert(id.slot(), refresh);
    }

    /// Brings the derived state `id` up to date before a read; see
    /// [`DerivedRefresh`].
    #[cold]
    pub(crate) fn refresh_derived(&self, id: StateId) {
        let refresh = {
            let inner = self.inner.borrow();
            inner
                .cells
                .get(id.slot_index())
                .filter(|cell| cell.generation == id.generation())
                .and_then(|_| inner.derived.get(&id.slot()))
                .and_then(Weak::upgrade)
        };
        if let Some(refresh) = refresh {
            refresh.refresh();
        }
    }

    pub(crate) fn stats(&self) -> (usize, usize) {
        let inner = self.inner.borrow();
        (inner.cells.len(), inner.free.len())
    }

    pub(crate) fn debug_stats(&self) -> StateArenaDebugStats {
        let inner = self.inner.borrow();
        StateArenaDebugStats {
            cells_len: inner.cells.len(),
            cells_cap: inner.cells.capacity(),
            free_len: inner.free.len(),
            free_cap: inner.free.capacity(),
        }
    }

    pub(crate) fn unregister_scope(&self, id: StateId, scope_id: ScopeId) {
        if let Some(watcher_cell) = self.watcher_cell(id) {
            watcher_cell.unregister_scope(scope_id);
        }
    }

    /// Stops the state `id` from recomposing the live scope `scope_id`.
    pub(crate) fn forget_scope(&self, id: StateId, scope_id: ScopeId) {
        if let Some(watcher_cell) = self.watcher_cell(id) {
            watcher_cell.forget_scope(scope_id);
        }
    }

    fn watcher_cell(&self, id: StateId) -> Option<Rc<dyn ScopeWatchCell>> {
        let inner = self.inner.borrow();
        inner
            .cells
            .get(id.slot_index())
            .filter(|slot| slot.generation == id.generation())
            .and_then(|slot| slot.watcher_cell.as_ref())
            .cloned()
    }

    pub(crate) fn register_lease(&self, id: StateId, lease: &Rc<StateHandleLease>) {
        let mut inner = self.inner.borrow_mut();
        let Some(slot) = inner.cells.get_mut(id.slot_index()) else {
            return;
        };
        if slot.generation != id.generation() {
            return;
        }
        slot.lease = Some(Rc::downgrade(lease));
    }

    pub(crate) fn retain_lease(&self, id: StateId) -> Option<Rc<StateHandleLease>> {
        let inner = self.inner.borrow();
        let slot = inner.cells.get(id.slot_index())?;
        if slot.generation != id.generation() {
            return None;
        }
        slot.lease.as_ref()?.upgrade()
    }
}

/// Names one state cell of a runtime. A generation count makes the id of a freed cell stale.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct StateId {
    slot: u32,
    generation: u32,
}

impl StateId {
    const fn new(slot: u32, generation: u32) -> Self {
        Self { slot, generation }
    }

    pub(crate) const fn slot(self) -> u32 {
        self.slot
    }

    pub(crate) const fn slot_index(self) -> usize {
        self.slot as usize
    }

    pub(crate) const fn generation(self) -> u32 {
        self.generation
    }
}

#[cfg(test)]
#[path = "tests/state_arena_tests.rs"]
mod tests;
