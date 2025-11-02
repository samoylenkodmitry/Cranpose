//! State tracking for measure-time subcomposition.
//!
//! The [`SubcomposeState`] keeps book of which slots are active, which nodes can
//! be reused, and which precompositions need to be disposed. Reuse follows a
//! two-phase lookup: first [`SlotId`]s that match exactly are preferred. If no
//! exact match exists, the [`SlotReusePolicy`] is consulted to determine whether
//! a node produced for another slot is compatible with the requested slot.

use crate::collections::map::HashMap; // FUTURE(no_std): replace HashMap/HashSet with arena-backed maps.
use crate::collections::map::HashSet;
use std::fmt;

use crate::{NodeId, RecomposeScope};

/// Identifier for a subcomposed slot.
///
/// This mirrors the `slotId` concept in Jetpack Compose where callers provide
/// stable identifiers for reusable children during measure-time composition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SlotId(pub u64);

impl SlotId {
    #[inline]
    pub fn new(raw: u64) -> Self {
        Self(raw)
    }

    #[inline]
    pub fn raw(self) -> u64 {
        self.0
    }
}

/// Policy that decides which previously composed slots should be retained for
/// potential reuse during the next subcompose pass.
pub trait SlotReusePolicy: Send + Sync + 'static {
    /// Returns the subset of slots that should be retained for reuse after the
    /// current measurement pass. Slots that are not part of the returned set
    /// will be disposed.
    fn get_slots_to_retain(&self, active: &[SlotId]) -> HashSet<SlotId>;

    /// Determines whether a node that previously rendered the slot `existing`
    /// can be reused when the caller requests `requested`.
    ///
    /// Implementations should document what constitutes compatibility (for
    /// example, identical slot identifiers, matching layout classes, or node
    /// types). Returning `true` allows [`SubcomposeState`] to migrate the node
    /// across slots instead of disposing it.
    fn are_compatible(&self, existing: SlotId, requested: SlotId) -> bool;
}

/// Default reuse policy that mirrors Jetpack Compose behaviour: dispose
/// everything from the tail so that the next measurement can decide which
/// content to keep alive. Compatibility defaults to exact slot matches.
#[derive(Debug, Default)]
pub struct DefaultSlotReusePolicy;

impl SlotReusePolicy for DefaultSlotReusePolicy {
    fn get_slots_to_retain(&self, active: &[SlotId]) -> HashSet<SlotId> {
        let _ = active;
        HashSet::default()
    }

    fn are_compatible(&self, existing: SlotId, requested: SlotId) -> bool {
        existing == requested
    }
}

#[derive(Default, Clone)]
struct NodeSlotMapping {
    slot_to_nodes: HashMap<SlotId, Vec<NodeId>>, // FUTURE(no_std): replace HashMap/Vec with arena-managed storage.
    node_to_slot: HashMap<NodeId, SlotId>,       // FUTURE(no_std): migrate to slab-backed map.
    slot_to_scopes: HashMap<SlotId, Vec<RecomposeScope>>, // FUTURE(no_std): use arena-backed scope lists.
}

impl fmt::Debug for NodeSlotMapping {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NodeSlotMapping")
            .field("slot_to_nodes", &self.slot_to_nodes)
            .field("node_to_slot", &self.node_to_slot)
            .finish()
    }
}

impl NodeSlotMapping {
    fn set_nodes(&mut self, slot: SlotId, nodes: &[NodeId]) {
        self.slot_to_nodes.insert(slot, nodes.to_vec());
        for node in nodes {
            self.node_to_slot.insert(*node, slot);
        }
    }

    fn set_scopes(&mut self, slot: SlotId, scopes: &[RecomposeScope]) {
        self.slot_to_scopes.insert(slot, scopes.to_vec());
    }

    fn add_node(&mut self, slot: SlotId, node: NodeId) {
        self.slot_to_nodes.entry(slot).or_default().push(node);
        self.node_to_slot.insert(node, slot);
    }

