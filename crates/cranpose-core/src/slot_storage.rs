//! Abstract slot storage trait and related types.
//!
//! This module defines the high-level interface that all slot storage backends
//! must implement. The `Composer` and composition engine interact exclusively
//! through this trait while keeping the composer decoupled from the concrete
//! slot table representation.

use crate::{Key, NodeId, Owned, ScopeId};

/// Opaque handle to a group in the slot storage.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct GroupId(pub(crate) usize);

/// Opaque handle to a value slot in the slot storage.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct ValueSlotId(pub(crate) usize);

impl ValueSlotId {
    pub(crate) fn new(index: usize) -> Self {
        Self(index)
    }

    pub(crate) fn index(&self) -> usize {
        self.0
    }
}

/// Result of starting a group.
pub struct StartGroup<G> {
    pub group: G,
    /// True if this group was restored from a gap (unstable children).
    pub restored_from_gap: bool,
}

/// Abstract slot API that the composer / composition engine talks to.
/// Concrete backends (SlotTable with gap buffer, chunked storage, arena, etc.)
/// implement this and can keep whatever internal layout they want.
pub trait SlotStorage {
    /// Opaque handle to a started group.
    type Group: Copy + Eq;
    /// Opaque handle to a value slot.
    type ValueSlot: Copy + Eq;

    // ── groups ──────────────────────────────────────────────────────────────

    /// Begin a group with the given key.
    ///
    /// Returns a handle to the group and whether it was restored from a gap
    /// (which means the composer needs to force-recompose the scope).
    fn begin_group(&mut self, key: Key) -> StartGroup<Self::Group>;

    /// Associate the runtime recomposition scope with this group.
    fn set_group_scope(&mut self, group: Self::Group, scope: ScopeId);

    /// End the current group.
    fn end_group(&mut self);

    /// Skip over the current group (used by the "skip optimization" in the macro).
    fn skip_current_group(&mut self);

    /// Return node ids that live in the current group (needed so the composer
    /// can reattach them to the parent when skipping).
    fn nodes_in_current_group(&self) -> Vec<NodeId>;

    // ── recomposition ───────────────────────────────────────────────────────

    /// Start recomposing the group that owns `scope`. Returns the group we
    /// started, or `None` if that scope is gone.
    fn begin_recranpose_at_scope(&mut self, scope: ScopeId) -> Option<Self::Group>;

    /// Finish the recomposition started with `begin_recranpose_at_scope`.
    fn end_recompose(&mut self);

    // ── values / remember ───────────────────────────────────────────────────

    /// Allocate or reuse a value slot at the current cursor.
    fn alloc_value_slot<T: 'static>(&mut self, init: impl FnOnce() -> T) -> Self::ValueSlot;

    /// Immutable read of a value slot.
    fn read_value<T: 'static>(&self, slot: Self::ValueSlot) -> &T;

    /// Mutable read of a value slot.
    fn read_value_mut<T: 'static>(&mut self, slot: Self::ValueSlot) -> &mut T;

    /// Overwrite an existing value slot.
    fn write_value<T: 'static>(&mut self, slot: Self::ValueSlot, value: T);

    /// Convenience "remember" built on top of value slots.
    fn remember<T: 'static>(&mut self, init: impl FnOnce() -> T) -> Owned<T>;

    // ── nodes ──────────────────────────────────────────────────────────────

    /// Peek a node at the current cursor (don't advance).
    /// Returns (NodeId, generation) so emit_node can verify the node hasn't been recycled.
    fn peek_node(&self) -> Option<(NodeId, u32)>;

    /// Record a node at the current cursor (and advance).
    fn record_node(&mut self, id: NodeId, gen: u32);

    /// Advance after we've read a node via the applier path.
    fn advance_after_node_read(&mut self);

    /// Step the cursor back by one (used when we probed and need to overwrite).
    fn step_back(&mut self);

    // ── lifecycle / cleanup ─────────────────────────────────────────────────

    /// "Finalize" the current group: mark unreachable tail as gaps.
    /// Returns `true` if we marked gaps (which means children are unstable).
    fn finalize_current_group(&mut self) -> bool;

    /// Reset to the beginning (used by subcompose + top-level render).
    fn reset(&mut self);

    /// Flush any deferred anchor rebuilds.
    fn flush(&mut self);
}
