//! What a node's layout shares: its layout state, measurement cache and
//! runtime state, in one allocation.

use std::{cell::RefCell, rc::Rc};

use cranpose_core::NodeId;
use cranpose_ui_graphics::{Rect, Size};
use cranpose_ui_layout::{MeasurePolicy, Placement};

use super::{ChildPlacement, LayoutCells, NodeCache};
use crate::{
    layout::{LayoutChildMeasureState, LayoutRuntimeState},
    render_state::AppContext,
};

// What layout reaches of a node without the applier, and what its parent's
// record of it, its kept measurement and its draws' coordinator rects share:
// one allocation, so the measure path that reads them fetches them together.
pub(crate) struct LayoutParts {
    cache: NodeCache,
    layout_state: LayoutCells,
    runtime_state: RefCell<LayoutRuntimeState>,
    // The record the node's parent keeps of it.
    record: LayoutChildMeasureState,
}

impl LayoutParts {
    pub(crate) fn new(runtime_state: LayoutRuntimeState) -> Self {
        Self {
            cache: NodeCache::default(),
            layout_state: LayoutCells::default(),
            runtime_state: RefCell::new(runtime_state),
            record: LayoutChildMeasureState::default(),
        }
    }

    // Parts with the empty measure policy, whose runtime state stays empty:
    // a subcompose node measures its children through a policy of its own.
    pub(crate) fn empty() -> Self {
        Self::new(LayoutRuntimeState::new(
            super::layout_node::empty_measure_policy(),
        ))
    }

    // Lets go of what the node held, as it leaves the tree for the recycle
    // pool: its parent's record of it, its children and its measure policy.
    // A record that matched the node's revision matches it no more. The
    // block keeps its room for the node that takes it.
    pub(crate) fn release_for_recycle(&self) {
        self.cache.advance_revision();
        self.record.start_over();
        self.runtime_state.borrow_mut().release_for_recycle();
    }

    // Makes `parts`, which `release_for_recycle` let go of, the parts of a
    // new node that `policy` measures, in place. Only parts that no one else
    // reaches refill: no parent's list, no draw's coordinator rect and no
    // child's record holds them. Gives `policy` back otherwise. Inlined: a
    // shell made fresh shares its parts, and its fill reads the count alone.
    #[inline]
    pub(crate) fn refill(
        parts: &Rc<Self>,
        policy: Rc<dyn MeasurePolicy>,
    ) -> Result<(), Rc<dyn MeasurePolicy>> {
        // Another holder of the parts may be in a borrow of them: the count
        // of holders comes before the borrow.
        if Rc::strong_count(parts) != 1 {
            return Err(policy);
        }
        Self::refill_held(parts, policy)
    }

    #[inline(never)]
    fn refill_held(
        parts: &Rc<Self>,
        policy: Rc<dyn MeasurePolicy>,
    ) -> Result<(), Rc<dyn MeasurePolicy>> {
        let mut runtime_state = parts.runtime_state.borrow_mut();
        if Rc::weak_count(parts) != runtime_state.own_parts_handles() {
            return Err(policy);
        }
        runtime_state.refill(policy)?;
        parts.cache.start_over();
        parts.layout_state.start_over();
        Ok(())
    }

    pub(crate) fn cache(&self) -> &NodeCache {
        &self.cache
    }

    pub(crate) fn layout_state(&self) -> &LayoutCells {
        &self.layout_state
    }

    pub(crate) fn record(&self) -> &LayoutChildMeasureState {
        &self.record
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
        self.layout_state.place_child(node_id, at, placer, context);
    }

    pub(crate) fn set_size(&self, node_id: Option<NodeId>, size: Size, context: &AppContext) {
        self.layout_state.set_size(node_id, size, context);
    }

    // Clears the placed flag of node `node_id`, whose parts these are, at the
    // start of a layout pass.
    pub(crate) fn clear_placed(&self, node_id: Option<NodeId>, context: &AppContext) {
        self.layout_state.clear_placed(node_id, context);
    }

    // Notes whether `placements`, the node's latest, give a child a z index
    // other than 0.
    pub(crate) fn note_child_placements(&self, placements: &[Placement]) {
        let with_z = placements.iter().any(|placement| placement.z_index != 0.0);
        self.layout_state.set_places_children_with_z_index(with_z);
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
        if self.layout_state.places_children_with_z_index() {
            super::layout_state::count_z_index_placer(false);
        }
    }
}
