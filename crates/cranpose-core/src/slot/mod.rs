mod anchors;
mod debug;
mod dense_id_map;
mod detach;
mod groups;
mod lifecycle;
mod nodes;
mod payload;
mod payload_locations;
mod reader;
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
pub use debug::{
    SlotDebugAnchor, SlotDebugEntry, SlotDebugEntryKind, SlotDebugGroup, SlotDebugScope,
    SlotDebugSnapshot, SlotTableDebugStats, SlotTableMutationDebugStats,
};
pub(crate) use detach::{dispose_detached_node_now, dispose_detached_subtree_now};
use groups::GroupRecord;
pub(crate) use lifecycle::{DeferredDrop, SlotLifecycleCoordinator};
use payload_locations::PayloadLocationRegistry;
pub use table::SlotTable;
pub(crate) use table::SlotWriteSession;
pub(in crate::slot) use types::root_node_ids_from_records;
pub(crate) use types::NodeLifecycle;
pub(crate) use types::{DetachedSubtree, FinishGroupResult, PayloadKind, SlotPassMode};
use types::{NodeRecord, PayloadRecord};
#[cfg(any(test, debug_assertions))]
pub(crate) use validate::SlotInvariantError;
pub(crate) use writer::SlotWriteSessionState;