    fn remove_by_node(&mut self, node: &NodeId) -> Option<SlotId> {
        if let Some(slot) = self.node_to_slot.remove(node) {
            if let Some(nodes) = self.slot_to_nodes.get_mut(&slot) {
                if let Some(index) = nodes.iter().position(|candidate| candidate == node) {
                    nodes.remove(index);
                }
                if nodes.is_empty() {
                    self.slot_to_nodes.remove(&slot);
                }
            }
            Some(slot)
        } else {
            None
        }
    }

    fn get_nodes(&self, slot: &SlotId) -> Option<&[NodeId]> {
        self.slot_to_nodes.get(slot).map(|nodes| nodes.as_slice())
    }

    fn get_slot(&self, node: &NodeId) -> Option<SlotId> {
        self.node_to_slot.get(node).copied()
    }

    fn deactivate_slot(&self, slot: SlotId) {
        if let Some(scopes) = self.slot_to_scopes.get(&slot) {
            for scope in scopes {
                scope.deactivate();
            }
        }
    }
}

/// Tracks the state of nodes produced by subcomposition, enabling reuse between
/// measurement passes.
pub struct SubcomposeState {
    mapping: NodeSlotMapping,
    active_order: Vec<SlotId>, // FUTURE(no_std): replace Vec with bounded ordering buffer.
    reusable_nodes: Vec<NodeId>, // FUTURE(no_std): replace Vec with stack-allocated node buffer.
    precomposed_nodes: HashMap<SlotId, Vec<NodeId>>, // FUTURE(no_std): use arena-backed precomposition lists.
    policy: Box<dyn SlotReusePolicy>,
    pub(crate) current_index: usize,
    pub(crate) reusable_count: usize,
    pub(crate) precomposed_count: usize,
}

impl fmt::Debug for SubcomposeState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SubcomposeState")
            .field("mapping", &self.mapping)
            .field("active_order", &self.active_order)
            .field("reusable_nodes", &self.reusable_nodes)
            .field("precomposed_nodes", &self.precomposed_nodes)
            .field("current_index", &self.current_index)
            .field("reusable_count", &self.reusable_count)
            .field("precomposed_count", &self.precomposed_count)
            .finish()
    }
}

impl Default for SubcomposeState {
    fn default() -> Self {
        Self::new(Box::new(DefaultSlotReusePolicy))
    }
}

impl SubcomposeState {
    /// Creates a new [`SubcomposeState`] using the supplied reuse policy.
    pub fn new(policy: Box<dyn SlotReusePolicy>) -> Self {
        Self {
            mapping: NodeSlotMapping::default(),
            active_order: Vec::new(),
            reusable_nodes: Vec::new(),
            precomposed_nodes: HashMap::default(), // FUTURE(no_std): initialize arena-backed precomposition map.
            policy,
            current_index: 0,
            reusable_count: 0,
            precomposed_count: 0,
        }
    }

    /// Sets the policy used for future reuse decisions.
    pub fn set_policy(&mut self, policy: Box<dyn SlotReusePolicy>) {
        self.policy = policy;
    }

    /// Records that the nodes in `node_ids` are currently rendering the provided
    /// `slot_id`.
    pub fn register_active(
        &mut self,
        slot_id: SlotId,
        node_ids: &[NodeId],
        scopes: &[RecomposeScope],
    ) {
        if let Some(position) = self.active_order.iter().position(|slot| *slot == slot_id) {
            self.active_order.remove(position);
        }
        for scope in scopes {
            scope.reactivate();
        }
        self.mapping.set_nodes(slot_id, node_ids);
        self.mapping.set_scopes(slot_id, scopes);
        if let Some(nodes) = self.precomposed_nodes.get_mut(&slot_id) {
            nodes.retain(|node| !node_ids.contains(node));
            if nodes.is_empty() {
                self.precomposed_nodes.remove(&slot_id);
            }
        }
        self.precomposed_count = self
            .precomposed_nodes
            .values()
            .map(|nodes| nodes.len())
            .sum();
        self.active_order.push(slot_id);
        self.current_index = self.active_order.len();
    }

    /// Stores a precomposed node for the provided slot. Precomposed nodes stay
    /// detached from the tree until they are activated by `register_active`.
    pub fn register_precomposed(&mut self, slot_id: SlotId, node_id: NodeId) {
        self.precomposed_nodes
            .entry(slot_id)
            .or_default()
            .push(node_id);
        self.precomposed_count = self
            .precomposed_nodes
            .values()
            .map(|nodes| nodes.len())
            .sum();
    }

