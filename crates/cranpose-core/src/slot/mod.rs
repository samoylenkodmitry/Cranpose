mod anchors;
mod branch_folds;
mod checked;
mod debug;
mod dense_id_map;
mod detach;
mod generational_registry;
mod groups;
mod growth;
mod introspection;
mod lifecycle;
mod movable;
mod nodes;
mod payload;
mod payload_anchors;
mod payload_store;
mod ranges;
mod scopes;
mod segments;
mod table;
mod types;
mod validate;
mod writer;

#[cfg(test)]
mod tests;

pub(crate) use anchors::AnchorRegistry;
#[cfg(any(test, debug_assertions))]
pub(crate) use anchors::AnchorState;
pub(crate) use branch_folds::BranchFolds;
pub(crate) use checked::checked_usize_to_u32;
pub(in crate::slot) use checked::{
    CheckedU32Delta, checked_u32_delta, checked_usize_to_i64, shift_u32_values,
};
pub(crate) use debug::SlotLifecycleDebugStats;
pub use debug::{
    SlotDebugAnchor, SlotDebugEntry, SlotDebugEntryKind, SlotDebugGroup, SlotDebugScope,
    SlotDebugSnapshot, SlotRetentionDebugStats, SlotTableDebugStats, SlotTableLocalDebugStats,
    SlotTableMutationDebugStats,
};
pub(crate) use detach::dispose_detached_node_now;
use groups::GroupRecord;
pub(crate) use lifecycle::{DeferredDrop, SlotLifecycleCoordinator};
pub(crate) use movable::{MOVABLE_PLACEHOLDER_STATIC_KEY, MOVABLE_STATIC_KEY, MovableIndex};
pub(in crate::slot) use payload::PayloadInit;
#[cfg(any(test, debug_assertions))]
pub(crate) use payload_anchors::PayloadAnchorLifecycle;
pub(crate) use payload_anchors::PayloadAnchorRegistry;
pub(in crate::slot) use ranges::{
    DirectChildRange, GroupNodeRange, GroupPayloadRange, GroupRange, NodeRange, PayloadRange,
    SubtreeRange,
};
pub use table::SlotTable;
pub(crate) use table::SlotWriteSession;
#[cfg(test)]
pub(crate) use table::ValueSlotError;
pub(crate) use types::{
    ActiveGroupId, ActiveSubtreeRoot, BRANCH_PATH_ROOT, ChildCursor, DetachedSubtree,
    FinishGroupResult, GroupKey, GroupKeySeed, GroupStart, GroupStartKind, NodeLifecycle,
    NodeSlotUpdate, PayloadAnchor, PayloadKind, RootNodeIds, SlotPassMode, ValueSlotId,
};
use types::{NodeRecord, PayloadRecord, PayloadType};
#[cfg(any(test, debug_assertions))]
pub(crate) use validate::SlotInvariantError;
#[cfg(test)]
pub(crate) use validate::SlotTreeContext;
pub(crate) use writer::SlotWriteSessionState;
