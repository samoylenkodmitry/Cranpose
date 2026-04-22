//! Semantic slot storage types used by [`crate::SlotTable`].
//!
//! The runtime still has one concrete slot table implementation, but the
//! composer talks in terms of group/value/node operations rather than physical
//! table layout details.

use crate::slot::{DetachedSubtree, FinishGroupResult, SlotInvariantError};
use crate::{AnchorId, Key, NodeId, Owned, ScopeId, SlotDebugSnapshot};

/// Stable structural identity for a group among siblings.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct GroupKey {
    pub(crate) static_key: Key,
    pub(crate) explicit_key: Option<Key>,
    pub(crate) ordinal: u32,
}

impl GroupKey {
    pub(crate) fn new(static_key: Key, explicit_key: Option<Key>, ordinal: u32) -> Self {
        Self {
            static_key,
            explicit_key,
            ordinal,
        }
    }
}

/// Seed used to reserve a full [`GroupKey`] in the active writer frame.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct GroupKeySeed {
    pub(crate) static_key: Key,
    pub(crate) explicit_key: Option<Key>,
}

impl GroupKeySeed {
    pub(crate) fn unkeyed(static_key: Key) -> Self {
        Self {
            static_key,
            explicit_key: None,
        }
    }

    pub(crate) fn keyed(static_key: Key, explicit_key: Key) -> Self {
        Self {
            static_key,
            explicit_key: Some(explicit_key),
        }
    }
}

/// Opaque handle to a group in the slot storage.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct GroupId {
    pub(crate) index: u32,
    pub(crate) generation: u32,
}

impl GroupId {
    pub(crate) fn new(index: usize, generation: u32) -> Self {
        Self {
            index: index as u32,
            generation,
        }
    }

    pub(crate) fn index(self) -> usize {
        self.index as usize
    }

    pub(crate) fn generation(self) -> u32 {
        self.generation
    }
}

/// Opaque handle to a value slot in the slot storage.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct ValueSlotId {
    pub(crate) anchor: usize,
    pub(crate) generation: u32,
}

impl ValueSlotId {
    pub(crate) fn new(anchor: usize, generation: u32) -> Self {
        Self { anchor, generation }
    }

    pub(crate) fn anchor(self) -> usize {
        self.anchor
    }

    pub(crate) fn generation(self) -> u32 {
        self.generation
    }
}

/// Opaque anchor handle for a group in active slot storage.
pub type GroupAnchor = AnchorId;

/// Semantic result of starting a group at the current writer cursor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GroupStartKind {
    Inserted,
    Reused,
    Moved,
    Restored,
}

/// Semantic input required to begin a group at the current writer cursor.
pub struct BeginGroupInput<R> {
    pub key: GroupKey,
    pub restored: Option<R>,
}

impl<R> BeginGroupInput<R> {
    pub fn new(key: GroupKey, restored: Option<R>) -> Self {
        Self { key, restored }
    }
}

/// Result of starting a group.
pub struct GroupStart<G> {
    pub group: G,
    pub anchor: GroupAnchor,
    pub scope_id: Option<ScopeId>,
    pub kind: GroupStartKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NodeRecordResult {
    pub reused: bool,
    pub id: NodeId,
}

#[allow(dead_code)]
pub(crate) trait SlotStorage {
    type Group: Copy + Eq;
    type ValueSlot: Copy + Eq;

    fn begin_group(&mut self, input: BeginGroupInput<DetachedSubtree>) -> GroupStart<Self::Group>;
    fn finish_group_body(&mut self) -> FinishGroupResult;
    fn end_group(&mut self);
    fn skip_group(&mut self);

    fn set_group_scope(&mut self, group: Self::Group, scope: ScopeId);
    fn begin_recompose_at_scope(&mut self, scope: ScopeId) -> Option<Self::Group>;
    fn end_recompose(&mut self);

    fn value_slot<T: 'static>(&mut self, init: impl FnOnce() -> T) -> Self::ValueSlot;
    fn read_value<T: 'static>(&self, slot: Self::ValueSlot) -> &T;
    fn read_value_mut<T: 'static>(&mut self, slot: Self::ValueSlot) -> &mut T;
    fn write_value<T: 'static>(&mut self, slot: Self::ValueSlot, value: T);
    fn remember<T: 'static>(&mut self, init: impl FnOnce() -> T) -> Owned<T>;

    fn record_node(&mut self, id: NodeId, generation: u32) -> NodeRecordResult;
    fn nodes_in_current_group(&self) -> Vec<NodeId>;

    fn validate(&self) -> Result<(), SlotInvariantError>;
    fn debug_snapshot(&self) -> SlotDebugSnapshot;
}
