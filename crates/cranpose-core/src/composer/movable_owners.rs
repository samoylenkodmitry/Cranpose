// Ownership and pass-lifetime tracking for remembered movable content.

use std::cell::{Cell, RefCell};

use crate::{
    Key, SlotTable,
    collections::map::{HashMap, HashSet},
    slot::DetachedSubtree,
};

pub(crate) struct PassGuard<'a> {
    owners: &'a MovableOwners,
}

impl Drop for PassGuard<'_> {
    fn drop(&mut self) {
        let depth = self
            .owners
            .active_passes
            .get()
            .checked_sub(1)
            .expect("movable compose pass guard unbalanced");
        self.owners.active_passes.set(depth);
    }
}

#[derive(Default)]
pub(crate) struct MovableOwners {
    active_passes: Cell<usize>,
    dropped: RefCell<Vec<Key>>,
    disposed_attached: RefCell<HashSet<Key>>,
    attached_hosts: RefCell<HashMap<Key, usize>>,
}

impl MovableOwners {
    pub(crate) fn enter_pass(&self) -> PassGuard<'_> {
        let depth = self
            .active_passes
            .get()
            .checked_add(1)
            .expect("movable compose pass depth overflow");
        self.active_passes.set(depth);
        PassGuard { owners: self }
    }

    pub(crate) fn record_dropped_in_pass(&self, id: Key) -> bool {
        if self.active_passes.get() == 0 {
            return false;
        }
        self.dropped.borrow_mut().push(id);
        true
    }

    pub(crate) fn has_dropped(&self) -> bool {
        !self.dropped.borrow().is_empty()
    }

    pub(crate) fn pop_dropped(&self) -> Option<Key> {
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

    pub(crate) fn note_attached(&self, id: Key, host_key: usize) {
        self.attached_hosts.borrow_mut().insert(id, host_key);
    }

    pub(crate) fn note_attached_in_subtree(&self, subtree: &DetachedSubtree, host_key: usize) {
        let mut ids = subtree.movable_ids().peekable();
        if ids.peek().is_none() {
            return;
        }
        let mut attached = self.attached_hosts.borrow_mut();
        for id in ids {
            attached.insert(id, host_key);
        }
    }

    pub(crate) fn note_attached_in_table(&self, table: &SlotTable, host_key: usize) {
        if !table.has_attached_movables() {
            return;
        }
        let mut attached = self.attached_hosts.borrow_mut();
        for id in table.attached_movable_ids() {
            attached.insert(id, host_key);
        }
    }

    pub(crate) fn forget_attached_in_subtree(&self, subtree: &DetachedSubtree, host_key: usize) {
        if self.attached_hosts.borrow().is_empty() {
            return;
        }
        let mut ids = subtree.movable_ids().peekable();
        if ids.peek().is_none() {
            return;
        }
        let mut attached = self.attached_hosts.borrow_mut();
        for id in ids {
            if attached.get(&id) == Some(&host_key) {
                attached.remove(&id);
            }
        }
    }

    pub(crate) fn attached_host_key(&self, id: Key) -> Option<usize> {
        let attached = self.attached_hosts.borrow();
        if attached.is_empty() {
            return None;
        }
        attached.get(&id).copied()
    }

    pub(crate) fn forget_stale_attached(&self, id: Key, expected_host_key: usize) {
        let mut attached = self.attached_hosts.borrow_mut();
        if attached.get(&id) == Some(&expected_host_key) {
            attached.remove(&id);
        }
    }

    pub(crate) fn forget_host(&self, host_key: usize, table: &SlotTable) {
        if table.has_attached_movables() && !self.attached_hosts.borrow().is_empty() {
            self.attached_hosts
                .borrow_mut()
                .retain(|_, &mut owner| owner != host_key);
        }
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

    #[cfg(any(test, debug_assertions))]
    pub(crate) fn debug_verify_host(&self, host_key: usize, table: &SlotTable) {
        if !table.has_attached_movables() {
            return;
        }
        let attached = self.attached_hosts.borrow();
        for id in table.attached_movable_ids() {
            debug_assert_eq!(
                attached.get(&id),
                Some(&host_key),
                "attached movable {id:?} missing from host {host_key} index"
            );
        }
    }
}
