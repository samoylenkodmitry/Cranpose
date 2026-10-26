//! What a node's layout shares: its layout state, measurement cache, runtime
//! state and coordinator rects, in one allocation.

use std::cell::RefCell;

use super::{LayoutState, NodeCache};
use crate::{layout::LayoutRuntimeState, modifier::CoordinatorGeometry};

// What layout reaches of a node without the applier, and what its parent's
// record of it, its kept measurement and its draws' coordinator rects share:
// one allocation, so the measure path that reads them fetches them together.
pub(crate) struct LayoutParts {
    cache: NodeCache,
    layout_state: RefCell<LayoutState>,
    runtime_state: RefCell<LayoutRuntimeState>,
    coordinator_geometry: CoordinatorGeometry,
}

impl LayoutParts {
    pub(crate) fn new(runtime_state: LayoutRuntimeState) -> Self {
        Self {
            cache: NodeCache::default(),
            layout_state: RefCell::default(),
            runtime_state: RefCell::new(runtime_state),
            coordinator_geometry: CoordinatorGeometry::default(),
        }
    }

    pub(crate) fn cache(&self) -> &NodeCache {
        &self.cache
    }

    pub(crate) fn layout_state(&self) -> &RefCell<LayoutState> {
        &self.layout_state
    }

    pub(crate) fn runtime_state(&self) -> &RefCell<LayoutRuntimeState> {
        &self.runtime_state
    }

    pub(crate) fn coordinator_geometry(&self) -> &CoordinatorGeometry {
        &self.coordinator_geometry
    }
}
