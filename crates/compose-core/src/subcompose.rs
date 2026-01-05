//! State tracking for measure-time subcomposition.
//!
//! The [`SubcomposeState`] keeps book of which slots are active, which nodes can
//! be reused, and which precompositions need to be disposed. Reuse follows a
//! two-phase lookup: first [`SlotId`]s that match exactly are preferred. If no
//! exact match exists, the [`SlotReusePolicy`] is consulted to determine whether
//! a node produced for another slot is compatible with the requested slot.

use crate::collections::map::HashMap; // FUTURE(no_std): replace HashMap/HashSet with arena-backed maps.
use crate::collections::map::HashSet;
use std::collections::VecDeque;
use std::fmt;
use std::rc::Rc;

use crate::{NodeId, RecomposeScope, SlotTable, SlotsHost};

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
///
/// Note: This trait does NOT require Send + Sync because the compose runtime
/// is single-threaded (uses Rc/RefCell throughout).
pub trait SlotReusePolicy: 'static {
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

    /// Registers the content type for a slot.
    ///
    /// Policies that support content-type-based reuse (like [`ContentTypeReusePolicy`])
    /// should override this to record the type. The default implementation is a no-op.
    ///
    /// Call this before subcomposing an item to enable content-type-aware slot reuse.
    fn register_content_type(&self, _slot_id: SlotId, _content_type: u64) {
        // Default: no-op for policies that don't care about content types
    }

    /// Removes the content type for a slot (e.g., when transitioning to None).
    ///
    /// Policies that track content types should override this to clean up.
    /// The default implementation is a no-op.
    fn remove_content_type(&self, _slot_id: SlotId) {
        // Default: no-op for policies that don't track content types
    }

    /// Prunes slot data for slots not in the active set.
    ///
    /// Called during [`SubcomposeState::prune_inactive_slots`] to allow policies
    /// to clean up any internal state for slots that are no longer needed.
    /// The default implementation is a no-op.
    fn prune_slots(&self, _keep_slots: &HashSet<SlotId>) {
        // Default: no-op for policies without per-slot state
    }
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

/// Reuse policy that allows cross-slot reuse when content types match.
///
/// This policy enables efficient recycling of layout nodes across different
/// slot IDs when they share the same content type (e.g., list items with
/// similar structure but different data).
///
/// # Example
///
/// ```rust,ignore
/// use compose_core::{ContentTypeReusePolicy, SubcomposeState, SlotId};
///
/// let mut policy = ContentTypeReusePolicy::new();
///
/// // Register content types for slots
/// policy.set_content_type(SlotId::new(0), 1); // Header type
/// policy.set_content_type(SlotId::new(1), 2); // Item type
/// policy.set_content_type(SlotId::new(2), 2); // Item type (same as slot 1)
///
/// // Slot 1 can reuse slot 2's node since they share content type 2
/// assert!(policy.are_compatible(SlotId::new(2), SlotId::new(1)));
/// ```
pub struct ContentTypeReusePolicy {
    /// Maps slot ID to content type.
    slot_types: std::cell::RefCell<HashMap<SlotId, u64>>,
}

impl std::fmt::Debug for ContentTypeReusePolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let types = self.slot_types.borrow();
        f.debug_struct("ContentTypeReusePolicy")
            .field("slot_types", &*types)
            .finish()
    }
}

impl Default for ContentTypeReusePolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl ContentTypeReusePolicy {
    /// Creates a new content-type-aware reuse policy.
    pub fn new() -> Self {
        Self {
            slot_types: std::cell::RefCell::new(HashMap::default()),
        }
    }

    /// Registers the content type for a slot.
    ///
    /// Call this when subcomposing an item with a known content type.
    pub fn set_content_type(&self, slot: SlotId, content_type: u64) {
        self.slot_types.borrow_mut().insert(slot, content_type);
    }

    /// Removes the content type for a slot (e.g., when disposed).
    pub fn remove_content_type(&self, slot: SlotId) {
        self.slot_types.borrow_mut().remove(&slot);
    }

