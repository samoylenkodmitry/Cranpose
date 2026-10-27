//! State tracking for measure-time subcomposition.
//!
//! The [`SubcomposeState`] keeps book of which slots are active, which nodes can
//! be reused, and which precompositions need to be disposed. Reuse follows a
//! two-phase lookup: first [`SlotId`]s that match exactly are preferred. If no
//! exact match exists, the [`SlotReusePolicy`] is consulted to determine whether
//! a node produced for another slot is compatible with the requested slot.

use std::{any::Any, fmt, rc::Rc};

use smallvec::SmallVec;

use crate::{
    CallbackHolder, NodeId, RecomposeScope, SlotTable, SlotsHost, collections::map::HashMap,
};

mod reuse_pool;
mod slot_reuse_policy;

use reuse_pool::ReusePool;
pub use slot_reuse_policy::{
    ContentTypeReusePolicy, DefaultSlotReusePolicy, SlotReusePolicy, TypedSlot,
};

/// One group of a slot table, for debugging: the group index, its call site key, its scope id and
/// the length of its subtree.
pub type DebugSlotGroup = (usize, crate::Key, Option<usize>, usize);

/// Identifier for a subcomposed slot.
///
/// This mirrors the `slotId` concept in Jetpack Compose where callers provide
/// stable identifiers for reusable children during measure-time composition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SlotId(pub u64);

impl SlotId {
    /// Creates a slot id from `raw`.
    #[inline]
    pub fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// Returns the identifier as a number.
    #[inline]
    pub fn raw(self) -> u64 {
        self.0
    }
}

#[doc(hidden)]
pub struct ExactSlotActivation<'a> {
    pub nodes: &'a [NodeId],
    pub was_recycled: bool,
}

/// Evicted roots and the slot state that stays alive until their nodes are disposed.
/// Pass this batch to [`crate::Composer::dispose_subcomposed_nodes`].
#[derive(Default)]
pub struct SubcomposeDisposal {
    pub(crate) nodes: Vec<NodeId>,
    pub(crate) slot_hosts: Vec<Rc<SlotsHost>>,
}

impl SubcomposeDisposal {
    /// Returns the roots awaiting disposal.
    ///
    /// ```
    /// use cranpose_core::SubcomposeState;
    /// let mut state = SubcomposeState::default();
    /// let disposed = state.finish_pass();
    /// assert!(disposed.nodes().is_empty());
    /// ```
    pub fn nodes(&self) -> &[NodeId] {
        &self.nodes
    }

    fn append(&mut self, mut other: Self) {
        self.nodes.append(&mut other.nodes);
        self.slot_hosts.append(&mut other.slot_hosts);
    }
}

/// What the state keeps for one slot, found with one lookup: activating a
/// kept slot reads all of it.
#[derive(Default)]
struct SlotRecord {
    /// The root nodes the slot's content composed, once it composed.
    nodes: Option<Vec<NodeId>>,
    /// The recompose scopes of the slot's content.
    scopes: Option<Vec<RecomposeScope>>,
    /// The slot is active: the last pass kept it, or the current one did.
    live: bool,
    /// The pass that activated the slot; zero once it left the active set.
    active_pass: u64,
    /// How many of the slot's nodes wait in the reuse pools.
    reusable_nodes: usize,
    /// The slot waits in the reuse pool and can be reactivated as it was.
    exact_reactivation: bool,
    content_type: Option<u64>,
    composition: Option<Rc<SlotsHost>>,
    callback: Option<CallbackHolder>,
    capture_key: Option<RetainedCaptureKey>,
    /// The content generation and owner epoch the slot last composed under.
    composed_generation: Option<(u64, u64)>,
}

impl SlotRecord {
    /// Whether a scope of the slot's content must run before the slot shows
    /// what it composed: an invalid one, or one a deactivation left inactive.
    fn has_stale_scopes(&self) -> bool {
        self.scopes
            .iter()
            .flatten()
            .any(|scope| scope.is_invalid() || !scope.is_active())
    }

