//! A subcompose node's policy result: from the slots of its last pass, or
//! from a pass that composes and measures the children it asks for.

use std::{cell::RefCell, rc::Rc};

use cranpose_core::{ApplierHost, NodeError, NodeId};
use cranpose_foundation::InvalidationKind;
use cranpose_ui_layout::{AlignmentLines, Constraints, Placement};

use super::{
    LayoutBuilderState, LayoutNodeContext, ModifierChainInputs,
    coordinator_chain::{CoordinatorChain, CoordinatorLeaf},
    core::Placeable,
};
use crate::{
    modifier::{Point, Size},
    subcompose_layout::{MeasureCallbacks, MeasuredChildren, SubcomposeLayoutNodeHandle},
    widgets::nodes::{IntrinsicKind, LayoutParts},
};

pub(super) struct SubcomposeChainMeasurement {
    pub(super) size: Size,
    pub(super) offset: Point,
    pub(super) content_offset: Point,
    pub(super) explicit_lines: AlignmentLines,
    pub(super) inner_constraints: Constraints,
    pub(super) placements: Vec<Placement>,
    pub(super) moved_coordinators: bool,
}

struct SubcomposeCoordinatorFrame<'a> {
    builder: &'a Rc<LayoutBuilderState>,
    node_handle: &'a SubcomposeLayoutNodeHandle,
    node_id: NodeId,
    measured_children: &'a MeasuredChildren,
    measure_error: &'a RefCell<Option<NodeError>>,
    density: f32,
    context: RefCell<LayoutNodeContext>,
    measured_policy: RefCell<Option<(Constraints, cranpose_ui_layout::MeasureResult)>>,
}

impl<'a> SubcomposeCoordinatorFrame<'a> {
    fn new(
        builder: &'a Rc<LayoutBuilderState>,
        node_handle: &'a SubcomposeLayoutNodeHandle,
        node_id: NodeId,
        measured_children: &'a MeasuredChildren,
        measure_error: &'a RefCell<Option<NodeError>>,
        density: f32,
    ) -> Self {
        Self {
            builder,
            node_handle,
            node_id,
            measured_children,
            measure_error,
            density,
            context: RefCell::new(LayoutNodeContext::new(density)),
            measured_policy: RefCell::new(None),
        }
    }

    fn take_invalidations(&self) -> Vec<InvalidationKind> {
        self.context.borrow_mut().take_invalidations()
    }
}

impl CoordinatorLeaf for SubcomposeCoordinatorFrame<'_> {
    fn density(&self) -> f32 {
        self.density
    }

    fn context(&self) -> &RefCell<LayoutNodeContext> {
        &self.context
    }

    fn measure_leaf(&self, chain: &CoordinatorChain, constraints: Constraints) -> Placeable {
        match self.builder.subcompose_policy_result(
            self.node_handle,
            self.node_id,
            constraints,
            self.measured_children,
            self.measure_error,
        ) {
            Ok(result) => {
                let size = result.size;
                let lines = result.alignment_lines;
                chain.record_leaf(size, constraints);
                *self.measured_policy.borrow_mut() = Some((constraints, result));
                Placeable::value(size.width, size.height, NodeId::default())
                    .with_alignment_lines(lines)
            }
            Err(err) => {
                let mut slot = self.measure_error.borrow_mut();
                if slot.is_none() {
                    *slot = Some(err);
                }
                Placeable::value(0.0, 0.0, NodeId::default())
            }
        }
    }

    fn intrinsic_leaf(&self, _kind: IntrinsicKind) -> f32 {
        0.0
    }
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the subcompose policy result in its file"
)]
impl LayoutBuilderState {
    pub(super) fn measure_subcompose_through_modifier_chain(
        self: &Rc<Self>,
        node_handle: &SubcomposeLayoutNodeHandle,
        parts: &LayoutParts,
        node_id: NodeId,
        chain_inputs: ModifierChainInputs,
        constraints: Constraints,
        measured_children: &MeasuredChildren,
    ) -> Result<SubcomposeChainMeasurement, NodeError> {
        let measure_error = RefCell::new(None);
        if !chain_inputs.uses_chain {
            let result = self.subcompose_policy_result(
                node_handle,
                node_id,
                constraints,
                measured_children,
                &measure_error,
            )?;
            if let Some(err) = measure_error.borrow_mut().take() {
                return Err(err);
            }
            let moved_coordinators = parts.runtime_state().borrow().take_moved_coordinators();
            return Ok(SubcomposeChainMeasurement {
                size: result.size,
                offset: chain_inputs.offset,
                content_offset: Point::default(),
                explicit_lines: result.alignment_lines,
                inner_constraints: constraints,
                placements: result.placements,
                moved_coordinators,
            });
        }

        let frame = SubcomposeCoordinatorFrame::new(
            self,
            node_handle,
            node_id,
            measured_children,
            &measure_error,
            chain_inputs.density.density(),
        );
        let (placeable, moved_coordinators) = {
            let runtime_state = parts.runtime_state().borrow();
            let placeable = runtime_state
                .coordinator_chain
                .measure_from(0, &frame, constraints);
            (placeable, runtime_state.take_moved_coordinators())
        };
        self.mark_invalidations(node_id, frame.take_invalidations());
        if let Some(err) = measure_error.borrow_mut().take() {
            return Err(err);
        }

        let (inner_constraints, placements) = frame
            .measured_policy
            .into_inner()
            .map_or((constraints, Vec::new()), |(inner, result)| {
                (inner, result.placements)
            });
        let (content_x, content_y) = placeable.content_offset();
        Ok(SubcomposeChainMeasurement {
            size: Size {
                width: placeable.width(),
                height: placeable.height(),
            },
            offset: chain_inputs.offset,
            content_offset: Point {
                x: content_x - chain_inputs.offset.x,
                y: content_y - chain_inputs.offset.y,
            },
            explicit_lines: placeable.alignment_lines(),
            inner_constraints,
            placements,
            moved_coordinators,
        })
    }

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
        let _reading = self.context.measure_reads.reading(node_id);
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
                            measured_children.borrow_mut().insert(child_id, measured);
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
                    measured_children.borrow().get(&child_id).copied()
                }),
                error: measure_error,
            },
        )
    }
}