    /// Clears all registered content types.
    pub fn clear(&self) {
        self.slot_types.borrow_mut().clear();
    }

    /// Returns the content type for a slot, if registered.
    pub fn get_content_type(&self, slot: SlotId) -> Option<u64> {
        self.slot_types.borrow().get(&slot).copied()
    }
}

impl SlotReusePolicy for ContentTypeReusePolicy {
    fn get_slots_to_retain(&self, active: &[SlotId]) -> HashSet<SlotId> {
        let _ = active;
        // Don't retain any - let SubcomposeState manage reusable pool
        HashSet::default()
    }

    fn are_compatible(&self, existing: SlotId, requested: SlotId) -> bool {
        // Exact match always wins
        if existing == requested {
            return true;
        }

        // Check content type compatibility
        let types = self.slot_types.borrow();
        match (types.get(&existing), types.get(&requested)) {
            (Some(existing_type), Some(requested_type)) => existing_type == requested_type,
            // If either slot has no type, fall back to exact match only
            _ => false,
        }
    }

    fn register_content_type(&self, slot_id: SlotId, content_type: u64) {
        self.set_content_type(slot_id, content_type);
    }

    fn remove_content_type(&self, slot_id: SlotId) {
        ContentTypeReusePolicy::remove_content_type(self, slot_id);
    }

    fn prune_slots(&self, keep_slots: &HashSet<SlotId>) {
        self.slot_types
            .borrow_mut()
            .retain(|slot, _| keep_slots.contains(slot));
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
                    // Also clean up slot_to_scopes when slot becomes empty
                    self.slot_to_scopes.remove(&slot);
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

    fn retain_slots(&mut self, active: &HashSet<SlotId>) -> Vec<NodeId> {
        let mut removed_nodes = Vec::new();
        self.slot_to_nodes.retain(|slot, nodes| {
            if active.contains(slot) {
                true
            } else {
                removed_nodes.extend(nodes.iter().copied());
                false
            }
        });
        self.slot_to_scopes.retain(|slot, _| active.contains(slot));
        for node in &removed_nodes {
            self.node_to_slot.remove(node);
        }
        removed_nodes
    }
}

/// Tracks the state of nodes produced by subcomposition, enabling reuse between
/// measurement passes.
pub struct SubcomposeState {
    mapping: NodeSlotMapping,
    active_order: Vec<SlotId>, // FUTURE(no_std): replace Vec with bounded ordering buffer.
    /// Per-content-type reusable node pools for O(1) compatible node lookup.
    /// Key is content type, value is a deque of (SlotId, NodeId) pairs.
    /// Nodes without content type go to `reusable_nodes_untyped`.
    reusable_by_type: HashMap<u64, VecDeque<(SlotId, NodeId)>>,
    /// Reusable nodes without a content type (fallback pool).
    reusable_nodes_untyped: VecDeque<(SlotId, NodeId)>,
    /// Maps slot to its content type for efficient lookup during reuse.
    slot_content_types: HashMap<SlotId, u64>,
    precomposed_nodes: HashMap<SlotId, Vec<NodeId>>, // FUTURE(no_std): use arena-backed precomposition lists.
    policy: Box<dyn SlotReusePolicy>,
    pub(crate) current_index: usize,
    pub(crate) reusable_count: usize,
    pub(crate) precomposed_count: usize,
    /// Per-slot SlotsHost for isolated compositions.
    /// Each SlotId gets its own slot table, avoiding cursor-based conflicts
    /// when items are subcomposed in different orders.
    slot_compositions: HashMap<SlotId, Rc<SlotsHost>>,
    /// Maximum number of reusable slots to keep cached per content type.
    max_reusable_per_type: usize,
    /// Maximum number of reusable slots for the untyped pool.
    max_reusable_untyped: usize,
    /// Whether the last slot registered via register_active was reused.
    /// Set during register_active, read via was_last_slot_reused().
    last_slot_reused: Option<bool>,
}

impl fmt::Debug for SubcomposeState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SubcomposeState")
            .field("mapping", &self.mapping)
            .field("active_order", &self.active_order)
            .field("reusable_by_type_count", &self.reusable_by_type.len())
            .field("reusable_untyped_count", &self.reusable_nodes_untyped.len())
            .field("precomposed_nodes", &self.precomposed_nodes)
            .field("current_index", &self.current_index)
            .field("reusable_count", &self.reusable_count)
            .field("precomposed_count", &self.precomposed_count)
            .field("slot_compositions_count", &self.slot_compositions.len())
            .finish()
    }
}

