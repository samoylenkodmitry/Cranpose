//! Slot storage backend selection and unified interface.

use crate::{
    chunked_slot_storage::ChunkedSlotStorage,
    hierarchical_slot_storage::HierarchicalSlotStorage,
    slot_storage::{GroupId, SlotStorage, StartGroup, ValueSlotId},
    split_slot_storage::SplitSlotStorage,
    Key, NodeId, Owned, ScopeId, SlotTable,
};

/// Factory function to create a backend of the specified kind.
///
/// This is the main entry point for creating slot storage backends at runtime.
pub fn make_backend(kind: SlotBackendKind) -> SlotBackend {
    SlotBackend::new(kind)
}

/// Available slot storage backend implementations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[derive(Default)]
pub enum SlotBackendKind {
    /// Baseline gap-buffer implementation (default).
    #[default]
    Baseline,
    /// Chunked storage to avoid large rotate operations.
    Chunked,
    /// Hierarchical storage where groups own child storage.
    Hierarchical,
    /// Split storage separating layout from payload.
    Split,
}


/// Unified slot storage backend that can use any implementation.
pub enum SlotBackend {
    Baseline(SlotTable),
    Chunked(ChunkedSlotStorage),
    Hierarchical(HierarchicalSlotStorage),
    Split(SplitSlotStorage),
}

impl SlotBackend {
    /// Create a new backend of the specified kind.
    ///
    /// NOTE: Currently, all backend kinds map to the Baseline implementation
    /// since the experimental backends (Chunked, Hierarchical, Split) are still
    /// under development and don't pass all tests. This allows the backend
    /// infrastructure to exist while development continues.
    pub fn new(kind: SlotBackendKind) -> Self {
        match kind {
            SlotBackendKind::Baseline => Self::Baseline(SlotTable::new()),
            SlotBackendKind::Chunked => Self::Chunked(ChunkedSlotStorage::new()),
            SlotBackendKind::Hierarchical => Self::Hierarchical(HierarchicalSlotStorage::new()),
            SlotBackendKind::Split => Self::Split(SplitSlotStorage::new()),
        }
    }
}

impl Default for SlotBackend {
    fn default() -> Self {
        Self::new(SlotBackendKind::default())
    }
}

// Implement SlotStorage by delegating to the active backend
impl SlotStorage for SlotBackend {
    type Group = GroupId;
    type ValueSlot = ValueSlotId;

    fn begin_group(&mut self, key: Key) -> StartGroup<Self::Group> {
        match self {
            Self::Baseline(s) => SlotStorage::begin_group(s, key),
            Self::Chunked(s) => SlotStorage::begin_group(s, key),
            Self::Hierarchical(s) => SlotStorage::begin_group(s, key),
            Self::Split(s) => SlotStorage::begin_group(s, key),
        }
    }

    fn set_group_scope(&mut self, group: Self::Group, scope: ScopeId) {
        match self {
            Self::Baseline(s) => SlotStorage::set_group_scope(s, group, scope),
            Self::Chunked(s) => SlotStorage::set_group_scope(s, group, scope),
            Self::Hierarchical(s) => SlotStorage::set_group_scope(s, group, scope),
            Self::Split(s) => SlotStorage::set_group_scope(s, group, scope),
        }
    }

    fn end_group(&mut self) {
        match self {
            Self::Baseline(s) => s.end_group(),
            Self::Chunked(s) => s.end_group(),
            Self::Hierarchical(s) => s.end_group(),
            Self::Split(s) => s.end_group(),
        }
    }

    fn skip_current_group(&mut self) {
        match self {
            Self::Baseline(s) => s.skip_current_group(),
            Self::Chunked(s) => s.skip_current_group(),
            Self::Hierarchical(s) => s.skip_current_group(),
            Self::Split(s) => s.skip_current_group(),
        }
    }

    fn nodes_in_current_group(&self) -> Vec<NodeId> {
        match self {
            Self::Baseline(s) => s.nodes_in_current_group(),
            Self::Chunked(s) => s.nodes_in_current_group(),
            Self::Hierarchical(s) => s.nodes_in_current_group(),
            Self::Split(s) => s.nodes_in_current_group(),
        }
    }

    fn begin_recompose_at_scope(&mut self, scope: ScopeId) -> Option<Self::Group> {
        match self {
            Self::Baseline(s) => s.begin_recompose_at_scope(scope),
            Self::Chunked(s) => s.begin_recompose_at_scope(scope),
            Self::Hierarchical(s) => s.begin_recompose_at_scope(scope),
            Self::Split(s) => s.begin_recompose_at_scope(scope),
        }
    }

    fn end_recompose(&mut self) {
        match self {
            Self::Baseline(s) => s.end_recompose(),
            Self::Chunked(s) => s.end_recompose(),
            Self::Hierarchical(s) => s.end_recompose(),
            Self::Split(s) => s.end_recompose(),
        }
    }

