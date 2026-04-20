//! Slot storage helper types used by [`crate::SlotTable`].
//!
//! The slot table is the single storage implementation in this crate. These
//! helpers keep the call sites readable without reintroducing an abstraction
//! layer around that single concrete type.

use crate::AnchorId;
use crate::{Key, ScopeId};

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
    pub(crate) group: GroupId,
    pub(crate) offset: u32,
    pub(crate) generation: u32,
}

impl ValueSlotId {
    pub(crate) fn new(group: GroupId, offset: usize, generation: u32) -> Self {
        Self {
            group,
            offset: offset as u32,
            generation,
        }
    }

    pub(crate) fn group(self) -> GroupId {
        self.group
    }

    pub(crate) fn offset(self) -> usize {
        self.offset as usize
    }

    pub(crate) fn generation(self) -> u32 {
        self.generation
    }
}

/// Semantic result of starting a group at the current writer cursor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GroupStartKind {
    Inserted,
    Reused,
    Moved,
    Restored,
}

/// Result of starting a scoped group.
pub struct StartScopedGroup<G> {
    pub group: G,
    pub anchor: AnchorId,
    pub scope_id: Option<ScopeId>,
    pub kind: GroupStartKind,
}