impl Default for SubcomposeState {
    fn default() -> Self {
        Self::new(Box::new(DefaultSlotReusePolicy))
    }
}

/// Default maximum reusable slots to cache per content type.
/// With multiple content types, total reusable = this * number_of_types.
const DEFAULT_MAX_REUSABLE_PER_TYPE: usize = 5;

/// Default maximum reusable slots for the untyped pool.
/// This is higher than per-type since all items without content types share this pool.
/// Matches RecyclerView's default cache size.
const DEFAULT_MAX_REUSABLE_UNTYPED: usize = 10;

impl SubcomposeState {
    /// Creates a new [`SubcomposeState`] using the supplied reuse policy.
    pub fn new(policy: Box<dyn SlotReusePolicy>) -> Self {
        Self {
            mapping: NodeSlotMapping::default(),
            active_order: Vec::new(),
            reusable_by_type: HashMap::default(),
            reusable_nodes_untyped: VecDeque::new(),
            slot_content_types: HashMap::default(),
            precomposed_nodes: HashMap::default(), // FUTURE(no_std): initialize arena-backed precomposition map.
            policy,
            current_index: 0,
            reusable_count: 0,
            precomposed_count: 0,
            slot_compositions: HashMap::default(),
            max_reusable_per_type: DEFAULT_MAX_REUSABLE_PER_TYPE,
            max_reusable_untyped: DEFAULT_MAX_REUSABLE_UNTYPED,
            last_slot_reused: None,
        }
    }

    /// Sets the policy used for future reuse decisions.
    pub fn set_policy(&mut self, policy: Box<dyn SlotReusePolicy>) {
        self.policy = policy;
    }

    /// Registers a content type for a slot.
    ///
    /// Stores the content type locally for efficient pool-based reuse lookup,
    /// and also delegates to the policy for compatibility checking.
    ///
    /// Call this before subcomposing an item to enable content-type-aware slot reuse.
    pub fn register_content_type(&mut self, slot_id: SlotId, content_type: u64) {
        self.slot_content_types.insert(slot_id, content_type);
        self.policy.register_content_type(slot_id, content_type);
    }

    /// Updates the content type for a slot, handling Some→None transitions.
    ///
    /// If `content_type` is `Some(type)`, registers the type for the slot.
    /// If `content_type` is `None`, removes any previously registered type.
    /// This ensures stale types don't drive incorrect reuse.
    pub fn update_content_type(&mut self, slot_id: SlotId, content_type: Option<u64>) {
        match content_type {
            Some(ct) => self.register_content_type(slot_id, ct),
            None => {
                self.slot_content_types.remove(&slot_id);
                self.policy.remove_content_type(slot_id);
            }
        }
    }

    /// Returns the content type for a slot, if registered.
    pub fn get_content_type(&self, slot_id: SlotId) -> Option<u64> {
        self.slot_content_types.get(&slot_id).copied()
    }

    /// Starts a new subcompose pass.
    ///
    /// Call this before subcomposing the current frame so the state can
    /// track which slots are active and dispose the inactive ones later.
    pub fn begin_pass(&mut self) {
        self.current_index = 0;
    }

    /// Finishes a subcompose pass, disposing slots that were not used.
    pub fn finish_pass(&mut self) -> Vec<NodeId> {
        let disposed = self.dispose_or_reuse_starting_from_index(self.current_index);
        self.prune_inactive_slots();
        disposed
    }

