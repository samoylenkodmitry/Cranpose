use super::{DetachedSubtree, SlotLifecycleDebugStats, SlotTable};
use std::any::Any;

/// Coordinates deferred payload disposal for the slot lifecycle described in
/// `docs/SLOT_TABLE_LIFECYCLE.md`.
pub(crate) struct DeferredDrop {
    value: Box<dyn Any>,
}

impl DeferredDrop {
    pub(crate) fn payload(value: Box<dyn Any>) -> Self {
        Self { value }
    }

    fn dispose(self) {
        drop(self.value);
    }
}

#[derive(Default)]
pub(crate) struct SlotLifecycleCoordinator {
    pending_drops: Vec<DeferredDrop>,
}

impl SlotLifecycleCoordinator {
    pub(crate) fn queue_drop(&mut self, drop: DeferredDrop) {
        self.pending_drops.push(drop);
    }

    pub(crate) fn queue_subtree_disposal(&mut self, subtree: DetachedSubtree) {
        for drop in subtree.into_payload_drops_rev() {
            self.queue_drop(drop);
        }
    }

    pub(crate) fn flush_pending_drops(&mut self) {
        while let Some(drop) = self.pending_drops.pop() {
            drop.dispose();
        }
    }

    pub(crate) fn compact_storage(&mut self) {
        if self.pending_drops.is_empty() {
            self.pending_drops.shrink_to_fit();
        }
    }

    pub(crate) fn pending_drops_len(&self) -> usize {
        self.pending_drops.len()
    }

    pub(crate) fn pending_drops_capacity(&self) -> usize {
        self.pending_drops.capacity()
    }

    pub(crate) fn debug_stats(&self) -> SlotLifecycleDebugStats {
        SlotLifecycleDebugStats {
            pending_drop_count: self.pending_drops_len(),
            pending_drop_capacity: self.pending_drops_capacity(),
        }
    }

    pub(crate) fn dispose_slot_table(&mut self, table: &mut SlotTable) {
        self.flush_pending_drops();
        let drops = table.take_all_drops();
        self.pending_drops.extend(drops);
        self.flush_pending_drops();
    }
}
