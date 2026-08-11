//! State tracking for measure-time subcomposition.
//!
//! The [`SubcomposeState`] keeps book of which slots are active, which nodes can
//! be reused, and which precompositions need to be disposed. Reuse follows a
//! two-phase lookup: first [`SlotId`]s that match exactly are preferred. If no
//! exact match exists, the [`SlotReusePolicy`] is consulted to determine whether
//! a node produced for another slot is compatible with the requested slot.

use crate::collections::map::HashMap;
use crate::collections::map::HashSet;
use smallvec::SmallVec;
use std::collections::VecDeque;
use std::fmt;
use std::rc::Rc;

use crate::{CallbackHolder, NodeId, RecomposeScope, SlotTable, SlotsHost};

pub type DebugSlotGroup = (usize, crate::Key, Option<usize>, usize);

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
    /// types). Returning `true` allows [`SubcomposeState`] to move the node
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
/// use cranpose_core::{ContentTypeReusePolicy, SubcomposeState, SlotId};
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
        if existing == requested {
            return true;
        }

        let types = self.slot_types.borrow();
        match (types.get(&existing), types.get(&requested)) {
            (Some(existing_type), Some(requested_type)) => existing_type == requested_type,
            (None, None) => true,
            _ => false,
        }
    }

    fn register_content_type(&self, slot_id: SlotId, content_type: u64) {
        self.set_content_type(slot_id, content_type);
    }

    fn remove_content_type(&self, slot_id: SlotId) {
        ContentTypeReusePolicy::remove_content_type(self, slot_id);
    }
}

#[doc(hidden)]
pub struct ExactSlotActivation {
    pub nodes: Vec<NodeId>,
    pub scopes: Vec<RecomposeScope>,
    pub reactivate_scopes: bool,
}

#[derive(Default, Clone)]

struct NodeSlotMapping {
    slot_to_nodes: HashMap<SlotId, Vec<NodeId>>,
    node_to_slot: HashMap<NodeId, SlotId>,
    slot_to_scopes: HashMap<SlotId, Vec<RecomposeScope>>,
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

    fn get_scopes(&self, slot: &SlotId) -> Option<&[RecomposeScope]> {
        self.slot_to_scopes
            .get(slot)
            .map(|scopes| scopes.as_slice())
    }

    fn slot_has_invalid_scopes(&self, slot: SlotId) -> bool {
        self.slot_to_scopes
            .get(&slot)
            .is_some_and(|scopes| scopes.iter().any(RecomposeScope::is_invalid))
    }

    fn deactivate_slot(&self, slot: SlotId) {
        if let Some(scopes) = self.slot_to_scopes.get(&slot) {
            for scope in scopes {
                scope.deactivate();
            }
        }
    }

    fn invalidate_scopes(&self) {
        for scopes in self.slot_to_scopes.values() {
            for scope in scopes {
                scope.invalidate();
            }
        }
    }
}