    /// Returns the SlotsHost for the given slot ID, creating a new one if it doesn't exist.
    /// Each slot gets its own isolated slot table, avoiding cursor-based conflicts when
    /// items are subcomposed in different orders.
    pub fn get_or_create_slots(&mut self, slot_id: SlotId) -> Rc<SlotsHost> {
        Rc::clone(self.slot_compositions.entry(slot_id).or_insert_with(|| {
            Rc::new(SlotsHost::new(crate::slot_backend::SlotBackend::Baseline(
                SlotTable::new(),
            )))
        }))
    }

    /// Records that the nodes in `node_ids` are currently rendering the provided
    /// `slot_id`.
    pub fn register_active(
        &mut self,
        slot_id: SlotId,
        node_ids: &[NodeId],
        scopes: &[RecomposeScope],
    ) {
        // Track whether this slot was reused (had existing nodes before this call)
        let was_reused =
            self.mapping.get_nodes(&slot_id).is_some() || self.active_order.contains(&slot_id);
        self.last_slot_reused = Some(was_reused);

        if let Some(position) = self.active_order.iter().position(|slot| *slot == slot_id) {
            if position < self.current_index {
                for scope in scopes {
                    scope.reactivate();
                }
                self.mapping.set_nodes(slot_id, node_ids);
                self.mapping.set_scopes(slot_id, scopes);
                if let Some(nodes) = self.precomposed_nodes.get_mut(&slot_id) {
                    let before_len = nodes.len();
                    nodes.retain(|node| !node_ids.contains(node));
                    let removed = before_len - nodes.len();
                    self.precomposed_count = self.precomposed_count.saturating_sub(removed);
                    if nodes.is_empty() {
                        self.precomposed_nodes.remove(&slot_id);
                    }
                }
                return;
            }
            self.active_order.remove(position);
        }
        for scope in scopes {
            scope.reactivate();
        }
        self.mapping.set_nodes(slot_id, node_ids);
        self.mapping.set_scopes(slot_id, scopes);
        if let Some(nodes) = self.precomposed_nodes.get_mut(&slot_id) {
            let before_len = nodes.len();
            nodes.retain(|node| !node_ids.contains(node));
            let removed = before_len - nodes.len();
            self.precomposed_count = self.precomposed_count.saturating_sub(removed);
            if nodes.is_empty() {
                self.precomposed_nodes.remove(&slot_id);
            }
        }
        let insert_at = self.current_index.min(self.active_order.len());
        self.active_order.insert(insert_at, slot_id);
        self.current_index += 1;
    }

    /// Stores a precomposed node for the provided slot. Precomposed nodes stay
    /// detached from the tree until they are activated by `register_active`.
    pub fn register_precomposed(&mut self, slot_id: SlotId, node_id: NodeId) {
        self.precomposed_nodes
            .entry(slot_id)
            .or_default()
            .push(node_id);
        self.precomposed_count += 1;
    }

    /// Returns the node that previously rendered this slot, if it is still
    /// considered reusable. Uses O(1) content-type based lookup when available.
    ///
    /// Lookup order:
    /// 1. Exact slot match in the appropriate pool
    /// 2. Any compatible node from the same content-type pool (O(1) pop)
    /// 3. Fallback to untyped pool with policy compatibility check
    pub fn take_node_from_reusables(&mut self, slot_id: SlotId) -> Option<NodeId> {
        // First, try to find an exact slot match in mapping
        if let Some(nodes) = self.mapping.get_nodes(&slot_id) {
            let first_node = nodes.first().copied();
            if let Some(node_id) = first_node {
                // Check if this node is in the reusable pools
                if self.remove_from_reusable_pools(node_id) {
                    self.update_reusable_count();
                    return Some(node_id);
                }
            }
        }

        // Get the content type for the requested slot
        let content_type = self.slot_content_types.get(&slot_id).copied();

        // Try to get a node from the same content-type pool (O(1))
        if let Some(ct) = content_type {
            if let Some(pool) = self.reusable_by_type.get_mut(&ct) {
                if let Some((old_slot, node_id)) = pool.pop_front() {
                    self.migrate_node_to_slot(node_id, old_slot, slot_id);
                    self.update_reusable_count();
                    return Some(node_id);
                }
            }
        }

        // Fallback: check untyped pool with policy compatibility
        let position = self
            .reusable_nodes_untyped
            .iter()
            .position(|(existing_slot, _)| self.policy.are_compatible(*existing_slot, slot_id));

        if let Some(index) = position {
            if let Some((old_slot, node_id)) = self.reusable_nodes_untyped.remove(index) {
                self.migrate_node_to_slot(node_id, old_slot, slot_id);
                self.update_reusable_count();
                return Some(node_id);
            }
        }

        None
    }

