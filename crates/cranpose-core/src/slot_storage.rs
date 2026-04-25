//! Semantic slot storage identifiers and group operation records used by
//! [`crate::SlotTable`].

use crate::{AnchorId, Key, NodeId, ScopeId};

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