    fn has_invalid_scopes(&self) -> bool {
        self.scopes.iter().flatten().any(RecomposeScope::is_invalid)
    }

    /// Whether the record holds nothing a later pass reads.
    fn is_empty(&self) -> bool {
        self.nodes.is_none()
            && self.scopes.is_none()
            && !self.live
            && self.reusable_nodes == 0
            && !self.exact_reactivation
            && self.content_type.is_none()
            && self.composition.is_none()
            && self.callback.is_none()
            && self.capture_key.is_none()
            && self.composed_generation.is_none()
    }
}

/// Tracks the state of nodes produced by subcomposition, enabling reuse between
/// measurement passes.
pub struct SubcomposeState {
    slots: HashMap<SlotId, SlotRecord>,
    node_to_slot: HashMap<NodeId, SlotId>,
    active_order: Vec<SlotId>,
    /// The running pass, counted from one: a slot activated in it has it as
    /// its `active_pass`.
    pass: u64,
    pool: ReusePool,
    precomposed_nodes: HashMap<SlotId, Vec<NodeId>>,
    policy: Box<dyn SlotReusePolicy>,
    pub(crate) current_index: usize,
    pub(crate) reusable_count: usize,
    pub(crate) precomposed_count: usize,
    last_slot_reused: Option<bool>,
    content_generation: std::cell::Cell<u64>,
    /// The content generation from which slots compose under changed
    /// static composition locals; zero while none changed.
    locals_generation: std::cell::Cell<u64>,
}

struct RetainedCaptureKey {
    value: Box<dyn Any>,
    eq: fn(&dyn Any, &dyn Any) -> bool,
}

fn retained_capture_key_eq<K: PartialEq + 'static>(stored: &dyn Any, candidate: &dyn Any) -> bool {
    match (stored.downcast_ref::<K>(), candidate.downcast_ref::<K>()) {
        (Some(stored), Some(candidate)) => stored == candidate,
        _ => false,
    }
}

