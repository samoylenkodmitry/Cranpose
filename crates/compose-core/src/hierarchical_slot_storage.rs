//! Hierarchical slot storage wrapper.
//!
//! This backend wraps `SlotTable` and provides a foundation for future
//! hierarchical storage features where groups could own isolated child storage.
//! Currently, all operations delegate directly to the root `SlotTable`.

use crate::{
    slot_storage::{GroupId, SlotStorage, StartGroup, ValueSlotId},
    Key, NodeId, Owned, ScopeId, SlotTable,
};

/// Hierarchical slot storage implementation.
///
/// Currently a thin wrapper over `SlotTable`. Future versions may support
/// isolated child storage for subtrees to prevent sibling recomposition interference.
#[derive(Default)]
pub struct HierarchicalSlotStorage {
    /// Root storage for all composition.
    root: SlotTable,
}

impl HierarchicalSlotStorage {
    pub fn new() -> Self {
        Self {
            root: SlotTable::new(),
        }
    }
}

impl SlotStorage for HierarchicalSlotStorage {
    type Group = GroupId;
    type ValueSlot = ValueSlotId;

    fn begin_group(&mut self, key: Key) -> StartGroup<Self::Group> {
        SlotStorage::begin_group(&mut self.root, key)
    }

    fn set_group_scope(&mut self, group: Self::Group, scope: ScopeId) {
        SlotStorage::set_group_scope(&mut self.root, group, scope);
    }

    fn end_group(&mut self) {
        SlotStorage::end_group(&mut self.root);
    }

    fn skip_current_group(&mut self) {
        SlotStorage::skip_current_group(&mut self.root);
    }

    fn nodes_in_current_group(&self) -> Vec<NodeId> {
        SlotStorage::nodes_in_current_group(&self.root)
    }

    fn begin_recompose_at_scope(&mut self, scope: ScopeId) -> Option<Self::Group> {
        SlotStorage::begin_recompose_at_scope(&mut self.root, scope)
    }

    fn end_recompose(&mut self) {
        SlotStorage::end_recompose(&mut self.root);
    }

    fn alloc_value_slot<T: 'static>(&mut self, init: impl FnOnce() -> T) -> Self::ValueSlot {
        SlotStorage::alloc_value_slot(&mut self.root, init)
    }

    fn read_value<T: 'static>(&self, slot: Self::ValueSlot) -> &T {
        SlotStorage::read_value(&self.root, slot)
    }

    fn read_value_mut<T: 'static>(&mut self, slot: Self::ValueSlot) -> &mut T {
        SlotStorage::read_value_mut(&mut self.root, slot)
    }

    fn write_value<T: 'static>(&mut self, slot: Self::ValueSlot, value: T) {
        SlotStorage::write_value(&mut self.root, slot, value);
    }

    fn remember<T: 'static>(&mut self, init: impl FnOnce() -> T) -> Owned<T> {
        SlotStorage::remember(&mut self.root, init)
    }

    fn peek_node(&self) -> Option<NodeId> {
        SlotStorage::peek_node(&self.root)
    }

    fn record_node(&mut self, id: NodeId) {
        SlotStorage::record_node(&mut self.root, id);
    }

    fn advance_after_node_read(&mut self) {
        SlotStorage::advance_after_node_read(&mut self.root);
    }

    fn step_back(&mut self) {
        SlotStorage::step_back(&mut self.root);
    }

    fn finalize_current_group(&mut self) -> bool {
        SlotStorage::finalize_current_group(&mut self.root)
    }

    fn reset(&mut self) {
        SlotStorage::reset(&mut self.root);
    }

    fn flush(&mut self) {
        SlotStorage::flush(&mut self.root);
    }
}

impl HierarchicalSlotStorage {
    /// Debug method to dump all groups.
    pub fn debug_dump_groups(&self) -> Vec<(usize, Key, Option<ScopeId>, usize)> {
        self.root.debug_dump_groups()
    }

    /// Debug method to dump all slots.
    pub fn debug_dump_all_slots(&self) -> Vec<(usize, String)> {
        self.root.debug_dump_all_slots()
    }
}
