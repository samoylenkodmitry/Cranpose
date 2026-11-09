//! What a node's layout shares: its layout state, measurement cache and
//! runtime state, in one allocation.

use std::cell::RefCell;

use cranpose_core::NodeId;
use cranpose_ui_graphics::{Rect, Size};
use cranpose_ui_layout::Placement;

use super::{ChildPlacement, LayoutState, NodeCache};
use crate::{layout::LayoutRuntimeState, render_state::AppContext};

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

    // Places node `node_id`, whose parts these are, where `placer` placed it
    // with `at`.
    pub(crate) fn place(
        &self,
        node_id: Option<NodeId>,
        at: ChildPlacement,
        placer: Option<NodeId>,
        context: &AppContext,
    ) {
        self.layout_state
            .borrow_mut()
            .place_child(node_id, at, placer, context);
    }

    // Writes the measured size of node `node_id`, whose parts these are.
    pub(crate) fn set_size(&self, node_id: Option<NodeId>, size: Size, context: &AppContext) {
        self.layout_state
            .borrow_mut()
            .set_size(node_id, size, context);
    }

    // Clears the placed flag of node `node_id`, whose parts these are, at the
    // start of a layout pass.
    pub(crate) fn clear_placed(&self, node_id: Option<NodeId>, context: &AppContext) {
        self.layout_state
            .borrow_mut()
            .clear_placed(node_id, context);
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