    /// Returns the node that previously rendered this slot, if it is still
    /// considered reusable. This performs a two-step lookup: first an exact
    /// slot match, then compatibility using the policy.
    pub fn take_node_from_reusables(&mut self, slot_id: SlotId) -> Option<NodeId> {
        if let Some(nodes) = self.mapping.get_nodes(&slot_id) {
            if let Some((position, _)) = self
                .reusable_nodes
                .iter()
                .enumerate()
                .find(|(_, candidate)| nodes.contains(candidate))
            {
                let node_id = self.reusable_nodes.remove(position);
                self.reusable_count = self.reusable_nodes.len();
                return Some(node_id);
            }
        }

        let position = self.reusable_nodes.iter().position(|node_id| {
            self.mapping
                .get_slot(node_id)
                .map(|existing_slot| self.policy.are_compatible(existing_slot, slot_id))
                .unwrap_or(false)
        });

        position.map(|index| {
            let node_id = self.reusable_nodes.remove(index);
            self.reusable_count = self.reusable_nodes.len();
            if let Some(previous_slot) = self.mapping.remove_by_node(&node_id) {
                self.mapping.add_node(slot_id, node_id);
                if let Some(nodes) = self.precomposed_nodes.get_mut(&previous_slot) {
                    nodes.retain(|candidate| *candidate != node_id);
                    if nodes.is_empty() {
                        self.precomposed_nodes.remove(&previous_slot);
                    }
                }
            }
            node_id
        })
    }

    /// Moves active slots starting from `start_index` to the reusable bucket.
    /// Returns the list of node ids that transitioned to the reusable pool.
    pub fn dispose_or_reuse_starting_from_index(&mut self, start_index: usize) -> Vec<NodeId> {
        // FUTURE(no_std): return iterator over bounded node buffer.
        if start_index >= self.active_order.len() {
            return Vec::new();
        }

        let retain = self
            .policy
            .get_slots_to_retain(&self.active_order[start_index..]);
        let mut moved = Vec::new();
        let mut retained = Vec::new();
        while self.active_order.len() > start_index {
            let slot = self.active_order.pop().expect("active_order not empty");
            if retain.contains(&slot) {
                retained.push(slot);
                continue;
            }
            self.mapping.deactivate_slot(slot);
            if let Some(nodes) = self.mapping.get_nodes(&slot) {
                for node in nodes {
                    self.reusable_nodes.push(*node);
                    moved.push(*node);
                }
            }
        }
        retained.reverse();
        self.active_order.extend(retained);
        self.reusable_count = self.reusable_nodes.len();
        moved
    }

    /// Returns a snapshot of currently reusable nodes.
    pub fn reusable(&self) -> &[NodeId] {
        &self.reusable_nodes
    }

    /// Returns a snapshot of precomposed nodes.
    pub fn precomposed(&self) -> &HashMap<SlotId, Vec<NodeId>> {
        // FUTURE(no_std): expose arena-backed view without HashMap.
        &self.precomposed_nodes
    }

    /// Removes any precomposed nodes whose slots were not activated during the
    /// current pass and returns their identifiers for disposal.
    pub fn drain_inactive_precomposed(&mut self) -> Vec<NodeId> {
        // FUTURE(no_std): drain into smallvec buffer.
        let active: HashSet<SlotId> = self.active_order.iter().copied().collect();
        let mut disposed = Vec::new();
        let mut empty_slots = Vec::new();
        for (slot, nodes) in self.precomposed_nodes.iter_mut() {
            if !active.contains(slot) {
                disposed.extend(nodes.iter().copied());
                empty_slots.push(*slot);
            }
        }
        for slot in empty_slots {
            self.precomposed_nodes.remove(&slot);
        }
        self.precomposed_count = self
            .precomposed_nodes
            .values()
            .map(|nodes| nodes.len())
            .sum();
        disposed
    }
}

#[cfg(test)]
#[path = "tests/subcompose_tests.rs"]
mod tests;
