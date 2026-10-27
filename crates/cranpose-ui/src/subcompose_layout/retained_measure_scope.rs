// A measure pass of a subcompose node that keeps the slots of its last pass
// as they are and composes nothing.

use std::rc::Rc;

use cranpose_core::{ApplierHost, Node, NodeId, SlotId, SubcomposeState};
use cranpose_foundation::lazy::LazyItemRoots;

use super::{MeasureResult, MeasureScratch, PlacedRootsScope, Placement, record_clean_slot_skip};
use crate::density::{Density, DensityMeasureScope};

// What a retained measure policy reads of its node: the slots its last pass
// kept, the nodes they hold and the buffers it lays out into. The pass keeps
// the slots in the order the last pass activated them; it succeeds when it
// kept every one of them, so it disposes none.
pub(crate) struct RetainedMeasureScope<'a> {
    state: &'a SubcomposeState,
    applier: &'a dyn ApplierHost,
    root_id: NodeId,
    density_scope: DensityMeasureScope,
    owner_epoch: u64,
    scratch: &'a mut MeasureScratch,
    // How many slots of the last pass's active order this pass kept.
    kept: usize,
}

impl<'a> RetainedMeasureScope<'a> {
    pub(super) fn new(
        state: &'a SubcomposeState,
        applier: &'a dyn ApplierHost,
        root_id: NodeId,
        (density, owner_epoch): (Density, u64),
        scratch: &'a mut MeasureScratch,
    ) -> Self {
        Self {
            state,
            applier,
            root_id,
            density_scope: DensityMeasureScope::new(density),
            owner_epoch,
            scratch,
            kept: 0,
        }
    }

    pub(crate) fn root_id(&self) -> NodeId {
        self.root_id
    }

    pub(crate) fn slot_is_active(&self, slot_id: SlotId) -> bool {
        self.state.slot_is_active(slot_id)
    }

    pub(crate) fn slot_is_retained(&self, slot_id: SlotId) -> bool {
        self.state.slot_is_retained(slot_id)
    }

    // Whether the slot has `content_type`, so registering it changes nothing.
    pub(crate) fn content_type_is(&self, slot_id: SlotId, content_type: Option<u64>) -> bool {
        self.state.get_content_type(slot_id) == content_type
    }

    // The slots of the last pass this pass has not kept yet, in order.
    pub(crate) fn unkept_slots(&self) -> &[SlotId] {
        self.state
            .active_slots()
            .get(self.kept..)
            .unwrap_or_default()
    }

    pub(crate) fn kept_every_slot(&self) -> bool {
        self.kept == self.state.active_slots_count()
    }

    // The active slot that holds the focus, as the composing pass finds it.
    pub(crate) fn focused_slot(&self) -> Option<SlotId> {
        let mut node_id = crate::active_focus_target()?;
        let applier = self.applier.borrow_dyn();
        while node_id != self.root_id {
            if let Some(slot) = self.state.active_slot_for_node(node_id) {
                return Some(slot);
            }
            node_id = applier.get_ref(node_id).ok()?.parent()?;
        }
        None
    }

    // Keeps `slot_id`, the next slot of the last pass, with its measured
    // children: its nodes hold `known_children`, none of which needs a
    // measure. The composing pass reuses such an item as it is.
    pub(crate) fn keep_measured(&mut self, slot_id: SlotId, known_children: &[NodeId]) -> bool {
        let Some(nodes) = self.state.kept_slot_nodes_at(self.kept, slot_id) else {
            return false;
        };
        let applier = self.applier.borrow_dyn();
        let mut expected = known_children.iter().copied();
        let mut clean = true;
        for &node_id in nodes {
            let Ok(node) = applier.get_ref(node_id) else {
                return false;
            };
            node.for_each_child(&mut |child| {
                clean &= expected.next() == Some(child)
                    && !applier.get_ref(child).is_ok_and(Node::layout_dirty);
            });
        }
        clean &= expected.next().is_none();
        if clean {
            self.kept += 1;
        }
        clean
    }

    // Keeps `slot_id`, the next slot of the last pass, whose composition for
    // `key` is clean and whose nodes hang under this node: the composing
    // pass keeps such an item beyond the viewport without composing it.
    pub(crate) fn keep_composed<K: PartialEq + 'static>(
        &mut self,
        slot_id: SlotId,
        key: &K,
    ) -> bool {
        if !self.state.retained_capture_key_matches(slot_id, key)
            || self.state.has_pending_precompositions(slot_id)
            || !self
                .state
                .slot_content_generation_current(slot_id, self.owner_epoch)
        {
            return false;
        }
        let Some(nodes) = self.state.kept_slot_nodes_at(self.kept, slot_id) else {
            return false;
        };
        let applier = self.applier.borrow_dyn();
        let in_place = nodes.iter().all(|&node_id| {
            applier
                .get_ref(node_id)
                .is_ok_and(|node| node.parent() == Some(self.root_id))
        });
        if in_place {
            self.kept += 1;
            record_clean_slot_skip();
        }
        in_place
    }
}

impl cranpose_ui_layout::MeasureScope for RetainedMeasureScope<'_> {
    fn density(&self) -> f32 {
        self.density_scope.density()
    }

    fn font_scale(&self) -> f32 {
        self.density_scope.font_scale()
    }
}

impl PlacedRootsScope for RetainedMeasureScope<'_> {
    fn reusable_slots_count(&self) -> usize {
        self.state.reusable_slots_count()
    }

    fn layout_with_placed_roots(
        &mut self,
        width: f32,
        height: f32,
        build: impl FnOnce(&mut Vec<Placement>, &mut Vec<Rc<LazyItemRoots>>),
    ) -> MeasureResult {
        self.scratch.layout_with_placed_roots(width, height, build)
    }
}
