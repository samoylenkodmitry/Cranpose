//! What a node's layout shares: its layout state, measurement cache and
//! runtime state, in one allocation.

use std::cell::RefCell;

use cranpose_ui_graphics::{Rect, Size};
use cranpose_ui_layout::Placement;

use super::{LayoutState, NodeCache};
use crate::layout::LayoutRuntimeState;

// What layout reaches of a node without the applier, and what its parent's
// record of it, its kept measurement and its draws' coordinator rects share:
// one allocation, so the measure path that reads them fetches them together.
pub(crate) struct LayoutParts {
    cache: NodeCache,
    layout_state: RefCell<LayoutState>,
    runtime_state: RefCell<LayoutRuntimeState>,
}

impl LayoutParts {
    pub(crate) fn new(runtime_state: LayoutRuntimeState) -> Self {
        Self {
            cache: NodeCache::default(),
            layout_state: RefCell::default(),
            runtime_state: RefCell::new(runtime_state),
        }
    }

    // Parts with the empty measure policy, whose runtime state stays empty:
    // a subcompose node measures its children through a policy of its own.
    pub(crate) fn empty() -> Self {
        Self::new(LayoutRuntimeState::new(
            super::layout_node::empty_measure_policy(),
        ))
    }

    pub(crate) fn cache(&self) -> &NodeCache {
        &self.cache
    }

    pub(crate) fn layout_state(&self) -> &RefCell<LayoutState> {
        &self.layout_state
    }

    // Notes whether `placements`, the node's latest, give a child a z index
    // other than 0.
    pub(crate) fn note_child_placements(&self, placements: &[Placement]) {
        let with_z = placements.iter().any(|placement| placement.z_index != 0.0);
        self.layout_state
            .borrow_mut()
            .set_places_children_with_z_index(with_z);
    }

    pub(crate) fn runtime_state(&self) -> &RefCell<LayoutRuntimeState> {
        &self.runtime_state
    }

    // Where the last measure placed the coordinator `ordinal` of a node of
    // `node_size`, as `CoordinatorRect` names it; `None` before layout placed
    // it.
    pub(crate) fn placed_coordinator_rect(&self, ordinal: usize, node_size: Size) -> Option<Rect> {
        self.runtime_state
            .borrow()
            .placed_coordinator_rect(ordinal, node_size)
    }
}

// A node whose last placement gave a child a z index counts among the
// scene's reasons to order siblings until its parts end.
impl Drop for LayoutParts {
    fn drop(&mut self) {
        if self.layout_state.get_mut().places_children_with_z_index() {
            super::layout_state::count_z_index_placer(false);
        }
    }
}
