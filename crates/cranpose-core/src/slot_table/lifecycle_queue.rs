use super::{NodeSlotState, SlotTable};
use crate::{runtime, AnchorId, NodeId, RecomposeScope};
use std::any::Any;

struct ChunkedPending<T> {
    sealed: Vec<Vec<T>>,
    active: Vec<T>,
    len: usize,
}

impl<T> Default for ChunkedPending<T> {
    fn default() -> Self {
        Self {
            sealed: Vec::new(),
            active: Vec::new(),
            len: 0,
        }
    }
}

impl<T> ChunkedPending<T> {
    fn len(&self) -> usize {
        self.len
    }

    fn capacity(&self) -> usize {
        self.active.capacity() + self.sealed.iter().map(Vec::capacity).sum::<usize>()
    }

    fn push(&mut self, value: T, initial_capacity: usize, chunk_capacity: usize) {
        if self.active.capacity() == 0 {
            self.active = Vec::with_capacity(initial_capacity);
        } else if self.active.len() == self.active.capacity() {
            let next_capacity = chunk_capacity.max(initial_capacity);
            let full_chunk = std::mem::replace(&mut self.active, Vec::with_capacity(next_capacity));
            self.sealed.push(full_chunk);
        }

        self.active.push(value);
        self.len += 1;
    }

    fn clear_and_drop_reverse(&mut self) {
        while let Some(value) = self.active.pop() {
            drop(value);
        }

        while let Some(mut chunk) = self.sealed.pop() {
            while let Some(value) = chunk.pop() {
                drop(value);
            }
        }

        self.len = 0;
    }

    fn drain_forward(&mut self, mut visitor: impl FnMut(T)) {
        let sealed = std::mem::take(&mut self.sealed);
        for chunk in sealed {
            for value in chunk {
                visitor(value);
            }
        }
        let active = std::mem::take(&mut self.active);
        for value in active {
            visitor(value);
        }
        self.len = 0;
    }

    fn trim_retained_capacity(&mut self, retained: usize, initial_capacity: usize) {
        self.sealed = Vec::new();
        if self.active.capacity() > retained.saturating_mul(4) {
            self.active = Vec::with_capacity(retained.max(initial_capacity));
        }
    }

    #[cfg(debug_assertions)]
    fn visit(&self, mut visitor: impl FnMut(&T)) {
        for chunk in &self.sealed {
            for value in chunk {
                visitor(value);
            }
        }
        for value in &self.active {
            visitor(value);
        }
    }
}

pub(crate) enum DeferredDrop {
    Group(Option<RecomposeScope>),
    Value(Box<dyn Any>),
}

impl DeferredDrop {
    fn dispose(self) {
        match self {
            Self::Group(scope) => drop(scope),
            Self::Value(value) => drop(value),
        }
    }
}

pub(in crate::slot_table) struct PendingDrops<T> {
    inner: ChunkedPending<T>,
}

impl<T> Default for PendingDrops<T> {
    fn default() -> Self {
        Self {
            inner: ChunkedPending::default(),
        }
    }
}

impl<T> PendingDrops<T> {
    const CHUNK_CAPACITY: usize = 1024;
    const INITIAL_CAPACITY: usize = 4;

    pub(in crate::slot_table) fn len(&self) -> usize {
        self.inner.len()
    }

    pub(in crate::slot_table) fn capacity(&self) -> usize {
        self.inner.capacity()
    }

    pub(in crate::slot_table) fn push(&mut self, value: T) {
        self.inner
            .push(value, Self::INITIAL_CAPACITY, Self::CHUNK_CAPACITY);
    }

    pub(in crate::slot_table) fn clear_and_drop_reverse(&mut self) {
        let _teardown = runtime::enter_state_teardown_scope();
        self.inner.clear_and_drop_reverse();
    }

    pub(in crate::slot_table) fn trim_retained_capacity(&mut self, retained: usize) {
        self.inner
            .trim_retained_capacity(retained, Self::INITIAL_CAPACITY);
    }
}

#[derive(Default)]
pub(in crate::slot_table) struct OrphanedNodeIds {
    inner: ChunkedPending<OrphanedNode>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OrphanedNode {
    pub id: NodeId,
    pub generation: u32,
    pub(crate) anchor: AnchorId,
}

impl OrphanedNode {
    pub(in crate::slot_table) fn new(id: NodeId, generation: u32, anchor: AnchorId) -> Self {
        Self {
            id,
            generation,
            anchor,
        }
    }
}

impl OrphanedNodeIds {
    const CHUNK_CAPACITY: usize = 1024;
    pub(in crate::slot_table) const INITIAL_CAPACITY: usize = 32;

    pub(in crate::slot_table) fn len(&self) -> usize {
        self.inner.len()
    }

    pub(in crate::slot_table) fn capacity(&self) -> usize {
        self.inner.capacity()
    }

