use std::{any::TypeId, cmp::Reverse, collections::BinaryHeap};

use super::MemoryApplier;
use crate::{Applier, Node, NodeError, NodeId, RecycleOrigin, RecycledNode, RecycledNodeInsertion};

impl Applier for MemoryApplier {
    fn record_structural_change(&mut self, parent_id: NodeId) {
        if self.state.structural_change_parents.last() != Some(&parent_id) {
            self.state.structural_change_parents.push(parent_id);
        }
    }

    fn create(&mut self, node: Box<dyn Node>) -> NodeId {
        let stable_id = self.state.next_stable_id;
        self.state.next_stable_id = self.state.next_stable_id.saturating_add(1);
        if stable_id >= Self::HIGH_ID_THRESHOLD {
            self.insert_high_id_node(stable_id, node, RecycleOrigin::Cold);
            return stable_id;
        }

        let physical_id = if let Some(Reverse(id)) = self.state.free_ids.pop() {
            debug_assert!(
                self.state.nodes[id].is_none(),
                "freelist entry {id} is not None"
            );
            self.state.nodes[id] = Some(node);
            self.state.physical_stable_ids[id] = Self::pack_stable_id(stable_id);
            self.state.physical_warm_recycled_origins[id] = RecycleOrigin::Cold;
            id
        } else {
            self.ensure_dense_node_storage_capacity();
            let id = self.state.nodes.len();
            self.state.nodes.push(Some(node));
            self.state
                .physical_stable_ids
                .push(Self::pack_stable_id(stable_id));
            self.state
                .physical_warm_recycled_origins
                .push(RecycleOrigin::Cold);
            id
        };
        self.state.stable_index.insert_fresh(stable_id, physical_id);
        stable_id
    }

    fn node_generation(&self, id: NodeId) -> u32 {
        if id >= Self::HIGH_ID_THRESHOLD {
            return self
                .state
                .high_ids
                .get(&id)
                .map_or(0, |slot| slot.generation);
        }
        self.state.stable_index.generation(id)
    }

    #[inline]
    fn get_mut(&mut self, id: NodeId) -> Result<&mut dyn Node, NodeError> {
        match self.resolve_node_index(id) {
            Some(physical_id) => self
                .state
                .nodes
                .get_mut(physical_id)
                .and_then(Option::as_deref_mut)
                .ok_or(NodeError::Missing { id }),
            None => self.high_id_node_mut(id),
        }
    }

    fn remove(&mut self, id: NodeId) -> Result<(), NodeError> {
        self.state.bubble_passes.forget();
        self.remove_subtree_postorder(id).map(|_| ())
    }

    fn begin_bubble_batch(&mut self) {
        self.state.bubble_passes.begin_batch();
    }

    fn note_layout_dirt(&mut self, id: NodeId) {
        self.state.layout_dirt_roots.note(id);
    }

    fn end_bubble_batch(&mut self) {
        self.state.bubble_passes.end_batch();
    }

    fn forget_bubble_passes(&mut self) {
        self.state.bubble_passes.forget();
    }

    fn note_bubble_pass(&mut self, id: NodeId, flags: u8) -> bool {
        self.resolve_node_index(id)
            .is_some_and(|index| self.state.bubble_passes.note(index, flags))
    }

    /// One lookup of the node's slot serves the pass note and the node.
    fn bubble_node(&mut self, id: NodeId, flags: u8, note: bool) -> Option<&mut dyn Node> {
        let Some(index) = self.resolve_node_index(id) else {
            return self
                .state
                .high_ids
                .get_mut(&id)
                .and_then(|slot| slot.node.as_deref_mut());
        };
        if note && self.state.bubble_passes.note(index, flags) {
            return None;
        }
        self.state.nodes.get_mut(index)?.as_deref_mut()
    }

    fn get_ref(&self, id: NodeId) -> Result<&dyn Node, NodeError> {
        Self::get_ref(self, id)
    }

