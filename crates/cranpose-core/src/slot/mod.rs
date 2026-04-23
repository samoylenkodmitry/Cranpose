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
mod table;
mod types;
mod validate;
mod writer;

#[cfg(test)]
mod tests;

pub(crate) use anchors::{AnchorRegistry, AnchorState};
pub use debug::{
    SlotDebugAnchor, SlotDebugGroup, SlotDebugScope, SlotDebugSnapshot, SlotTableDebugStats,
};
pub(crate) use detach::dispose_detached_subtree_now;
use groups::GroupRecord;
pub(crate) use lifecycle::{DeferredDrop, SlotLifecycleCoordinator};
use payload_locations::PayloadLocationRegistry;
pub use table::SlotTable;
#[cfg_attr(not(test), allow(unused_imports))]
pub(crate) use table::{SlotWriteSession, SlotWriteSessionState};
pub(crate) use types::NodeLifecycle;
pub(crate) use types::{DetachedSubtree, FinishGroupResult, SlotPassMode};
use types::{NodeRecord, PayloadRecord};
pub(crate) use validate::SlotInvariantError;