    pub(in crate::slot_table) fn push(&mut self, orphaned: OrphanedNode) {
        self.inner
            .push(orphaned, Self::INITIAL_CAPACITY, Self::CHUNK_CAPACITY);
    }

    pub(in crate::slot_table) fn drain_forward(&mut self, visitor: impl FnMut(OrphanedNode)) {
        self.inner.drain_forward(visitor);
        self.trim_retained_capacity(Self::INITIAL_CAPACITY);
    }

    pub(in crate::slot_table) fn trim_retained_capacity(&mut self, retained: usize) {
        self.inner
            .trim_retained_capacity(retained, Self::INITIAL_CAPACITY);
    }

    #[cfg(debug_assertions)]
    pub(in crate::slot_table) fn snapshot(&self) -> Vec<OrphanedNode> {
        let mut orphaned = Vec::with_capacity(self.len());
        self.inner.visit(|node| orphaned.push(*node));
        orphaned
    }
}

#[derive(Default)]
pub(crate) struct SlotLifecycleCoordinator {
    pending_drops: PendingDrops<DeferredDrop>,
    orphaned_node_ids: OrphanedNodeIds,
    preserved_orphaned_node_ids: OrphanedNodeIds,
}

impl SlotLifecycleCoordinator {
    pub(crate) fn pending_drops_len(&self) -> usize {
        self.pending_drops.len()
    }

    pub(crate) fn pending_drops_capacity(&self) -> usize {
        self.pending_drops.capacity()
    }

    pub(crate) fn orphaned_node_ids_len(&self) -> usize {
        self.orphaned_node_ids.len()
    }

    pub(crate) fn orphaned_node_ids_capacity(&self) -> usize {
        self.orphaned_node_ids.capacity()
    }

    pub(crate) fn deactivate_scope(&self, scope: &RecomposeScope) {
        scope.deactivate();
    }

    pub(crate) fn push_drop(&mut self, deferred_drop: DeferredDrop) {
        self.pending_drops.push(deferred_drop);
    }

    pub(crate) fn queue_orphaned_node(&mut self, orphaned: OrphanedNode) {
        self.orphaned_node_ids.push(orphaned);
    }

    pub(crate) fn preserve_orphaned_node(&mut self, orphaned: OrphanedNode) {
        self.preserved_orphaned_node_ids.push(orphaned);
    }

    pub(crate) fn drain_orphaned_node_ids_with(&mut self, visitor: impl FnMut(OrphanedNode)) {
        self.orphaned_node_ids.drain_forward(visitor);
    }

    pub(crate) fn drain_orphaned_node_ids(&mut self) -> Vec<OrphanedNode> {
        let mut orphaned = Vec::with_capacity(self.orphaned_node_ids.len());
        self.drain_orphaned_node_ids_with(|node| orphaned.push(node));
        orphaned
    }

    pub(crate) fn flush_pending_drops(&mut self) {
        self.pending_drops.clear_and_drop_reverse();
        let retained = self.pending_drops.len().max(4);
        self.pending_drops.trim_retained_capacity(retained);
    }

    pub(crate) fn reconcile_orphaned_nodes(&mut self, table: &SlotTable) {
        let preserved = self.drain_preserved_orphaned_node_ids();
        for orphaned in preserved {
            match table.orphaned_node_state(orphaned) {
                NodeSlotState::Active => {}
                NodeSlotState::PreservedGap => self.preserve_orphaned_node(orphaned),
                NodeSlotState::Missing => self.queue_orphaned_node(orphaned),
            }
        }
    }

    pub(crate) fn dispose_drops_reverse(&mut self, mut drops: Vec<DeferredDrop>) {
        let _teardown = runtime::enter_state_teardown_scope();
        while let Some(deferred_drop) = drops.pop() {
            deferred_drop.dispose();
        }
    }

    pub(crate) fn trim_orphaned_node_capacity(&mut self, retained: usize) {
        self.orphaned_node_ids.trim_retained_capacity(retained);
    }

    #[cfg(debug_assertions)]
    pub(crate) fn orphaned_node_ids_snapshot(&self) -> Vec<OrphanedNode> {
        self.orphaned_node_ids.snapshot()
    }

    #[cfg(debug_assertions)]
    pub(crate) fn preserved_orphaned_node_ids_snapshot(&self) -> Vec<OrphanedNode> {
        self.preserved_orphaned_node_ids.snapshot()
    }

    pub(crate) fn clear_orphaned_nodes(&mut self) {
        let _ = self.drain_orphaned_node_ids();
        let _ = self.drain_preserved_orphaned_node_ids();
        self.trim_orphaned_node_capacity(32);
        self.preserved_orphaned_node_ids.trim_retained_capacity(32);
    }

    fn drain_preserved_orphaned_node_ids(&mut self) -> Vec<OrphanedNode> {
        let mut orphaned = Vec::with_capacity(self.preserved_orphaned_node_ids.len());
        self.preserved_orphaned_node_ids
            .drain_forward(|node| orphaned.push(node));
        orphaned
    }
}
