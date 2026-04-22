use super::{DetachedSubtree, SlotTable, SlotTableDebugStats};
use std::any::Any;

pub(crate) enum DeferredDrop {
    Value(Box<dyn Any>),
}

impl DeferredDrop {
    fn dispose(self) {
        match self {
            Self::Value(value) => drop(value),
        }
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
        let mut subtree = subtree;
        subtree.mark_nodes_disposed();
        for value in subtree.into_payload_values_rev() {
            self.queue_drop(DeferredDrop::Value(value));
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

    pub(crate) fn fill_debug_stats(&self, stats: &mut SlotTableDebugStats) {
        stats.pending_slot_drops_len = self.pending_drops_len();
        stats.pending_slot_drops_cap = self.pending_drops_capacity();
    }

    pub(crate) fn dispose_slot_table(&mut self, table: &mut SlotTable) {
        self.flush_pending_drops();
        let drops = table.take_all_drops();
        self.pending_drops.extend(drops);
        self.flush_pending_drops();
    }
}