/// Tracks the state of nodes produced by subcomposition, enabling reuse between
/// measurement passes.
pub struct SubcomposeState {
    mapping: NodeSlotMapping,
    active_order: Vec<SlotId>,
    live_slots: HashSet<SlotId>,
    current_pass_active_slots: HashSet<SlotId>,
    /// Per-content-type reusable node pools used to narrow compatible node lookup.
    /// Key is content type, value is a deque of (SlotId, NodeId) pairs.
    /// Nodes without content type go to `reusable_nodes_untyped`.
    reusable_by_type: HashMap<u64, VecDeque<(SlotId, NodeId)>>,
    /// Reusable nodes without a content type (fallback pool).
    reusable_nodes_untyped: VecDeque<(SlotId, NodeId)>,
    reusable_node_counts: HashMap<SlotId, usize>,
    exact_reactivation_slots: HashSet<SlotId>,
    /// Maps slot to its content type for efficient lookup during reuse.
    slot_content_types: HashMap<SlotId, u64>,
    precomposed_nodes: HashMap<SlotId, Vec<NodeId>>,
    policy: Box<dyn SlotReusePolicy>,
    pub(crate) current_index: usize,
    pub(crate) reusable_count: usize,
    pub(crate) precomposed_count: usize,
    /// Per-slot SlotsHost for isolated compositions.
    /// Each SlotId gets its own slot table, avoiding cursor-based conflicts
    /// when items are subcomposed in different orders.
    slot_compositions: HashMap<SlotId, Rc<SlotsHost>>,
    /// Latest slot root callbacks keyed by slot id.
    slot_callbacks: HashMap<SlotId, CallbackHolder>,
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
            live_slots: HashSet::default(),
            current_pass_active_slots: HashSet::default(),
            reusable_by_type: HashMap::default(),
            reusable_nodes_untyped: VecDeque::new(),
            reusable_node_counts: HashMap::default(),
            exact_reactivation_slots: HashSet::default(),
            slot_content_types: HashMap::default(),
            precomposed_nodes: HashMap::default(),
            policy,
            current_index: 0,
            reusable_count: 0,
            precomposed_count: 0,
            slot_compositions: HashMap::default(),
            slot_callbacks: HashMap::default(),
            max_reusable_per_type: DEFAULT_MAX_REUSABLE_PER_TYPE,
            max_reusable_untyped: DEFAULT_MAX_REUSABLE_UNTYPED,
            last_slot_reused: None,
        }
    }

    /// Sets the policy used for future reuse decisions.
    pub fn set_policy(&mut self, policy: Box<dyn SlotReusePolicy>) {
        self.policy = policy;
    }

    pub fn set_reusable_pool_limits(&mut self, per_type: usize, untyped: usize) {
        self.max_reusable_per_type = per_type;
        self.max_reusable_untyped = untyped;
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

    /// Updates the content type for a slot, handling optional content types.
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
        self.current_pass_active_slots.clear();
    }

    /// Returns the current active slot cursor for the in-progress pass.
    pub fn active_slot_cursor(&self) -> usize {
        self.current_index
    }

    /// Restores the active slot cursor for work that should not become part of
    /// the rendered active set, such as lazy-list prefetch measurement.
    pub fn restore_active_slot_cursor(&mut self, cursor: usize) {
        self.current_index = cursor.min(self.active_order.len());
    }

    /// Moves an active slot out of the rendered set and into the reusable pool.
    pub fn recycle_active_slot(&mut self, slot_id: SlotId) -> Vec<NodeId> {
        self.recycle_active_slot_internal(slot_id, false)
    }

    pub fn recycle_prefetched_active_slot(&mut self, slot_id: SlotId) -> Vec<NodeId> {
        self.recycle_active_slot_internal(slot_id, true)
    }

    pub fn recycle_active_slots_where(
        &mut self,
        mut predicate: impl FnMut(SlotId) -> bool,
    ) -> Vec<NodeId> {
        let slots: Vec<_> = self
            .active_order
            .iter()
            .copied()
            .filter(|slot| !self.current_pass_active_slots.contains(slot))
            .filter(|slot| predicate(*slot))
            .collect();
        let mut disposed = Vec::new();
        for slot in slots {
            disposed.extend(self.recycle_active_slot(slot));
        }
        disposed
    }

    fn recycle_active_slot_internal(
        &mut self,
        slot_id: SlotId,
        allow_exact_reactivation: bool,
    ) -> Vec<NodeId> {
        let Some(position) = self
            .active_order
            .iter()
            .position(|candidate| *candidate == slot_id)
        else {
            return Vec::new();
        };
        self.active_order.remove(position);
        if position < self.current_index {
            self.current_index = self.current_index.saturating_sub(1);
        }
        self.move_slot_to_reusable(slot_id, allow_exact_reactivation);
        self.enforce_reusable_pool_limits()
    }

    /// Finishes a subcompose pass, disposing slots that were not used.
    pub fn finish_pass(&mut self) -> Vec<NodeId> {
        self.dispose_or_reuse_starting_from_index(self.current_index)
    }

    /// Returns the SlotsHost for the given slot ID, creating a new one if it doesn't exist.
    /// Each slot gets its own isolated slot table, avoiding cursor-based conflicts when
    /// items are subcomposed in different orders.
    pub fn get_or_create_slots(&mut self, slot_id: SlotId) -> Rc<SlotsHost> {
        Rc::clone(
            self.slot_compositions
                .entry(slot_id)
                .or_insert_with(|| Rc::new(SlotsHost::new(SlotTable::new()))),
        )
    }

    /// Returns the latest callback holder for the given slot, creating one if needed.
    pub fn callback_holder(&mut self, slot_id: SlotId) -> CallbackHolder {
        self.slot_callbacks.entry(slot_id).or_default().clone()
    }

    #[doc(hidden)]
    pub fn activate_current_active_slot(&mut self, slot_id: SlotId) -> Option<Vec<NodeId>> {
        if self.current_pass_active_slots.contains(&slot_id)
            || self.exact_reactivation_slots.contains(&slot_id)
            || self
                .active_order
                .get(self.current_index)
                .copied()
                .is_none_or(|active_slot| active_slot != slot_id)
            || self.mapping.slot_has_invalid_scopes(slot_id)
        {
            return None;
        }

        let nodes = self.mapping.get_nodes(&slot_id)?.to_vec();
        self.last_slot_reused = Some(true);
        self.live_slots.insert(slot_id);
        self.current_pass_active_slots.insert(slot_id);
        self.current_index += 1;
        Some(nodes)
    }

    #[doc(hidden)]
    pub fn take_exact_slot_activation(&mut self, slot_id: SlotId) -> Option<ExactSlotActivation> {
        let active_position = self
            .active_order
            .iter()
            .position(|candidate| *candidate == slot_id);
        let is_exact_reactivation = self.exact_reactivation_slots.contains(&slot_id);
        if active_position.is_none() && !is_exact_reactivation
            || self.mapping.slot_has_invalid_scopes(slot_id)
        {
            return None;
        }

        let nodes = self.mapping.get_nodes(&slot_id)?.to_vec();
        let scopes = self
            .mapping
            .get_scopes(&slot_id)
            .unwrap_or_default()
            .to_vec();
        if active_position.is_none() {
            for node in &nodes {
                let _ = self.remove_from_reusable_pools(*node);
            }
        }
        self.exact_reactivation_slots.remove(&slot_id);
        Some(ExactSlotActivation {
            nodes,
            scopes,
            reactivate_scopes: active_position.is_none(),
        })
    }

    #[doc(hidden)]
    pub fn take_exact_slot_for_activation(
        &mut self,
        slot_id: SlotId,
    ) -> Option<(Vec<NodeId>, Vec<RecomposeScope>)> {
        self.take_exact_slot_activation(slot_id)
            .map(|activation| (activation.nodes, activation.scopes))
    }

    /// Records that the nodes in `node_ids` are currently rendering the provided
    /// `slot_id`.
    pub fn register_active(
        &mut self,
        slot_id: SlotId,
        node_ids: &[NodeId],
        scopes: &[RecomposeScope],
    ) {
        self.register_active_with_scope_reactivation(slot_id, node_ids, scopes, true);
    }

    #[doc(hidden)]
    pub fn register_active_with_scope_reactivation(
        &mut self,
        slot_id: SlotId,
        node_ids: &[NodeId],
        scopes: &[RecomposeScope],
        reactivate_scopes: bool,
    ) {
        let was_reused = self.mapping.get_nodes(&slot_id).is_some();
        self.last_slot_reused = Some(was_reused);
        self.live_slots.insert(slot_id);
        self.current_pass_active_slots.insert(slot_id);
        self.exact_reactivation_slots.remove(&slot_id);

        if let Some(position) = self.active_order.iter().position(|slot| *slot == slot_id) {
            if position < self.current_index {
                if reactivate_scopes {
                    for scope in scopes {
                        scope.reactivate();
                    }
                }
                self.update_active_slot_mapping(slot_id, node_ids, scopes);
                return;
            }
            self.active_order.remove(position);
        }
        if reactivate_scopes {
            for scope in scopes {
                scope.reactivate();
            }
        }
        self.update_active_slot_mapping(slot_id, node_ids, scopes);
        let insert_at = self.current_index.min(self.active_order.len());
        self.active_order.insert(insert_at, slot_id);
        self.current_index += 1;
    }

    fn update_active_slot_mapping(
        &mut self,
        slot_id: SlotId,
        node_ids: &[NodeId],
        scopes: &[RecomposeScope],
    ) {
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

    fn increment_reusable_slot(&mut self, slot_id: SlotId) {
        *self.reusable_node_counts.entry(slot_id).or_insert(0) += 1;
        self.reusable_count += 1;
    }

    fn decrement_reusable_slot(&mut self, slot_id: SlotId) {
        let Some(entry) = self.reusable_node_counts.get_mut(&slot_id) else {
            self.reusable_count = self.reusable_count.saturating_sub(1);
            return;
        };
        *entry = entry.saturating_sub(1);
        if *entry == 0 {
            self.reusable_node_counts.remove(&slot_id);
        }
        self.reusable_count = self.reusable_count.saturating_sub(1);
    }

    fn prune_slot_if_unused(&mut self, slot_id: SlotId) {
        if self.live_slots.contains(&slot_id) || self.reusable_node_counts.contains_key(&slot_id) {
            return;
        }

        debug_assert!(
            self.mapping.get_nodes(&slot_id).is_none(),
            "inactive slot {slot_id:?} still has mapped nodes",
        );

        self.slot_compositions.remove(&slot_id);
        self.slot_callbacks.remove(&slot_id);
        self.slot_content_types.remove(&slot_id);
        self.policy.remove_content_type(slot_id);
        if let Some(nodes) = self.precomposed_nodes.remove(&slot_id) {
            self.precomposed_count = self.precomposed_count.saturating_sub(nodes.len());
        }
    }

    /// Returns the node that previously rendered this slot, if it is still
    /// considered reusable. Content-type buckets narrow the candidate set, then
    /// the reuse policy makes the final compatibility decision.
    ///
    /// Lookup order:
    /// 1. Exact slot match in the appropriate pool
    /// 2. Any policy-compatible node from the same content-type pool
    /// 3. Fallback to untyped pool with policy compatibility check
    pub fn take_node_from_reusables(&mut self, slot_id: SlotId) -> Option<NodeId> {
        if let Some(nodes) = self.mapping.get_nodes(&slot_id) {
            let first_node = nodes.first().copied();
            if let Some(node_id) = first_node {
                let _ = self.remove_from_reusable_pools(node_id);
                self.exact_reactivation_slots.remove(&slot_id);
                return Some(node_id);
            }
        }

        let content_type = self.slot_content_types.get(&slot_id).copied();

        if let Some(ct) = content_type {
            if let Some((old_slot, node_id)) = self.take_compatible_typed_reusable(ct, slot_id) {
                self.decrement_reusable_slot(old_slot);
                self.move_node_to_slot(node_id, old_slot, slot_id);
                return Some(node_id);
            }
        }

        let exact_reactivation_slots = &self.exact_reactivation_slots;
        let policy = &self.policy;
        let position = self
            .reusable_nodes_untyped
            .iter()
            .position(|(existing_slot, _)| {
                !exact_reactivation_slots.contains(existing_slot)
                    && policy.are_compatible(*existing_slot, slot_id)
            });

        if let Some(index) = position {
            if let Some((old_slot, node_id)) = self.reusable_nodes_untyped.remove(index) {
                self.decrement_reusable_slot(old_slot);
                self.move_node_to_slot(node_id, old_slot, slot_id);
                return Some(node_id);
            }
        }

        None
    }

    fn take_compatible_typed_reusable(
        &mut self,
        content_type: u64,
        requested_slot: SlotId,
    ) -> Option<(SlotId, NodeId)> {
        let reused = {
            let policy = &self.policy;
            let exact_reactivation_slots = &self.exact_reactivation_slots;
            let pool = self.reusable_by_type.get_mut(&content_type)?;
            let index = pool.iter().position(|(existing_slot, _)| {
                !exact_reactivation_slots.contains(existing_slot)
                    && policy.are_compatible(*existing_slot, requested_slot)
            })?;
            pool.remove(index)
        };

        if self
            .reusable_by_type
            .get(&content_type)
            .map(|pool| pool.is_empty())
            .unwrap_or(false)
        {
            self.reusable_by_type.remove(&content_type);
        }

        reused
    }

    /// Removes a node from whatever reusable pool it's in.
    fn remove_from_reusable_pools(&mut self, node_id: NodeId) -> Option<SlotId> {
        // Check typed pools
        let mut typed_match = None;
        for (&content_type, pool) in &self.reusable_by_type {
            if let Some(position) = pool
                .iter()
                .position(|(_, pooled_node)| *pooled_node == node_id)
            {
                typed_match = Some((content_type, position));
                break;
            }
        }
        if let Some((content_type, position)) = typed_match {
            let pool = self.reusable_by_type.get_mut(&content_type)?;
            let (slot, _) = pool.remove(position)?;
            if pool.is_empty() {
                self.reusable_by_type.remove(&content_type);
            }
            self.decrement_reusable_slot(slot);
            return Some(slot);
        }
        // Check untyped pool
        if let Some(position) = self
            .reusable_nodes_untyped
            .iter()
            .position(|(_, n)| *n == node_id)
        {
            if let Some((slot, _)) = self.reusable_nodes_untyped.remove(position) {
                self.decrement_reusable_slot(slot);
                return Some(slot);
            }
        }
        None
    }

    /// Moves a node from one slot to another, updating mappings.
    fn move_node_to_slot(&mut self, node_id: NodeId, old_slot: SlotId, new_slot: SlotId) {
        if old_slot == new_slot {
            return;
        }

        self.mapping.remove_by_node(&node_id);
        self.exact_reactivation_slots.remove(&old_slot);
        self.mapping.add_node(new_slot, node_id);
        self.slot_content_types.remove(&old_slot);
        self.policy.remove_content_type(old_slot);
        if let Some(slots) = self.slot_compositions.remove(&old_slot) {
            self.slot_compositions.insert(new_slot, slots);
        }
        if let Some(callback) = self.slot_callbacks.remove(&old_slot) {
            self.slot_callbacks.insert(new_slot, callback);
        }
        if let Some(nodes) = self.precomposed_nodes.get_mut(&old_slot) {
            let before_len = nodes.len();
            nodes.retain(|candidate| *candidate != node_id);
            let removed = before_len - nodes.len();
            self.precomposed_count = self.precomposed_count.saturating_sub(removed);
            if nodes.is_empty() {
                self.precomposed_nodes.remove(&old_slot);
            }
        }
    }

    /// Moves active slots starting from `start_index` to the reusable bucket.
    /// Returns the list of node ids that were DISPOSED (not just moved to reusable).
    /// Nodes that exceed max_reusable_per_type are disposed instead of cached.
    pub fn dispose_or_reuse_starting_from_index(&mut self, start_index: usize) -> Vec<NodeId> {
        if start_index >= self.active_order.len() {
            return Vec::new();
        }

        let retain = self
            .policy
            .get_slots_to_retain(&self.active_order[start_index..]);
        let mut retained = Vec::new();
        while self.active_order.len() > start_index {
            let Some(slot) = self.active_order.pop() else {
                break;
            };
            if retain.contains(&slot) {
                retained.push(slot);
                continue;
            }
            self.move_slot_to_reusable(slot, false);
        }
        retained.reverse();
        self.active_order.extend(retained);

        self.enforce_reusable_pool_limits()
    }

    fn move_slot_to_reusable(&mut self, slot: SlotId, allow_exact_reactivation: bool) {
        self.live_slots.remove(&slot);
        self.current_pass_active_slots.remove(&slot);
        self.mapping.deactivate_slot(slot);
        if allow_exact_reactivation {
            self.exact_reactivation_slots.insert(slot);
        } else {
            self.exact_reactivation_slots.remove(&slot);
            if let Some(host) = self.slot_compositions.get(&slot) {
                host.forget_effects();
            }
        }

        let content_type = self.slot_content_types.get(&slot).copied();
        let nodes: SmallVec<[NodeId; 4]> = self
            .mapping
            .get_nodes(&slot)
            .into_iter()
            .flatten()
            .copied()
            .collect();
        for node in nodes {
            if let Some(ct) = content_type {
                self.reusable_by_type
                    .entry(ct)
                    .or_default()
                    .push_back((slot, node));
            } else {
                self.reusable_nodes_untyped.push_back((slot, node));
            }
            self.increment_reusable_slot(slot);
        }
    }

    fn enforce_reusable_pool_limits(&mut self) -> Vec<NodeId> {
        let mut disposed = Vec::new();
        let mut typed_disposals = Vec::new();
        for pool in self.reusable_by_type.values_mut() {
            while pool.len() > self.max_reusable_per_type {
                if let Some((slot, node_id)) = pool.pop_front() {
                    typed_disposals.push((slot, node_id));
                }
            }
        }
        for (slot, node_id) in typed_disposals {
            self.decrement_reusable_slot(slot);
            self.mapping.remove_by_node(&node_id);
            self.exact_reactivation_slots.remove(&slot);
            disposed.push(node_id);
        }

        // Enforce limit on untyped pool (uses separate, larger limit)
        while self.reusable_nodes_untyped.len() > self.max_reusable_untyped {
            if let Some((slot, node_id)) = self.reusable_nodes_untyped.pop_front() {
                self.decrement_reusable_slot(slot);
                self.mapping.remove_by_node(&node_id);
                self.exact_reactivation_slots.remove(&slot);
                disposed.push(node_id);
            }
        }

        self.reusable_by_type.retain(|_, pool| !pool.is_empty());
        disposed
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

    /// Invalidates all tracked subcomposition scopes.
    ///
    /// Hosts should call this when parent-captured inputs change without directly invalidating
    /// the child scopes themselves. The next subcompose pass will then re-run active slot
    /// content instead of skipping with stale captures.
    pub fn invalidate_scopes(&self) {
        self.mapping.invalidate_scopes();
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

    #[doc(hidden)]
    pub fn debug_scope_ids_by_slot(&self) -> Vec<(u64, Vec<usize>)> {
        self.mapping
            .slot_to_scopes
            .iter()
            .map(|(slot, scopes)| (slot.raw(), scopes.iter().map(RecomposeScope::id).collect()))
            .collect()
    }

    #[doc(hidden)]
    pub fn debug_slot_table_for_slot(&self, slot_id: SlotId) -> Option<Vec<crate::SlotDebugEntry>> {
        let slots = self.slot_compositions.get(&slot_id)?;
        Some(slots.borrow().debug_dump_slot_entries())
    }

    #[doc(hidden)]
    pub fn debug_slot_table_groups_for_slot(&self, slot_id: SlotId) -> Option<Vec<DebugSlotGroup>> {
        let slots = self.slot_compositions.get(&slot_id)?;
        Some(slots.borrow().debug_dump_groups())
    }

    /// Returns a snapshot of precomposed nodes.
    pub fn precomposed(&self) -> &HashMap<SlotId, Vec<NodeId>> {
        &self.precomposed_nodes
    }

    /// Removes any precomposed nodes whose slots were not activated during the
    /// current pass and returns their identifiers for disposal.
    pub fn drain_inactive_precomposed(&mut self) -> Vec<NodeId> {
        let mut disposed = Vec::new();
        let mut empty_slots = Vec::new();
        for (slot, nodes) in self.precomposed_nodes.iter_mut() {
            if !self.current_pass_active_slots.contains(slot) {
                disposed.extend(nodes.iter().copied());
                empty_slots.push(*slot);
            }
        }
        for slot in empty_slots {
            self.precomposed_nodes.remove(&slot);
            self.prune_slot_if_unused(slot);
        }
        // disposed.len() is the exact count of nodes removed
        self.precomposed_count = self.precomposed_count.saturating_sub(disposed.len());
        disposed
    }
}

#[cfg(test)]
#[path = "tests/subcompose_tests.rs"]
mod tests;
