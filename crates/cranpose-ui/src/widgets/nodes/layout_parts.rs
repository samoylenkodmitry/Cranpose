//! The layout state and the runtime state of a layout node, in one
//! allocation.

use std::cell::RefCell;

use super::LayoutState;
use crate::layout::LayoutRuntimeState;

// The layout state and the runtime state of a node, which layout reaches
// without the applier and the node's clones share: one allocation, so the
// measure path that reads them fetches them together.
pub(crate) struct LayoutParts {
    pub(super) layout_state: RefCell<LayoutState>,
    pub(super) runtime_state: RefCell<LayoutRuntimeState>,
}

impl LayoutParts {
    pub(crate) fn layout_state(&self) -> &RefCell<LayoutState> {
        &self.layout_state
    }

    pub(crate) fn runtime_state(&self) -> &RefCell<LayoutRuntimeState> {
        &self.runtime_state
    }
}
