// Ownership, pass-lifetime, and cold pending-site tracking for movable content.

use std::{
    cell::{Cell, RefCell},
    sync::OnceLock,
};

use super::PendingMovable;
use crate::{Composer, ComposerCore, Key, NodeError, RetentionMode, SlotTable, SlotsHost};

pub(crate) type DetachedChildMovableHandler =
    fn(
        &ComposerCore,
        &SlotsHost,
        crate::slot::DetachedSubtree,
        RetentionMode,
    ) -> Result<Option<crate::slot::DetachedSubtree>, NodeError>;

// Registered on the first movable-content call so binaries that never use
// movable content let `--gc-sections` dead-strip the movable-only machinery.
pub(crate) struct MovableHooks {
    pub(crate) handle_detached_child: DetachedChildMovableHandler,
    pub(crate) settle_pass: fn(&Composer) -> Result<(), NodeError>,
    pub(crate) forget_movables: fn(&Composer, &[Key]) -> Result<(), NodeError>,
    pub(crate) forget_host: fn(&MovableOwners, &SlotTable),
    pub(crate) clear_host: fn(&MovableOwners, usize, &SlotTable),
}

static MOVABLE_HOOKS: OnceLock<&'static MovableHooks> = OnceLock::new();

#[inline]
pub(crate) fn movable_hooks() -> Option<&'static MovableHooks> {
    MOVABLE_HOOKS.get().copied()
}

pub(crate) fn install_movable_hooks(hooks: &'static MovableHooks) {
    MOVABLE_HOOKS.get_or_init(|| hooks);
}

const FLAG_DROPPED: u8 = 1 << 0;
const FLAG_PENDING: u8 = 1 << 1;
const FLAG_DISPOSED: u8 = 1 << 2;

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
struct MovableColdState {
    // Flat (host_key, site) list: avoids instantiating HashMap<usize, Vec<PendingMovable>>.
    pending: Vec<(usize, PendingMovable)>,
    dropped: Vec<DroppedMovable>,
    // Sorted by Key: avoids instantiating a dedicated HashSet<Key> RawTable.
    disposed_attached: Vec<Key>,
}

#[derive(Default)]
pub(crate) struct MovableOwners {
    active_passes: Cell<u16>,
    flags: Cell<u8>,
    cold: RefCell<Option<Box<MovableColdState>>>,
}

#[cold]
#[inline(never)]
pub(super) fn panic_invariant(message: &'static str) -> ! {
    panic!("{message}");
}

impl MovableOwners {
    #[inline]
    pub(crate) fn enter_pass_depth(&self) {
        let Some(next) = self.active_passes.get().checked_add(1) else {
            panic_invariant("movable compose pass depth overflow");
        };
        self.active_passes.set(next);
    }

    #[inline]
    pub(crate) fn leave_pass_depth(&self) {
        let Some(next) = self.active_passes.get().checked_sub(1) else {
            panic_invariant("movable compose pass guard unbalanced");
        };
        self.active_passes.set(next);
    }