    fn create_virtual(&mut self, node: Box<dyn Node>) -> NodeId {
        let id = self.create(node);
        self.state.virtual_node_ids.insert(id);
        id
    }

    fn insert_recycled_node_or_create(
        &mut self,
        stable_id: NodeId,
        node: Box<dyn Node>,
    ) -> RecycledNodeInsertion {
        if self.contains_node_id(stable_id) {
            let id = self.create(node);
            return RecycledNodeInsertion::fresh(
                id,
                Some(NodeError::AlreadyExists { id: stable_id }),
            );
        }

        self.insert_available_with_id(stable_id, node);
        RecycledNodeInsertion::reused(stable_id)
    }

    fn compact(&mut self) {
        let live_count = self
            .state
            .nodes
            .iter()
            .filter(|slot| slot.is_some())
            .count();
        let tombstone_count = self.state.nodes.len().saturating_sub(live_count);
        if tombstone_count == 0 {
            return;
        }
        if self.state.nodes.len() > Self::EAGER_COMPACT_NODE_LEN && tombstone_count < live_count {
            return;
        }
        let rehouse_live_nodes = tombstone_count >= live_count;
        let mut packed_nodes = Vec::with_capacity(live_count);
        let mut packed_physical_stable_ids = Vec::with_capacity(live_count);
        let mut packed_warm_recycled_origins = Vec::with_capacity(live_count);

        for physical_id in 0..self.state.nodes.len() {
            let Some(mut node) = self.state.nodes[physical_id].take() else {
                continue;
            };
            if rehouse_live_nodes && let Some(rehoused) = node.rehouse_for_live_compaction() {
                node = rehoused;
            }
            let stable_id = std::mem::replace(
                &mut self.state.physical_stable_ids[physical_id],
                Self::INVALID_STABLE_ID,
            );
            debug_assert_ne!(
                stable_id,
                Self::INVALID_STABLE_ID,
                "live physical slot must have a stable id",
            );
            let stable_id = Self::unpack_stable_id(stable_id);
            packed_nodes.push(Some(node));
            packed_physical_stable_ids.push(Self::pack_stable_id(stable_id));
            packed_warm_recycled_origins
                .push(self.state.physical_warm_recycled_origins[physical_id]);
            self.state
                .stable_index
                .set_slot(stable_id, packed_nodes.len() - 1);
        }

        self.state.nodes = packed_nodes;
        self.state.physical_stable_ids = packed_physical_stable_ids;
        self.state.physical_warm_recycled_origins = packed_warm_recycled_origins;
        self.state.free_ids = BinaryHeap::new();
        debug_assert_eq!(
            self.state.stable_index.live(),
            live_count,
            "every live node keeps one indexed slot through compaction",
        );
        self.prune_stable_generations();
    }

    fn take_recycled_node(&mut self, key: TypeId) -> Option<RecycledNode> {
        self.state.recycle_pools.get_mut(&key)?.take()
    }

    fn set_recycled_node_origin(&mut self, id: NodeId, origin: RecycleOrigin) {
        if let Some(physical_id) = self.resolve_node_index(id) {
            self.state.physical_warm_recycled_origins[physical_id] = origin;
        } else if let Some(slot) = self.state.high_ids.get_mut(&id)
            && slot.node.is_some()
        {
            slot.origin = origin;
        }
    }

    fn seed_recycled_node_shell(
        &mut self,
        key: TypeId,
        recycle_pool_limit: Option<usize>,
        shell: Box<dyn Node>,
    ) {
        self.seed_recycled_node_shell_impl(key, recycle_pool_limit, shell);
    }

    fn record_fresh_recyclable_creation(&mut self, key: TypeId) {
        self.state
            .recycle_pools
            .entry(key)
            .or_default()
            .fresh_creations += 1;
    }

    fn clear_recycled_nodes(&mut self) {
        let next_stable_id = &mut self.state.next_stable_id;
        for pool in self.state.recycle_pools.values_mut() {
            pool.end_pass(next_stable_id);
        }
        self.prune_stable_generations();
        self.compact();
    }
}
