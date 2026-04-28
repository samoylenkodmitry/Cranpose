//! Crate-private slot storage identifiers and group operation records used by
//! [`crate::SlotTable`].

use crate::{slot::checked_usize_to_u32, AnchorId, Key, NodeId, ScopeId};

/// Stable structural identity for a group among siblings.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct GroupKey {
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
pub(crate) struct GroupKeySeed {
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

/// Transient handle to a group in the active slot storage.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) struct ActiveGroupId {
    pub(crate) index: u32,
    pub(crate) generation: u32,
}

impl ActiveGroupId {
    pub(crate) fn new(index: usize, generation: u32) -> Self {
        Self {
            index: checked_usize_to_u32(index, "active group id index"),
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

/// Stable semantic identity for a payload record.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) struct PayloadAnchor {
    id: u32,
    generation: u32,
}

impl PayloadAnchor {
    pub(crate) fn new(id: usize, generation: u32) -> Self {
        Self {
            id: checked_usize_to_u32(id, "payload anchor id"),
            generation,
        }
    }

    pub(crate) fn id(self) -> usize {
        self.id as usize
    }

    pub(crate) fn generation(self) -> u32 {
        self.generation
    }

    pub(crate) fn with_generation(self, generation: u32) -> Self {
        Self {
            id: self.id,
            generation,
        }
    }
}

/// Opaque handle to a value slot in the slot storage.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) struct ValueSlotId {
    pub(crate) anchor: PayloadAnchor,
}

impl ValueSlotId {
    pub(crate) fn new(anchor: PayloadAnchor) -> Self {
        Self { anchor }
    }

    pub(crate) fn anchor(self) -> PayloadAnchor {
        self.anchor
    }
}

/// Semantic result of starting a group at the current writer cursor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GroupStartKind {
    Inserted,
    Reused,
    Moved,
    Restored,
}

/// Semantic input required to begin a group at the current writer cursor.
pub(crate) struct BeginGroupInput<R> {
    pub(crate) key: GroupKey,
    pub(crate) restored: Option<R>,
}

impl<R> BeginGroupInput<R> {
    pub(crate) fn new(key: GroupKey, restored: Option<R>) -> Self {
        Self { key, restored }
    }
}

/// Result of starting a group.
pub(crate) struct GroupStart<G> {
    pub(crate) group: G,
    pub(crate) anchor: AnchorId,
    pub(crate) scope_id: Option<ScopeId>,
    pub(crate) kind: GroupStartKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NodeSlotUpdate {
    Reused {
        id: NodeId,
        generation: u32,
    },
    Inserted {
        id: NodeId,
        generation: u32,
    },
    Replaced {
        old_id: NodeId,
        old_generation: u32,
        new_id: NodeId,
        new_generation: u32,
    },
}
