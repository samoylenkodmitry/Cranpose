//! Hierarchical slot storage where groups can own isolated child storage.
//!
//! This backend provides infrastructure for groups to have their own isolated
//! slot storage, preventing sibling groups from affecting each other during
//! recomposition. Currently uses a simple delegation model where all operations
//! go to the current storage (root or child).
//!
//! ## Implementation Details
//!
//! - **Root storage**: A `SlotTable` for top-level composition
//! - **Child storages**: `HashMap<usize, SlotTable>` for isolated subtrees
//! - **Storage stack**: Tracks which storage is currently active, allowing
//!   operations to be routed to the correct store
//! - **Current implementation**: All groups use the current storage (no automatic
//!   child allocation). Provides infrastructure for future heuristics.
//! - **Recomposition**: `begin_recompose_at_scope` searches both root and child
//!   stores, switching context when a scope is found in a child store
//!
//! ## Future Work
//!
//! TODO: Implement heuristic for automatic child storage allocation. Potential strategies:
//! - Allocate child storage for groups at depth >= 2
//! - Allocate based on specific key patterns or annotations
//! - Provide explicit API for requesting isolated storage
//!
//! ## Trade-offs
//!
//! - **Pros**: Perfect isolation between subtrees (when implemented), better
//!   cache locality for large compositions
//! - **Cons**: Higher memory overhead (multiple SlotTables), complexity in
//!   managing storage lifecycle and routing

use crate::{
    slot_storage::{GroupId, SlotStorage, StartGroup, ValueSlotId},
    Key, NodeId, Owned, ScopeId, SlotTable,
};
use std::collections::HashMap;

/// Hierarchical slot storage implementation.
///
/// Currently operates over the root SlotTable only; child storage isolation
/// is a future extension. Infrastructure for child stores exists but is not
/// automatically activated. Groups can potentially own their own child storage
/// to isolate recomposition changes to only the affected subtree.
#[derive(Default)]
pub struct HierarchicalSlotStorage {
    /// Root storage for top-level composition.
    root: SlotTable,
    /// Child storages owned by groups, indexed by group ID.
    child_stores: HashMap<usize, SlotTable>,
    /// Stack tracking which storage we're currently using.
    storage_stack: Vec<StorageFrame>,
    /// Counter for allocating child storage IDs.
    next_child_id: usize,
}

/// Frame tracking which storage is currently active in the hierarchy.
struct StorageFrame {
    /// Which child storage index we switched to (None = root).
    child_store_id: Option<usize>,
    /// Group ID that owns this storage.
    #[allow(dead_code)] // Tracked for debugging/future child storage management
    group_id: usize,
}

impl HierarchicalSlotStorage {
    pub fn new() -> Self {
        Self {
            root: SlotTable::new(),
            ..Default::default()
        }
    }

    /// Get a reference to the current storage (root or child).
    fn current_storage(&self) -> &SlotTable {
        if let Some(frame) = self.storage_stack.last() {
            if let Some(id) = frame.child_store_id {
                return self.child_stores.get(&id).unwrap();
            }
        }
        &self.root
    }

    /// Get a mutable reference to the current storage.
    fn current_storage_mut(&mut self) -> &mut SlotTable {
        if let Some(frame) = self.storage_stack.last() {
            if let Some(id) = frame.child_store_id {
                return self.child_stores.get_mut(&id).unwrap();
            }
        }
        &mut self.root
    }

    /// Allocate a new child storage for a group.
    ///
    /// Infrastructure for future hierarchical storage feature where groups can own
    /// isolated storage to prevent sibling recomposition interference.
    #[allow(dead_code)] // Planned for automatic child storage allocation heuristics
    fn alloc_child_storage(&mut self, group_id: usize) -> usize {
        let id = self.next_child_id;
        self.next_child_id += 1;
        self.child_stores.insert(id, SlotTable::new());

        // Push storage frame to track that we're now operating in this child storage
        self.storage_stack.push(StorageFrame {
            child_store_id: Some(id),
            group_id,
        });

        id
    }

    /// Pop the storage stack when exiting a child storage.
    ///
    /// Pairs with alloc_child_storage for future hierarchical storage feature.
    #[allow(dead_code)] // Planned for automatic child storage allocation heuristics
    fn pop_child_storage(&mut self) {
        if let Some(frame) = self.storage_stack.last() {
            if frame.child_store_id.is_some() {
                self.storage_stack.pop();
            }
        }
    }
}

impl SlotStorage for HierarchicalSlotStorage {
    type Group = GroupId;
    type ValueSlot = ValueSlotId;