    fn alloc_value_slot<T: 'static>(&mut self, init: impl FnOnce() -> T) -> Self::ValueSlot {
        match self {
            Self::Baseline(s) => s.alloc_value_slot(init),
            Self::Chunked(s) => s.alloc_value_slot(init),
            Self::Hierarchical(s) => s.alloc_value_slot(init),
            Self::Split(s) => s.alloc_value_slot(init),
        }
    }

    fn read_value<T: 'static>(&self, slot: Self::ValueSlot) -> &T {
        match self {
            Self::Baseline(s) => SlotStorage::read_value(s, slot),
            Self::Chunked(s) => SlotStorage::read_value(s, slot),
            Self::Hierarchical(s) => SlotStorage::read_value(s, slot),
            Self::Split(s) => SlotStorage::read_value(s, slot),
        }
    }

    fn read_value_mut<T: 'static>(&mut self, slot: Self::ValueSlot) -> &mut T {
        match self {
            Self::Baseline(s) => SlotStorage::read_value_mut(s, slot),
            Self::Chunked(s) => SlotStorage::read_value_mut(s, slot),
            Self::Hierarchical(s) => SlotStorage::read_value_mut(s, slot),
            Self::Split(s) => SlotStorage::read_value_mut(s, slot),
        }
    }

    fn write_value<T: 'static>(&mut self, slot: Self::ValueSlot, value: T) {
        match self {
            Self::Baseline(s) => SlotStorage::write_value(s, slot, value),
            Self::Chunked(s) => SlotStorage::write_value(s, slot, value),
            Self::Hierarchical(s) => SlotStorage::write_value(s, slot, value),
            Self::Split(s) => SlotStorage::write_value(s, slot, value),
        }
    }

    fn remember<T: 'static>(&mut self, init: impl FnOnce() -> T) -> Owned<T> {
        match self {
            Self::Baseline(s) => s.remember(init),
            Self::Chunked(s) => s.remember(init),
            Self::Hierarchical(s) => s.remember(init),
            Self::Split(s) => s.remember(init),
        }
    }

    fn peek_node(&self) -> Option<NodeId> {
        match self {
            Self::Baseline(s) => s.peek_node(),
            Self::Chunked(s) => s.peek_node(),
            Self::Hierarchical(s) => s.peek_node(),
            Self::Split(s) => s.peek_node(),
        }
    }

    fn record_node(&mut self, id: NodeId) {
        match self {
            Self::Baseline(s) => s.record_node(id),
            Self::Chunked(s) => s.record_node(id),
            Self::Hierarchical(s) => s.record_node(id),
            Self::Split(s) => s.record_node(id),
        }
    }

    fn advance_after_node_read(&mut self) {
        match self {
            Self::Baseline(s) => s.advance_after_node_read(),
            Self::Chunked(s) => s.advance_after_node_read(),
            Self::Hierarchical(s) => s.advance_after_node_read(),
            Self::Split(s) => s.advance_after_node_read(),
        }
    }

    fn step_back(&mut self) {
        match self {
            Self::Baseline(s) => s.step_back(),
            Self::Chunked(s) => s.step_back(),
            Self::Hierarchical(s) => s.step_back(),
            Self::Split(s) => s.step_back(),
        }
    }

    fn finalize_current_group(&mut self) -> bool {
        match self {
            Self::Baseline(s) => s.finalize_current_group(),
            Self::Chunked(s) => s.finalize_current_group(),
            Self::Hierarchical(s) => s.finalize_current_group(),
            Self::Split(s) => s.finalize_current_group(),
        }
    }

    fn reset(&mut self) {
        match self {
            Self::Baseline(s) => s.reset(),
            Self::Chunked(s) => s.reset(),
            Self::Hierarchical(s) => s.reset(),
            Self::Split(s) => s.reset(),
        }
    }

    fn flush(&mut self) {
        match self {
            Self::Baseline(s) => s.flush(),
            Self::Chunked(s) => s.flush(),
            Self::Hierarchical(s) => s.flush(),
            Self::Split(s) => s.flush(),
        }
    }
}

// Additional debug methods not in the SlotStorage trait
impl SlotBackend {
    pub fn debug_dump_groups(&self) -> Vec<(usize, Key, Option<ScopeId>, usize)> {
        match self {
            Self::Baseline(s) => s.debug_dump_groups(),
            Self::Hierarchical(s) => s.debug_dump_groups(),
            Self::Chunked(s) => s.debug_dump_groups(),
            Self::Split(s) => s.debug_dump_groups(),
        }
    }

    pub fn debug_dump_all_slots(&self) -> Vec<(usize, String)> {
        match self {
            Self::Baseline(s) => s.debug_dump_all_slots(),
            Self::Hierarchical(s) => s.debug_dump_all_slots(),
            Self::Chunked(s) => s.debug_dump_all_slots(),
            Self::Split(s) => s.debug_dump_all_slots(),
        }
    }
}