impl fmt::Debug for SubcomposeState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SubcomposeState")
            .field("slots_count", &self.slots.len())
            .field("node_to_slot", &self.node_to_slot)
            .field("active_order", &self.active_order)
            .field("reusable_by_type_count", &self.pool.type_count())
            .field("reusable_untyped_count", &self.pool.untyped_len())
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
            slots: HashMap::default(),
            node_to_slot: HashMap::default(),
            active_order: Vec::new(),
            pass: 1,
            pool: ReusePool::new(),
            precomposed_nodes: HashMap::default(),
            policy,
            current_index: 0,
            reusable_count: 0,
            precomposed_count: 0,
            last_slot_reused: None,
            content_generation: std::cell::Cell::new(0),
            locals_generation: std::cell::Cell::new(0),
        }
    }

    /// Sets the policy used for future reuse decisions.
    pub fn set_policy(&mut self, policy: Box<dyn SlotReusePolicy>) {
        self.policy = policy;
    }

    /// Sets how many composed nodes the reusable pool keeps: `per_type` for each content type and
    /// `untyped` for nodes without one. Nodes over a limit are disposed.
    pub fn set_reusable_pool_limits(&mut self, per_type: usize, untyped: usize) {
        self.pool.set_limits(per_type, untyped);
    }

    fn record(&mut self, slot_id: SlotId) -> &mut SlotRecord {
        self.slots.entry(slot_id).or_default()
    }

    fn slot_nodes(&self, slot_id: SlotId) -> Option<&[NodeId]> {
        self.slots.get(&slot_id)?.nodes.as_deref()
    }

    fn typed_slot(&self, slot_id: SlotId) -> TypedSlot {
        TypedSlot {
            id: slot_id,
            content_type: self.get_content_type(slot_id),
        }
    }

    fn slot_active_in_pass(&self, slot_id: SlotId) -> bool {
        self.slots
            .get(&slot_id)
            .is_some_and(|record| record.active_pass == self.pass)
    }

    fn clear_exact_reactivation(&mut self, slot_id: SlotId) {
        if let Some(record) = self.slots.get_mut(&slot_id) {
            record.exact_reactivation = false;
        }
    }

    /// Registers a content type for a slot.
    ///
    /// Call this before subcomposing an item to enable content-type-aware slot reuse.
    pub fn register_content_type(&mut self, slot_id: SlotId, content_type: u64) {
        self.record(slot_id).content_type = Some(content_type);
    }

    /// Updates the content type for a slot, handling optional content types.
    ///
    /// If `content_type` is `Some(type)`, registers the type for the slot.
    /// If `content_type` is `None`, removes any previously registered type.
    /// This ensures stale types don't drive incorrect reuse.
    pub fn update_content_type(&mut self, slot_id: SlotId, content_type: Option<u64>) {
        match content_type {
            Some(content_type) => self.register_content_type(slot_id, content_type),
            None => {
                if let Some(record) = self.slots.get_mut(&slot_id) {
                    record.content_type = None;
                }
            }
        }
    }

    /// Returns the content type for a slot, if registered.
    pub fn get_content_type(&self, slot_id: SlotId) -> Option<u64> {
        self.slots.get(&slot_id)?.content_type
    }

    /// Returns the active slot that owns a registered root node.
    ///
    /// Descendants must be resolved to their slot root by the caller. Reusable
    /// and removed slots are excluded.
    pub fn active_slot_for_node(&self, node_id: NodeId) -> Option<SlotId> {
        let slot = *self.node_to_slot.get(&node_id)?;
        self.slot_is_active(slot).then_some(slot)
    }

    /// Starts a new subcompose pass.
    ///
    /// Call this before subcomposing the current frame so the state can
    /// track which slots are active and dispose the inactive ones later.
    pub fn begin_pass(&mut self) {
        self.current_index = 0;
        self.pass += 1;
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
    pub fn recycle_active_slot(&mut self, slot_id: SlotId) -> SubcomposeDisposal {
        self.recycle_active_slot_internal(slot_id, false)
    }

    /// Like `recycle_active_slot`, for a slot that was composed ahead of need. The slot can be
    /// reactivated as it was when its id is asked for again, unless its effects were forgotten.
    pub fn recycle_prefetched_active_slot(&mut self, slot_id: SlotId) -> SubcomposeDisposal {
        self.recycle_active_slot_internal(slot_id, true)
    }

    /// Recycles every active slot that `predicate` accepts and that the current pass has not used.
    /// Returns the nodes and slot hosts that were disposed.
    pub fn recycle_active_slots_where(
        &mut self,
        mut predicate: impl FnMut(SlotId) -> bool,
    ) -> SubcomposeDisposal {
        let slots: Vec<_> = self
            .active_order
            .iter()
            .copied()
            .filter(|slot| !self.slot_active_in_pass(*slot))
            .filter(|slot| predicate(*slot))
            .collect();
        let mut disposed = SubcomposeDisposal::default();
        for slot in slots {
            disposed.append(self.recycle_active_slot(slot));
        }
        disposed
    }

    fn recycle_active_slot_internal(
        &mut self,
        slot_id: SlotId,
        allow_exact_reactivation: bool,
    ) -> SubcomposeDisposal {
        let Some(position) = self
            .active_order
            .iter()
            .position(|candidate| *candidate == slot_id)
        else {
            return SubcomposeDisposal::default();
        };
        self.active_order.remove(position);
        if position < self.current_index {
            self.current_index = self.current_index.saturating_sub(1);
        }
        self.move_slot_to_reusable(slot_id, allow_exact_reactivation);
        self.enforce_reusable_pool_limits()
    }

    /// Finishes a subcompose pass, disposing slots that were not used.
    pub fn finish_pass(&mut self) -> SubcomposeDisposal {
        self.dispose_or_reuse_starting_from_index(self.current_index)
    }

    /// Returns the SlotsHost for the given slot ID, creating a new one if it doesn't exist.
    /// Each slot gets its own isolated slot table, avoiding cursor-based conflicts when
    /// items are subcomposed in different orders.
    pub fn get_or_create_slots(&mut self, slot_id: SlotId) -> Rc<SlotsHost> {
        Rc::clone(
            self.record(slot_id)
                .composition
                .get_or_insert_with(|| Rc::new(SlotsHost::new(SlotTable::new()))),
        )
    }

    /// Returns the latest callback holder for the given slot, creating one if needed.
    pub fn callback_holder(&mut self, slot_id: SlotId) -> CallbackHolder {
        self.record(slot_id)
            .callback
            .get_or_insert_with(CallbackHolder::default)
            .clone()
    }

    /// The root nodes of `slot_id` when it is the active slot at `position`
    /// in [`Self::active_slots`] and the next pass would activate it there
    /// as it is. A measure pass that keeps the slots of the last one checks
    /// them in that order without starting a pass.
    pub fn kept_slot_nodes_at(&self, position: usize, slot_id: SlotId) -> Option<&[NodeId]> {
        if self.active_order.get(position) != Some(&slot_id) {
            return None;
        }
        let record = self.slots.get(&slot_id)?;
        if record.exact_reactivation || record.reusable_nodes > 0 || record.has_stale_scopes() {
            return None;
        }
        record.nodes.as_deref()
    }

    /// The active slots, in the order the last pass activated them.
    pub fn active_slots(&self) -> &[SlotId] {
        &self.active_order
    }

    #[doc(hidden)]
    pub fn activate_current_active_slot(&mut self, slot_id: SlotId) -> Option<&[NodeId]> {
        if !self.activate_current_active_slot_at_cursor(slot_id) {
            return None;
        }
        self.slot_nodes(slot_id)
    }

    fn activate_current_active_slot_at_cursor(&mut self, slot_id: SlotId) -> bool {
        if self.active_order.get(self.current_index) != Some(&slot_id) {
            return false;
        }
        let pass = self.pass;
        let Some(record) = self.slots.get_mut(&slot_id) else {
            return false;
        };
        if record.active_pass == pass
            || record.exact_reactivation
            || record.reusable_nodes > 0
            || record.nodes.is_none()
            || record.has_stale_scopes()
        {
            return false;
        }
        record.live = true;
        record.active_pass = pass;
        self.last_slot_reused = Some(true);
        self.current_index += 1;
        true
    }

    /// Whether precomposed nodes are waiting to be consumed for this slot.
    /// A pending precomposition means the retained mapping is about to be
    /// superseded, so clean-slot reuse must reject and compose.
    pub fn has_pending_precompositions(&self, slot_id: SlotId) -> bool {
        self.precomposed_nodes.contains_key(&slot_id)
    }

    /// Whether the slot's stored capture key equals `key`. A missing key never
    /// matches: a slot must compose at least once under the key discipline
    /// before it may skip.
    pub fn retained_capture_key_matches<K: PartialEq + 'static>(
        &self,
        slot_id: SlotId,
        key: &K,
    ) -> bool {
        self.slots
            .get(&slot_id)
            .and_then(|record| record.capture_key.as_ref())
            .is_some_and(|retained| (retained.eq)(retained.value.as_ref(), key))
    }

    /// Records the capture key the slot is being composed under.
    pub fn store_retained_capture_key<K: PartialEq + 'static>(&mut self, slot_id: SlotId, key: K) {
        self.record(slot_id).capture_key = Some(RetainedCaptureKey {
            value: Box::new(key),
            eq: retained_capture_key_eq::<K>,
        });
    }

    #[doc(hidden)]
    pub fn activate_exact_slot(&mut self, slot_id: SlotId) -> Option<ExactSlotActivation<'_>> {
        let was_recycled = if self.activate_current_active_slot_at_cursor(slot_id) {
            false
        } else {
            self.reactivate_exact_slot(slot_id)?
        };
        Some(ExactSlotActivation {
            nodes: self.slot_nodes(slot_id)?,
            was_recycled,
        })
    }

    /// Activates `slot_id` as it was away from the cursor: active further on,
    /// or waiting in the reuse pool to be reactivated as it was. Returns
    /// whether it came from the pool.
    fn reactivate_exact_slot(&mut self, slot_id: SlotId) -> Option<bool> {
        let active_position = self
            .active_order
            .iter()
            .position(|candidate| *candidate == slot_id);
        let record = self.slots.get(&slot_id)?;
        if active_position.is_none() && !record.exact_reactivation || record.has_invalid_scopes() {
            return None;
        }
        let nodes: SmallVec<[NodeId; 4]> = record.nodes.as_deref()?.iter().copied().collect();
        if active_position.is_none() {
            for scope in record.scopes.iter().flatten() {
                scope.reactivate();
            }
            for &node in &nodes {
                let _ = self.remove_from_reusable_pools(node);
            }
        }
        self.mark_slot_active(slot_id, true);
        Self::consume_precomposed_nodes(
            &mut self.precomposed_nodes,
            &mut self.precomposed_count,
            slot_id,
            &nodes,
        );
        Some(active_position.is_none())
    }

    /// Records that the nodes in `node_ids` are currently rendering the provided
    /// `slot_id`.
    pub fn register_active(
        &mut self,
        slot_id: SlotId,
        node_ids: &[NodeId],
        scopes: &[RecomposeScope],
    ) {
        for scope in scopes {
            scope.reactivate();
        }
        let record = self.slots.entry(slot_id).or_default();
        let was_reused = record.nodes.is_some();
        let nodes = record.nodes.get_or_insert_default();
        nodes.clear();
        nodes.extend_from_slice(node_ids);
        let held_scopes = record.scopes.get_or_insert_default();
        held_scopes.clear();
        held_scopes.extend_from_slice(scopes);
        for &node in node_ids {
            self.node_to_slot.insert(node, slot_id);
        }
        Self::consume_precomposed_nodes(
            &mut self.precomposed_nodes,
            &mut self.precomposed_count,
            slot_id,
            node_ids,
        );
        self.mark_slot_active(slot_id, was_reused);
    }

    fn mark_slot_active(&mut self, slot_id: SlotId, was_reused: bool) {
        self.last_slot_reused = Some(was_reused);
        let pass = self.pass;
        let record = self.record(slot_id);
        record.live = true;
        record.active_pass = pass;
        record.exact_reactivation = false;

        if let Some(position) = self.active_order.iter().position(|slot| *slot == slot_id) {
            if position < self.current_index {
                return;
            }
            self.active_order.remove(position);
        }
        let insert_at = self.current_index.min(self.active_order.len());
        self.active_order.insert(insert_at, slot_id);
        self.current_index += 1;
    }

    fn consume_precomposed_nodes(
        precomposed_nodes: &mut HashMap<SlotId, Vec<NodeId>>,
        precomposed_count: &mut usize,
        slot_id: SlotId,
        node_ids: &[NodeId],
    ) {
        if let Some(nodes) = precomposed_nodes.get_mut(&slot_id) {
            let before_len = nodes.len();
            nodes.retain(|node| !node_ids.contains(node));
            let removed = before_len - nodes.len();
            *precomposed_count = precomposed_count.saturating_sub(removed);
            if nodes.is_empty() {
                precomposed_nodes.remove(&slot_id);
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

    fn decrement_reusable_slot(&mut self, slot_id: SlotId) {
        if let Some(record) = self.slots.get_mut(&slot_id) {
            record.reusable_nodes = record.reusable_nodes.saturating_sub(1);
        }
        self.reusable_count = self.reusable_count.saturating_sub(1);
    }

    /// Takes `node` out of the slot it rendered, and returns that slot. A
    /// slot left with no nodes forgets its scopes too.
    fn remove_node_from_slot(&mut self, node: NodeId) -> Option<SlotId> {
        let slot = self.node_to_slot.remove(&node)?;
        if let Some(record) = self.slots.get_mut(&slot)
            && let Some(nodes) = record.nodes.as_mut()
        {
            if let Some(index) = nodes.iter().position(|candidate| *candidate == node) {
                nodes.remove(index);
            }
            if nodes.is_empty() {
                record.nodes = None;
                record.scopes = None;
            }
        }
        Some(slot)
    }

    /// Whether `slot_id` is composed and active: the last pass kept it, or
    /// the current one did.
    pub fn slot_is_active(&self, slot_id: SlotId) -> bool {
        self.slots.get(&slot_id).is_some_and(|record| record.live)
    }

    /// Whether `slot_id` still has a composition here: active, or kept in the
    /// reuse pool where it can be reactivated. A slot that is neither has
    /// been disposed, and nothing measured for it can be used again.
    pub fn slot_is_retained(&self, slot_id: SlotId) -> bool {
        self.slots
            .get(&slot_id)
            .is_some_and(|record| record.live || record.reusable_nodes > 0)
    }

    fn prune_slot_if_unused(&mut self, slot_id: SlotId) -> Option<Rc<SlotsHost>> {
        if self.slot_is_retained(slot_id) {
            return None;
        }

        debug_assert!(
            self.slot_nodes(slot_id).is_none(),
            "inactive slot {slot_id:?} still has mapped nodes",
        );

        let host = self
            .slots
            .remove(&slot_id)
            .and_then(|record| record.composition);
        if let Some(nodes) = self.precomposed_nodes.remove(&slot_id) {
            self.precomposed_count = self.precomposed_count.saturating_sub(nodes.len());
        }
        host
    }

    /// Returns the node that previously rendered this slot, if it is still
    /// considered reusable, and whether it was REBOUND — taken from another
    /// slot, so its retained subtree is about to show different content. An
    /// exact-slot reactivation hands the same item its own subtree back and
    /// is not a rebinding.
    ///
    /// Lookup order:
    /// 1. Exact slot match in the appropriate pool (not a rebinding)
    /// 2. Any policy-compatible node from the same content-type pool
    /// 3. Fallback to untyped pool with policy compatibility check
    pub fn take_node_from_reusables(&mut self, slot_id: SlotId) -> Option<(NodeId, bool)> {
        if let Some(node_id) = self
            .slot_nodes(slot_id)
            .and_then(|nodes| nodes.first().copied())
        {
            let _ = self.remove_from_reusable_pools(node_id);
            self.clear_exact_reactivation(slot_id);
            return Some((node_id, false));
        }

        let requested = self.typed_slot(slot_id);
        let (pool, slots, policy) = (&mut self.pool, &self.slots, &self.policy);
        let accepts = |existing| reusable_for(slots, policy.as_ref(), existing, requested);
        // A typed slot falls back to the untyped queue.
        let (old_slot, node_id) = requested
            .content_type
            .and_then(|content_type| pool.take(Some(content_type), accepts))
            .or_else(|| pool.take(None, accepts))?;
        self.decrement_reusable_slot(old_slot);
        self.move_node_to_slot(node_id, old_slot, slot_id);
        Some((node_id, true))
    }

    fn remove_from_reusable_pools(&mut self, node_id: NodeId) -> Option<SlotId> {
        let slot = self.pool.remove_node(node_id)?;
        self.decrement_reusable_slot(slot);
        Some(slot)
    }

    fn move_node_to_slot(&mut self, node_id: NodeId, old_slot: SlotId, new_slot: SlotId) {
        if old_slot == new_slot {
            return;
        }

        self.remove_node_from_slot(node_id);
        let (composition, callback) = match self.slots.get_mut(&old_slot) {
            Some(old) => {
                old.exact_reactivation = false;
                old.content_type = None;
                old.capture_key = None;
                old.composed_generation = None;
                (old.composition.take(), old.callback.take())
            }
            None => (None, None),
        };
        if self.slots.get(&old_slot).is_some_and(SlotRecord::is_empty) {
            self.slots.remove(&old_slot);
        }
        self.node_to_slot.insert(node_id, new_slot);
        let new = self.record(new_slot);
        new.nodes.get_or_insert_default().push(node_id);
        new.capture_key = None;
        new.composed_generation = None;
        if composition.is_some() {
            new.composition = composition;
        }
        if callback.is_some() {
            new.callback = callback;
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
    /// Nodes past the pool's limits are disposed instead of cached.
    pub fn dispose_or_reuse_starting_from_index(
        &mut self,
        start_index: usize,
    ) -> SubcomposeDisposal {
        if start_index >= self.active_order.len() {
            return SubcomposeDisposal::default();
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
        let record = self.slots.entry(slot).or_default();
        record.live = false;
        record.active_pass = 0;
        for scope in record.scopes.iter().flatten() {
            scope.deactivate();
        }
        let forgot_effects = record
            .composition
            .as_ref()
            .is_some_and(|host| host.forget_effects());
        record.exact_reactivation = allow_exact_reactivation && !forgot_effects;

        let content_type = record.content_type;
        let nodes = record.nodes.as_deref().unwrap_or_default();
        record.reusable_nodes += nodes.len();
        self.reusable_count += nodes.len();
        self.pool.push(content_type, slot, nodes.iter().copied());
    }

    fn enforce_reusable_pool_limits(&mut self) -> SubcomposeDisposal {
        let mut disposed = SubcomposeDisposal::default();
        for (slot, node_id) in self.pool.take_excess() {
            self.dispose_reusable_node(slot, node_id, &mut disposed);
        }
        disposed
    }

    /// Disposes `node_id`, which waited in a reuse pool for `slot`.
    fn dispose_reusable_node(
        &mut self,
        slot: SlotId,
        node_id: NodeId,
        disposed: &mut SubcomposeDisposal,
    ) {
        self.decrement_reusable_slot(slot);
        self.remove_node_from_slot(node_id);
        self.clear_exact_reactivation(slot);
        disposed.slot_hosts.extend(self.prune_slot_if_unused(slot));
        disposed.nodes.push(node_id);
    }

    /// Returns a snapshot of currently reusable nodes.
    pub fn reusable(&self) -> Vec<NodeId> {
        self.pool.nodes().collect()
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
    /// Invalidating scopes alone is not a reliable "recompose on next
    /// measure" signal: the recomposer drains invalid scopes before measure
    /// runs, re-executing the RETAINED slot callbacks — parent-captured
    /// values baked into a replaced closure never flow in, yet the scopes
    /// come back valid. The generation bump is the unlaunderable half: only
    /// a measure-time compose of the slot brings it current, so clean-slot
    /// reuse stays blocked until the new content actually landed.
    pub fn invalidate_scopes(&self) {
        for scope in self
            .slots
            .values()
            .flat_map(|record| record.scopes.iter().flatten())
        {
            scope.invalidate();
        }
        self.content_generation
            .set(self.content_generation.get() + 1);
    }

    /// Records that a static composition local the slots compose under
    /// changed: every slot composed before must compose again, and run
    /// every body it holds, before it may be reused.
    pub fn invalidate_locals(&self) {
        self.bump_content_generation();
        self.locals_generation.set(self.content_generation.get());
    }

    /// Whether the slot composed before a static composition local it reads
    /// from changed; see [`Self::invalidate_locals`]. A composition another
    /// slot left counts as fresh: its scopes come back inactive, and a group
    /// entered with an inactive scope composes again anyway.
    pub fn slot_locals_stale(&self, slot_id: SlotId) -> bool {
        let changed_at = self.locals_generation.get();
        self.slots
            .get(&slot_id)
            .and_then(|record| record.composed_generation)
            .is_some_and(|(generation, _)| generation < changed_at)
    }

    /// Advances the content generation without invalidating scopes: every
    /// slot must re-compose once before clean-slot reuse may skip it again.
    /// Called when the owning node leaves its parent — a retained subtree
    /// that comes back from the reuse pool restarts its effects only if its
    /// slots actually recompose, and scope flags carry no durable trace of
    /// the detach (deactivation walks stop at nested slot-host boundaries).
    pub fn bump_content_generation(&self) {
        self.content_generation
            .set(self.content_generation.get() + 1);
    }

    /// Whether the slot last composed under the current content generation
    /// and owner-chain deactivation epoch. False after any
    /// [`Self::invalidate_scopes`], and false after any composition up the
    /// owner chain was deactivated, until a measure-time compose of this
    /// slot runs.
    pub fn slot_content_generation_current(&self, slot_id: SlotId, owner_epoch: u64) -> bool {
        self.slots
            .get(&slot_id)
            .and_then(|record| record.composed_generation)
            == Some((self.content_generation.get(), owner_epoch))
    }

    /// Records that the slot just composed under the current generation and
    /// the given owner-chain deactivation epoch.
    pub fn mark_slot_composed_current(&mut self, slot_id: SlotId, owner_epoch: u64) {
        let generation = self.content_generation.get();
        self.record(slot_id).composed_generation = Some((generation, owner_epoch));
    }

    /// Returns whether the last slot registered via [`Self::register_active`] was reused.
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
        self.slots
            .iter()
            .filter_map(|(slot, record)| {
                let scopes = record.scopes.as_ref()?;
                Some((slot.raw(), scopes.iter().map(RecomposeScope::id).collect()))
            })
            .collect()
    }

    #[doc(hidden)]
    pub fn debug_slot_table_for_slot(&self, slot_id: SlotId) -> Option<Vec<crate::SlotDebugEntry>> {
        let slots = self.slots.get(&slot_id)?.composition.as_ref()?;
        Some(slots.borrow().debug_dump_slot_entries())
    }

    #[doc(hidden)]
    pub fn debug_slot_table_groups_for_slot(&self, slot_id: SlotId) -> Option<Vec<DebugSlotGroup>> {
        let slots = self.slots.get(&slot_id)?.composition.as_ref()?;
        Some(slots.borrow().debug_dump_groups())
    }

    /// Returns a snapshot of precomposed nodes.
    pub fn precomposed(&self) -> &HashMap<SlotId, Vec<NodeId>> {
        &self.precomposed_nodes
    }

    /// Removes any precomposed nodes whose slots were not activated during the
    /// current pass and returns their identifiers for disposal.
    pub fn drain_inactive_precomposed(&mut self) -> SubcomposeDisposal {
        let mut disposed = SubcomposeDisposal::default();
        let mut empty_slots = Vec::new();
        for (slot, nodes) in &self.precomposed_nodes {
            if !self.slot_active_in_pass(*slot) {
                disposed.nodes.extend(nodes.iter().copied());
                empty_slots.push(*slot);
            }
        }
        for slot in empty_slots {
            self.precomposed_nodes.remove(&slot);
            disposed.slot_hosts.extend(self.prune_slot_if_unused(slot));
        }
        self.precomposed_count = self.precomposed_count.saturating_sub(disposed.nodes.len());
        disposed
    }
}

/// Whether the node pooled for `existing` may compose `requested`: the slot
/// waits to be reactivated as it was, and `policy` decides the rest.
fn reusable_for(
    slots: &HashMap<SlotId, SlotRecord>,
    policy: &dyn SlotReusePolicy,
    existing: SlotId,
    requested: TypedSlot,
) -> bool {
    let record = slots.get(&existing);
    !record.is_some_and(|record| record.exact_reactivation)
        && policy.are_compatible(
            TypedSlot {
                id: existing,
                content_type: record.and_then(|record| record.content_type),
            },
            requested,
        )
}

#[cfg(test)]
#[path = "tests/subcompose_tests.rs"]
mod tests;