    fn begin_group(&mut self, key: Key) -> StartGroup<Self::Group> {
        // Begin group in current storage using trait method
        let result = SlotStorage::begin_group(self.current_storage_mut(), key);

        // TODO: Implement heuristic for when to allocate child storage.
        // Potential strategies:
        // 1. Allocate child storage for groups at depth >= 2
        // 2. Allocate based on specific key patterns
        // 3. Allow explicit API to request isolated storage
        //
        // For now, all groups use the current storage (root or inherited child).
        // This still allows recomposition to switch to child stores via
        // begin_recompose_at_scope.

        result
    }

    fn set_group_scope(&mut self, group: Self::Group, scope: ScopeId) {
        // Delegate through the SlotStorage trait, not inherent methods
        SlotStorage::set_group_scope(self.current_storage_mut(), group, scope);
    }

    fn end_group(&mut self) {
        SlotStorage::end_group(self.current_storage_mut());

        // TODO: When we implement automatic child storage allocation,
        // we need to pop the storage_stack here when ending a group
        // that owns a child storage. For now, this is a no-op since
        // we don't automatically allocate child stores.
    }

    fn skip_current_group(&mut self) {
        SlotStorage::skip_current_group(self.current_storage_mut());
    }

    fn nodes_in_current_group(&self) -> Vec<NodeId> {
        SlotStorage::nodes_in_current_group(self.current_storage())
    }

    fn begin_recompose_at_scope(&mut self, scope: ScopeId) -> Option<Self::Group> {
        // Try root storage first
        if let Some(group) = SlotStorage::begin_recompose_at_scope(&mut self.root, scope) {
            return Some(group);
        }

        // Search child storages
        for (id, storage) in &mut self.child_stores {
            if let Some(group) = SlotStorage::begin_recompose_at_scope(storage, scope) {
                // Push a frame to indicate we're in this child storage
                self.storage_stack.push(StorageFrame {
                    child_store_id: Some(*id),
                    group_id: 0, // Would need to track properly
                });
                return Some(group);
            }
        }

        None
    }

    fn end_recompose(&mut self) {
        if let Some(frame) = self.storage_stack.last() {
            if frame.child_store_id.is_some() {
                self.storage_stack.pop();
                return;
            }
        }
        SlotStorage::end_recompose(&mut self.root);
    }

    fn alloc_value_slot<T: 'static>(&mut self, init: impl FnOnce() -> T) -> Self::ValueSlot {
        SlotStorage::alloc_value_slot(self.current_storage_mut(), init)
    }

    fn read_value<T: 'static>(&self, slot: Self::ValueSlot) -> &T {
        SlotStorage::read_value(self.current_storage(), slot)
    }

    fn read_value_mut<T: 'static>(&mut self, slot: Self::ValueSlot) -> &mut T {
        SlotStorage::read_value_mut(self.current_storage_mut(), slot)
    }

    fn write_value<T: 'static>(&mut self, slot: Self::ValueSlot, value: T) {
        SlotStorage::write_value(self.current_storage_mut(), slot, value);
    }

    fn remember<T: 'static>(&mut self, init: impl FnOnce() -> T) -> Owned<T> {
        SlotStorage::remember(self.current_storage_mut(), init)
    }

    fn peek_node(&self) -> Option<NodeId> {
        SlotStorage::peek_node(self.current_storage())
    }

    fn record_node(&mut self, id: NodeId) {
        SlotStorage::record_node(self.current_storage_mut(), id);
    }

    fn advance_after_node_read(&mut self) {
        SlotStorage::advance_after_node_read(self.current_storage_mut());
    }

    fn step_back(&mut self) {
        SlotStorage::step_back(self.current_storage_mut());
    }

    fn finalize_current_group(&mut self) -> bool {
        SlotStorage::finalize_current_group(self.current_storage_mut())
    }

    fn reset(&mut self) {
        SlotStorage::reset(&mut self.root);
        for storage in self.child_stores.values_mut() {
            SlotStorage::reset(storage);
        }
        self.storage_stack.clear();
    }

    fn flush(&mut self) {
        SlotStorage::flush(&mut self.root);
        for storage in self.child_stores.values_mut() {
            SlotStorage::flush(storage);
        }
    }
}

impl HierarchicalSlotStorage {
    /// Debug method to dump all groups from the current storage.
    pub fn debug_dump_groups(&self) -> Vec<(usize, Key, Option<ScopeId>, usize)> {
        // For now, just return groups from root storage
        // TODO: Consider including child storage groups with offset indices
        self.root.debug_dump_groups()
    }

    /// Debug method to dump all slots from the current storage.
    pub fn debug_dump_all_slots(&self) -> Vec<(usize, String)> {
        // For now, just return slots from root storage
        // TODO: Consider including child storage slots with offset indices
        self.root.debug_dump_all_slots()
    }
}
