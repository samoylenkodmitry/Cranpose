//! Slot storage helper types used by [`crate::SlotTable`].
//!
//! The slot table is the single storage implementation in this crate. These
//! helpers keep the call sites readable without reintroducing an abstraction
//! layer around that single concrete type.

use crate::AnchorId;
use crate::RecomposeScope;

/// Opaque handle to a group in the slot storage.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct GroupId(pub(crate) usize);

/// Result of starting a group.
pub struct StartGroup<G> {
    pub group: G,
    pub anchor: AnchorId,
    /// True if this group was restored from a gap (unstable children).
    pub restored_from_gap: bool,
}

/// Result of starting a scoped group.
pub struct StartScopedGroup<G> {
    pub group: G,
    pub anchor: AnchorId,
    pub scope: RecomposeScope,
    /// True if this group was restored from a gap (unstable children).
    pub restored_from_gap: bool,
}