    /// Removes a node from whatever reusable pool it's in.
    fn remove_from_reusable_pools(&mut self, node_id: NodeId) -> bool {
        // Check typed pools
        for pool in self.reusable_by_type.values_mut() {
            if let Some(pos) = pool.iter().position(|(_, n)| *n == node_id) {
                pool.remove(pos);
                return true;
            }
        }
        // Check untyped pool
        if let Some(pos) = self
            .reusable_nodes_untyped
            .iter()
            .position(|(_, n)| *n == node_id)
        {
            self.reusable_nodes_untyped.remove(pos);
            return true;
        }
        false
    }

    /// Migrates a node from one slot to another, updating mappings.
    fn migrate_node_to_slot(&mut self, node_id: NodeId, old_slot: SlotId, new_slot: SlotId) {
        self.mapping.remove_by_node(&node_id);
        self.mapping.add_node(new_slot, node_id);
        if let Some(nodes) = self.precomposed_nodes.get_mut(&old_slot) {
            nodes.retain(|candidate| *candidate != node_id);
            if nodes.is_empty() {
                self.precomposed_nodes.remove(&old_slot);
            }
        }
    }

    /// Updates the reusable_count from all pools.
    fn update_reusable_count(&mut self) {
        self.reusable_count = self
            .reusable_by_type
            .values()
            .map(|p| p.len())
            .sum::<usize>()
            + self.reusable_nodes_untyped.len();
    }

    /// Moves active slots starting from `start_index` to the reusable bucket.
    /// Returns the list of node ids that were DISPOSED (not just moved to reusable).
    /// Nodes that exceed max_reusable_per_type are disposed instead of cached.
    pub fn dispose_or_reuse_starting_from_index(&mut self, start_index: usize) -> Vec<NodeId> {
        // FUTURE(no_std): return iterator over bounded node buffer.
        if start_index >= self.active_order.len() {
            return Vec::new();
        }

        let retain = self
            .policy
            .get_slots_to_retain(&self.active_order[start_index..]);
        let mut retained = Vec::new();
        while self.active_order.len() > start_index {
            let slot = self.active_order.pop().expect("active_order not empty");
            if retain.contains(&slot) {
                retained.push(slot);
                continue;
            }
            self.mapping.deactivate_slot(slot);

            // Add nodes to appropriate content-type pool
            let content_type = self.slot_content_types.get(&slot).copied();
            if let Some(nodes) = self.mapping.get_nodes(&slot) {
                for node in nodes {
                    if let Some(ct) = content_type {
                        self.reusable_by_type
                            .entry(ct)
                            .or_default()
                            .push_back((slot, *node));
                    } else {
                        self.reusable_nodes_untyped.push_back((slot, *node));
                    }
                }
            }
        }
        retained.reverse();
        self.active_order.extend(retained);

        // Enforce max_reusable_per_type limit per pool - dispose oldest nodes first (FIFO)
        let mut disposed = Vec::new();

        // Enforce limit on typed pools
        for pool in self.reusable_by_type.values_mut() {
            while pool.len() > self.max_reusable_per_type {
                if let Some((_, node_id)) = pool.pop_front() {
                    self.mapping.remove_by_node(&node_id);
                    disposed.push(node_id);
                }
            }
        }

        // Enforce limit on untyped pool (uses separate, larger limit)
        while self.reusable_nodes_untyped.len() > self.max_reusable_untyped {
            if let Some((_, node_id)) = self.reusable_nodes_untyped.pop_front() {
                self.mapping.remove_by_node(&node_id);
                disposed.push(node_id);
            }
        }

        self.update_reusable_count();
        disposed
    }

