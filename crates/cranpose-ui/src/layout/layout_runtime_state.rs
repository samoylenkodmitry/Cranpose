//! What a layout node keeps between its measures: its children's records,
//! its modifier coordinators and its measure policy.

use std::rc::Rc;

use cranpose_ui_layout::MeasurePolicy;

use super::{
    ModifierChainInputs, coordinator_chain::CoordinatorChain,
    layout_child_measure_state::LayoutChildren,
};
use crate::{
    modifier::{Rect as GeometryRect, Size},
    widgets::nodes::LayoutNode,
};

pub(crate) struct LayoutRuntimeState {
    /// `None` until the node first binds a child: most nodes are leaves.
    pub(super) children: Option<Box<LayoutChildren>>,
    pub(super) coordinator_chain: CoordinatorChain,
    pub(super) measure_policy: Rc<dyn MeasurePolicy>,
}

impl LayoutRuntimeState {
    pub(crate) fn new(measure_policy: Rc<dyn MeasurePolicy>) -> Self {
        Self {
            children: None,
            coordinator_chain: CoordinatorChain::default(),
            measure_policy,
        }
    }

    pub(super) fn bind_node(&mut self, node: &LayoutNode) -> ModifierChainInputs {
        self.coordinator_chain.sync(node)
    }

    pub(crate) fn measure_policy(&self) -> &dyn MeasurePolicy {
        &*self.measure_policy
    }

    // Gives the node `policy`; false when it holds that one already.
    pub(crate) fn replace_measure_policy(&mut self, policy: Rc<dyn MeasurePolicy>) -> bool {
        let replaced = !Rc::ptr_eq(&self.measure_policy, &policy);
        self.measure_policy = policy;
        replaced
    }

    // Notes that a measure placed the node's coordinators, and says whether
    // one of them moved since the scene last heard of the node. A node
    // without layout modifiers moves only with its size, which the node
    // reports itself.
    pub(super) fn take_moved_coordinators(&self) -> bool {
        self.coordinator_chain.take_moved()
    }

    pub(crate) fn placed_coordinator_rect(
        &self,
        ordinal: usize,
        node_size: Size,
    ) -> Option<GeometryRect> {
        self.coordinator_chain.placed_rect(ordinal, node_size)
    }
}
