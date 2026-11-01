//! A subcompose node's policy result: from the slots of its last pass, or
//! from a pass that composes and measures the children it asks for.

use std::{cell::RefCell, rc::Rc};

use cranpose_core::{ApplierHost, NodeError, NodeId};
use cranpose_ui_layout::Constraints;

use super::{LayoutBuilderState, core::Placeable};
use crate::subcompose_layout::{MeasureCallbacks, MeasuredChildren, SubcomposeLayoutNodeHandle};

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the subcompose policy result in its file"
)]
impl LayoutBuilderState {
    // The result of `node_handle`'s policy at `constraints`: from the slots
    // of its last pass where its retained measure policy keeps them, else
    // from a pass that composes, which puts the children it measures in
    // `measured_children` and its first failure in `measure_error`.
    pub(super) fn subcompose_policy_result(
        self: &Rc<Self>,
        node_handle: &SubcomposeLayoutNodeHandle,
        node_id: NodeId,
        constraints: Constraints,
        measured_children: &MeasuredChildren,
        measure_error: &RefCell<Option<NodeError>>,
    ) -> Result<cranpose_ui_layout::MeasureResult, NodeError> {
        let applier_host: Rc<dyn ApplierHost> = Rc::clone(&self.applier) as Rc<dyn ApplierHost>;
        if let Some(result) = node_handle.measure_retained(&*applier_host, node_id, constraints) {
            return Ok(result);
        }
        let runtime_handle = {
            let mut runtime_handle = self.runtime_handle.borrow_mut();
            if runtime_handle.is_none()
                && let Ok(applier) = self.applier.try_borrow_typed()
            {
                *runtime_handle = applier.runtime_handle();
            }
            runtime_handle.clone().ok_or(NodeError::MissingContext {
                id: node_id,
                reason: "runtime handle required for subcomposition",
            })?
        };
        node_handle.measure_with_callbacks(
            &applier_host,
            &runtime_handle,
            node_id,
            constraints,
            MeasureCallbacks {
                measurer: Box::new(|child_id: NodeId, child_constraints: Constraints| {
                    match self.measure_node(child_id, child_constraints) {
                        Ok(measured) => {
                            measured_children
                                .borrow_mut()
                                .insert(child_id, Rc::clone(&measured));
                            let size = measured.size_for_parent();
                            Placeable::value(size.width, size.height, child_id)
                                .with_alignment_lines(measured.alignment_lines_for_parent())
                        }
                        Err(err) => {
                            let mut slot = measure_error.borrow_mut();
                            if slot.is_none() {
                                *slot = Some(err);
                            }
                            Placeable::value(0.0, 0.0, child_id)
                        }
                    }
                }),
                retained_measure_lookup: Box::new(|child_id| {
                    measured_children.borrow().get(&child_id).cloned()
                }),
                error: measure_error,
            },
        )
    }
}