    pub(crate) fn enter_pass(&self) -> PassGuard<'_> {
        self.enter_pass_depth();
        PassGuard { owners: self }
    }

    #[inline]
    pub(crate) fn in_nested_pass(&self) -> bool {
        self.active_passes.get() > 1
    }

    #[inline]
    fn has_flag(&self, bit: u8) -> bool {
        (self.flags.get() & bit) != 0
    }

    #[inline(never)]
    fn sync_flags(&self, cold: &MovableColdState) {
        let mut flags = 0_u8;
        if !cold.dropped.is_empty() {
            flags |= FLAG_DROPPED;
        }
        if !cold.pending.is_empty() {
            flags |= FLAG_PENDING;
        }
        if !cold.disposed_attached.is_empty() {
            flags |= FLAG_DISPOSED;
        }
        self.flags.set(flags);
    }

    #[cold]
    #[inline(never)]
    fn ensure_cold(slot: &mut Option<Box<MovableColdState>>) -> &mut MovableColdState {
        slot.get_or_insert_with(Default::default)
    }

    pub(crate) fn record_dropped_in_pass(&self, dropped: DroppedMovable) -> bool {
        if self.active_passes.get() == 0 {
            return false;
        }
        self.push_dropped(dropped);
        true
    }

    #[cold]
    #[inline(never)]
    pub(crate) fn push_dropped(&self, dropped: DroppedMovable) {
        let mut slot = self.cold.borrow_mut();
        let cold = Self::ensure_cold(&mut slot);
        cold.dropped.push(dropped);
        self.sync_flags(cold);
    }

    #[cold]
    #[inline(never)]
    pub(crate) fn requeue_dropped(&self, deferred: Vec<DroppedMovable>) {
        let mut slot = self.cold.borrow_mut();
        let cold = Self::ensure_cold(&mut slot);
        cold.dropped.extend(deferred);
        self.sync_flags(cold);
    }

    #[cold]
    #[inline(never)]
    pub(crate) fn clear_dropped_owner_host(&self, id: Key) {
        if !self.has_flag(FLAG_DROPPED) {
            return;
        }
        if let Some(cold) = self.cold.borrow_mut().as_deref_mut() {
            for dropped in &mut cold.dropped {
                if dropped.id == id {
                    dropped.owner_host = None;
                }
            }
        }
    }

    #[inline]
    pub(crate) fn has_dropped(&self) -> bool {
        self.has_flag(FLAG_DROPPED)
    }

    #[inline]
    pub(crate) fn has_pending_any(&self) -> bool {
        self.has_flag(FLAG_PENDING)
    }

    #[cold]
    #[inline(never)]
    pub(crate) fn pop_dropped(&self) -> Option<DroppedMovable> {
        if !self.has_flag(FLAG_DROPPED) {
            return None;
        }
        let mut slot = self.cold.borrow_mut();
        let cold = slot.as_deref_mut()?;
        let popped = cold.dropped.pop();
        self.sync_flags(cold);
        popped
    }

    #[cold]
    #[inline(never)]
    pub(crate) fn note_disposed_attached(&self, id: Key) {
        let mut slot = self.cold.borrow_mut();
        let cold = Self::ensure_cold(&mut slot);
        if let Err(pos) = cold.disposed_attached.binary_search(&id) {
            cold.disposed_attached.insert(pos, id);
        }
        self.sync_flags(cold);
    }

    #[inline]
    pub(crate) fn take_disposed_attached(&self, id: Key) -> bool {
        if !self.has_flag(FLAG_DISPOSED) {
            return false;
        }
        self.take_disposed_attached_slow(id)
    }

    #[cold]
    #[inline(never)]
    fn take_disposed_attached_slow(&self, id: Key) -> bool {
        let mut slot = self.cold.borrow_mut();
        let Some(cold) = slot.as_deref_mut() else {
            return false;
        };
        let Ok(pos) = cold.disposed_attached.binary_search(&id) else {
            return false;
        };
        cold.disposed_attached.remove(pos);
        self.sync_flags(cold);
        true
    }

    #[inline]
    pub(crate) fn forget_host(&self, table: &SlotTable) {
        if self.has_flag(FLAG_DISPOSED)
            && let Some(hooks) = movable_hooks()
        {
            (hooks.forget_host)(self, table);
        }
    }

    #[cold]
    #[inline(never)]
    pub(super) fn forget_disposed_in_table(&self, table: &SlotTable) {
        let mut slot = self.cold.borrow_mut();
        let Some(cold) = slot.as_deref_mut() else {
            return;
        };
        cold.disposed_attached
            .retain(|&id| !table.movable_is_attached(id));
        self.sync_flags(cold);
    }

    #[inline]
    pub(crate) fn clear_host(&self, host_key: usize, table: &SlotTable) {
        if self.has_flag(FLAG_PENDING | FLAG_DISPOSED)
            && let Some(hooks) = movable_hooks()
        {
            (hooks.clear_host)(self, host_key, table);
        }
    }

    #[cold]
    #[inline(never)]
    pub(super) fn clear_host_slow(&self, host_key: usize, table: &SlotTable) {
        let mut slot = self.cold.borrow_mut();
        let Some(cold) = slot.as_deref_mut() else {
            return;
        };
        cold.pending.retain(|(key, _)| *key != host_key);
        if !cold.disposed_attached.is_empty() {
            cold.disposed_attached
                .retain(|&id| !table.movable_is_attached(id));
        }
        self.sync_flags(cold);
    }

    #[cold]
    #[inline(never)]
    pub(crate) fn record_pending(&self, host_key: usize, pending: PendingMovable) {
        let mut slot = self.cold.borrow_mut();
        let cold = Self::ensure_cold(&mut slot);
        if cold
            .pending
            .iter()
            .all(|(key, site)| *key != host_key || site.placeholder != pending.placeholder)
        {
            cold.pending.push((host_key, pending));
        }
        self.sync_flags(cold);
    }

    #[cold]
    #[inline(never)]
    pub(crate) fn take_all_pending(&self) -> Vec<(usize, PendingMovable)> {
        if !self.has_flag(FLAG_PENDING) {
            return Vec::new();
        }
        let mut slot = self.cold.borrow_mut();
        let Some(cold) = slot.as_deref_mut() else {
            return Vec::new();
        };
        let taken = std::mem::take(&mut cold.pending);
        self.sync_flags(cold);
        taken
    }

    #[cold]
    #[inline(never)]
    pub(crate) fn restore_pending(&self, waiting: Vec<(usize, PendingMovable)>) {
        if waiting.is_empty() {
            return;
        }
        let mut slot = self.cold.borrow_mut();
        let cold = Self::ensure_cold(&mut slot);
        if cold.pending.is_empty() {
            cold.pending = waiting;
        } else {
            cold.pending.extend(waiting);
        }
        self.sync_flags(cold);
    }

    #[inline]
    pub(crate) fn has_pending(&self, id: Key) -> bool {
        if !self.has_flag(FLAG_PENDING) {
            return false;
        }
        self.has_pending_slow(id)
    }

    #[cold]
    #[inline(never)]
    fn has_pending_slow(&self, id: Key) -> bool {
        let slot = self.cold.borrow();
        slot.as_deref().is_some_and(|cold| {
            cold.pending
                .iter()
                .any(|(_, site)| site.key.movable_id() == Some(id))
        })
    }
}
