mod anchors;
mod debug;
mod detach;
mod groups;
mod lifecycle;
mod nodes;
mod payload;
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
use groups::GroupRecord;
pub(crate) use lifecycle::{DeferredDrop, SlotLifecycleCoordinator};
pub use table::SlotTable;
#[cfg_attr(not(test), allow(unused_imports))]
pub(crate) use table::{SlotWriteSession, SlotWriteSessionState};
pub use types::GroupFlags;
pub(crate) use types::NodeLifecycle;
pub(crate) use types::{DetachedSubtree, FinishGroupResult, SlotPassMode};
use types::{NodeRecord, PayloadKind, PayloadRecord};
pub(crate) use validate::SlotInvariantError;
