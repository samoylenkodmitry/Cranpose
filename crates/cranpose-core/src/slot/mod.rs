mod anchors;
mod checked;
mod debug;
mod dense_id_map;
mod detach;
mod groups;
mod introspection;
mod lifecycle;
mod nodes;
mod payload;
mod payload_anchors;
mod ranges;
mod scope_index;
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
pub(crate) use checked::checked_usize_to_u32;
pub(in crate::slot) use checked::{checked_u32_delta, checked_usize_to_i64, CheckedU32Delta};
pub(crate) use debug::SlotLifecycleDebugStats;
pub use debug::{
    SlotDebugAnchor, SlotDebugEntry, SlotDebugEntryKind, SlotDebugGroup, SlotDebugScope,
    SlotDebugSnapshot, SlotRetentionDebugStats, SlotTableDebugStats, SlotTableLocalDebugStats,
    SlotTableMutationDebugStats,
};
pub(crate) use detach::{dispose_detached_node_now, dispose_detached_subtree_now};
use groups::GroupRecord;
pub(crate) use lifecycle::{DeferredDrop, SlotLifecycleCoordinator};
#[cfg(any(test, debug_assertions))]
pub(crate) use payload_anchors::PayloadAnchorLifecycle;
pub(crate) use payload_anchors::PayloadAnchorRegistry;
pub(in crate::slot) use ranges::{
    DirectChildRange, GroupNodeRange, GroupPayloadRange, GroupRange, NodeRange, PayloadRange,
    SubtreeRange,
};
pub(crate) use scope_index::ScopeIndex;
pub use table::SlotTable;
pub(crate) use table::SlotWriteSession;
pub(in crate::slot) use types::collect_root_node_ids_from_records_into;
pub(crate) use types::NodeLifecycle;
pub(crate) use types::{
    ActiveGroupId, ActiveSubtreeRoot, BeginGroupInput, ChildCursor, DetachedChild, DetachedSubtree,
    FinishGroupResult, GroupKey, GroupKeySeed, GroupStart, GroupStartKind, NodeSlotUpdate,
    PayloadAnchor, PayloadKind, SlotPassMode, ValueSlotId,
};
use types::{NodeRecord, PayloadRecord};
#[cfg(any(test, debug_assertions))]
pub(crate) use validate::SlotInvariantError;
#[cfg(test)]
pub(crate) use validate::SlotTreeContext;
pub(crate) use writer::SlotWriteSessionState;