    fn prune_inactive_slots(&mut self) {
        let active: HashSet<SlotId> = self.active_order.iter().copied().collect();

        // Collect reusable slots from all pools
        let mut reusable_slots: HashSet<SlotId> = HashSet::default();
        for pool in self.reusable_by_type.values() {
            for (slot, _) in pool {
                reusable_slots.insert(*slot);
            }
        }
        for (slot, _) in &self.reusable_nodes_untyped {
            reusable_slots.insert(*slot);
        }

        let mut keep_slots = active.clone();
        keep_slots.extend(reusable_slots);
        self.mapping.retain_slots(&keep_slots);

        // Keep slot compositions for both active AND reusable slots.
        // This ensures items can be reused without full recomposition when scrolling back.
        // Only truly removed slots (not active, not reusable) should have their compositions cleared.
        self.slot_compositions
            .retain(|slot, _| keep_slots.contains(slot));

        // Clean up content type mappings for inactive slots
        self.slot_content_types
            .retain(|slot, _| keep_slots.contains(slot));

        // Notify policy to prune its internal slot data
        self.policy.prune_slots(&keep_slots);

        // Track count before pruning to compute removed count
        let before_count = self.precomposed_count;
        let mut removed_from_precomposed = 0usize;
        self.precomposed_nodes.retain(|slot, nodes| {
            if active.contains(slot) {
                true
            } else {
                removed_from_precomposed += nodes.len();
                false
            }
        });

        // Prune typed pools - retain only nodes that still have valid slots in mapping
        for pool in self.reusable_by_type.values_mut() {
            pool.retain(|(_, node)| self.mapping.get_slot(node).is_some());
        }
        // Remove empty typed pools
        self.reusable_by_type.retain(|_, pool| !pool.is_empty());

        // Prune untyped pool
        self.reusable_nodes_untyped
            .retain(|(_, node)| self.mapping.get_slot(node).is_some());

        self.update_reusable_count();
        self.precomposed_count = before_count.saturating_sub(removed_from_precomposed);
    }

    /// Returns a snapshot of currently reusable nodes.
    pub fn reusable(&self) -> Vec<NodeId> {
        let mut nodes: Vec<NodeId> = self
            .reusable_by_type
            .values()
            .flat_map(|pool| pool.iter().map(|(_, n)| *n))
            .collect();
        nodes.extend(self.reusable_nodes_untyped.iter().map(|(_, n)| *n));
        nodes
    }

    /// Returns the number of slots currently active (in use during this pass).
    ///
    /// This reflects the slots that were activated via `register_active()` during
    /// the current measurement pass.
    pub fn active_slots_count(&self) -> usize {
        self.active_order.len()
    }

    /// Returns the number of reusable slots in the pool.
    ///
    /// These are slots that were previously active but are now available for reuse
    /// by compatible content types.
    pub fn reusable_slots_count(&self) -> usize {
        self.reusable_count
    }

    /// Returns whether the last slot registered via [`register_active`] was reused.
    ///
    /// Returns `Some(true)` if the slot already existed (was reused from pool or
    /// was recomposed), `Some(false)` if it was newly created, or `None` if no
    /// slot has been registered yet this pass.
    ///
    /// This is useful for tracking composition statistics in lazy layouts.
    pub fn was_last_slot_reused(&self) -> Option<bool> {
        self.last_slot_reused
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
        // disposed.len() is the exact count of nodes removed
        self.precomposed_count = self.precomposed_count.saturating_sub(disposed.len());
        disposed
    }
}

#[cfg(test)]
#[path = "tests/subcompose_tests.rs"]
mod tests;
