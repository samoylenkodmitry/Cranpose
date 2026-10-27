// The policies that decide which subcomposed slots a subcompose state keeps
// for reuse and which slot's nodes another slot may take.

use super::SlotId;
use crate::collections::map::HashSet;

/// Policy that decides which previously composed slots should be retained for
/// potential reuse during the next subcompose pass.
///
/// Note: This trait does NOT require Send + Sync because the compose runtime
/// is single-threaded (uses Rc/RefCell throughout).
pub trait SlotReusePolicy: 'static {
    /// Returns the subset of slots that should be retained for reuse after the
    /// current measurement pass. Slots that are not part of the returned set
    /// will be disposed. The default retains none.
    fn get_slots_to_retain(&self, active: &[SlotId]) -> HashSet<SlotId> {
        let _ = active;
        HashSet::default()
    }

    /// Determines whether a node that previously rendered the slot `existing`
    /// can be reused when the caller requests `requested`. Each slot comes
    /// with the content type [`SubcomposeState`](super::SubcomposeState) holds for it.
    ///
    /// Implementations should document what constitutes compatibility (for
    /// example, identical slot identifiers, matching layout classes, or node
    /// types). Returning `true` allows [`SubcomposeState`](super::SubcomposeState) to move the node
    /// across slots instead of disposing it.
    fn are_compatible(&self, existing: TypedSlot, requested: TypedSlot) -> bool;
}

/// A slot with the content type registered for it, if any: what a
/// [`SlotReusePolicy`] decides reuse by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TypedSlot {
    /// The slot.
    pub id: SlotId,
    /// The content type registered for the slot.
    pub content_type: Option<u64>,
}

/// Default reuse policy that mirrors Jetpack Compose behaviour: dispose
/// everything from the tail so that the next measurement can decide which
/// content to keep alive. Compatibility defaults to exact slot matches.
#[derive(Debug, Default)]
pub struct DefaultSlotReusePolicy;

impl SlotReusePolicy for DefaultSlotReusePolicy {
    fn are_compatible(&self, existing: TypedSlot, requested: TypedSlot) -> bool {
        existing.id == requested.id
    }
}

/// Reuse policy that allows cross-slot reuse when content types match.
///
/// This policy enables efficient recycling of layout nodes across different
/// slot IDs when they share the same content type (e.g., list items with
/// similar structure but different data). Two slots without a content type
/// are compatible too.
///
/// # Example
///
/// ```
/// use cranpose_core::{ContentTypeReusePolicy, SlotId, SlotReusePolicy, TypedSlot};
///
/// let policy = ContentTypeReusePolicy;
/// let item = |id, content_type| TypedSlot {
///     id: SlotId::new(id),
///     content_type,
/// };
///
/// // Slot 1 can reuse slot 2's node since they share content type 2
/// assert!(policy.are_compatible(item(2, Some(2)), item(1, Some(2))));
/// // A header cannot take an item's node
/// assert!(!policy.are_compatible(item(2, Some(2)), item(0, Some(1))));
/// ```
#[derive(Debug, Default)]
pub struct ContentTypeReusePolicy;

impl SlotReusePolicy for ContentTypeReusePolicy {
    fn are_compatible(&self, existing: TypedSlot, requested: TypedSlot) -> bool {
        existing.id == requested.id || existing.content_type == requested.content_type
    }
}
