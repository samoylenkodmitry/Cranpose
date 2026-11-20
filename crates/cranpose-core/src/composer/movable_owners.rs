// Ownership and pass-lifetime tracking for remembered movable content.

use std::cell::{Cell, RefCell};

use crate::{Key, SlotTable, collections::map::HashSet};

pub(crate) struct PassGuard<'a> {
    owners: &'a MovableOwners,
}

impl Drop for PassGuard<'_> {
    fn drop(&mut self) {
        self.owners.leave_pass_depth();
    }
}

#[derive(Clone, Copy)]
pub(crate) struct DroppedMovable {
    pub(crate) id: Key,
    // The slot table where this owner was created, when its content has never
    // been shown in any other table. `None` once shown across tables.
    pub(crate) owner_host: Option<usize>,
}

#[derive(Default)]
pub(crate) struct MovableOwners {
    active_passes: Cell<usize>,
    dropped: RefCell<Vec<DroppedMovable>>,
    disposed_attached: RefCell<HashSet<Key>>,
}

impl MovableOwners {
    #[inline]
    pub(crate) fn enter_pass_depth(&self) {
        let depth = self.active_passes.get();
        debug_assert!(depth < usize::MAX, "movable compose pass depth overflow");
        self.active_passes.set(depth.saturating_add(1));
    }

    #[inline]
    pub(crate) fn leave_pass_depth(&self) {
        let depth = self.active_passes.get();
        debug_assert!(depth > 0, "movable compose pass guard unbalanced");
        self.active_passes.set(depth.saturating_sub(1));
    }

    pub(crate) fn enter_pass(&self) -> PassGuard<'_> {
        self.enter_pass_depth();
        PassGuard { owners: self }
    }

    pub(crate) fn in_nested_pass(&self) -> bool {
        self.active_passes.get() > 1
    }

    pub(crate) fn record_dropped_in_pass(&self, dropped: DroppedMovable) -> bool {
        if self.active_passes.get() == 0 {
            return false;
        }
        self.dropped.borrow_mut().push(dropped);
        true
    }

    pub(crate) fn requeue_dropped(&self, deferred: impl IntoIterator<Item = DroppedMovable>) {
        self.dropped.borrow_mut().extend(deferred);
    }

    pub(crate) fn clear_dropped_owner_host(&self, id: Key) {
        if self.dropped.borrow().is_empty() {
            return;
        }
        for dropped in self.dropped.borrow_mut().iter_mut() {
            if dropped.id == id {
                dropped.owner_host = None;
            }
        }
    }

    pub(crate) fn has_dropped(&self) -> bool {
        !self.dropped.borrow().is_empty()
    }

    pub(crate) fn pop_dropped(&self) -> Option<DroppedMovable> {
        self.dropped.borrow_mut().pop()
    }

    pub(crate) fn note_disposed_attached(&self, id: Key) {
        self.disposed_attached.borrow_mut().insert(id);
    }

    pub(crate) fn take_disposed_attached(&self, id: Key) -> bool {
        if self.disposed_attached.borrow().is_empty() {
            return false;
        }
        self.disposed_attached.borrow_mut().remove(&id)
    }

    pub(crate) fn forget_host(&self, table: &SlotTable) {
        if !self.disposed_attached.borrow().is_empty() {
            self.forget_disposed_in_table(table);
        }
    }

    #[cold]
    #[inline(never)]
    fn forget_disposed_in_table(&self, table: &SlotTable) {
        self.disposed_attached
            .borrow_mut()
            .retain(|&id| !table.movable_is_attached(id));
    }
}
