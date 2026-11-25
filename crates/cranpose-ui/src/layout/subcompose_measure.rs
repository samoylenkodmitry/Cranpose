//! A subcompose node's policy result: from the slots of its last pass, or
//! from a pass that composes and measures the children it asks for.

use std::{cell::RefCell, rc::Rc};

use cranpose_core::{ApplierHost, NodeError, NodeId};
use cranpose_ui_layout::{AlignmentLines, Constraints, Placement};

use super::{
    LayoutBuilderState,
    coordinator_chain::{CoordinatorChain, CoordinatorFrame, SubcomposeCoordinatorLeaf},
    core::Placeable,
};
use crate::{
    modifier::{Point, Size},
    subcompose_layout::{MeasureCallbacks, MeasuredChildren, SubcomposeLayoutNodeHandle},
    widgets::nodes::LayoutParts,
};

#[derive(Clone, Copy)]
pub(crate) enum SubcomposeChainPlan {
    Bypass {
        offset: Point,
        moved_coordinators: bool,
    },
    Direct {
        offset: Point,
        inner_constraints: Constraints,
    },
    General {
        density: f32,
        offset: Point,
    },
}

pub(crate) struct SubcomposeMeasureStart {
    pub(crate) handle: SubcomposeLayoutNodeHandle,
    pub(crate) parts: Rc<LayoutParts>,
    pub(crate) plan: SubcomposeChainPlan,
    pub(crate) measured_children: Rc<MeasuredChildren>,
}

impl SubcomposeMeasureStart {
    pub(crate) fn new(
        handle: SubcomposeLayoutNodeHandle,
        parts: Rc<LayoutParts>,
        plan: SubcomposeChainPlan,
        measured_children: Rc<MeasuredChildren>,
    ) -> Self {
        Self {
            handle,
            parts,
            plan,
            measured_children,
        }
    }
}

pub(super) struct SubcomposeChainMeasurement {
    pub(super) size: Size,
    pub(super) offset: Point,
    pub(super) content_offset: Point,
    pub(super) explicit_lines: AlignmentLines,
    pub(super) inner_constraints: Constraints,
    pub(super) placements: Vec<Placement>,
    pub(super) moved_coordinators: bool,
    pub(super) invalidations: u8,
}

struct SubcomposeLeaf<'a> {
    builder: &'a Rc<LayoutBuilderState>,
    node_handle: &'a SubcomposeLayoutNodeHandle,
    node_id: NodeId,
    measured_children: &'a MeasuredChildren,
    measure_error: &'a RefCell<Option<NodeError>>,
    measured_policy: RefCell<Option<(Constraints, cranpose_ui_layout::MeasureResult)>>,
}

impl SubcomposeCoordinatorLeaf for SubcomposeLeaf<'_> {
    #[inline(never)]
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
        plan: SubcomposeChainPlan,
        constraints: Constraints,
        measured_children: &MeasuredChildren,
    ) -> Result<SubcomposeChainMeasurement, NodeError> {
        let measure_error = RefCell::new(None);
        match plan {
            SubcomposeChainPlan::Bypass {
                offset,
                moved_coordinators,
            } => {
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
                Ok(SubcomposeChainMeasurement {
                    size: result.size,
                    offset,
                    content_offset: Point::default(),
                    explicit_lines: result.alignment_lines,
                    inner_constraints: constraints,
                    placements: result.placements,
                    moved_coordinators,
                    invalidations: 0,
                })
            }
            SubcomposeChainPlan::Direct {
                offset,
                inner_constraints,
            } => {
                let result = self.subcompose_policy_result(
                    node_handle,
                    node_id,
                    inner_constraints,
                    measured_children,
                    &measure_error,
                )?;
                if let Some(err) = measure_error.borrow_mut().take() {
                    return Err(err);
                }
                let (size, accumulated, explicit_lines, moved_coordinators) = {
                    let runtime_state = parts.runtime_state().borrow();
                    let (size, accumulated, explicit_lines) = runtime_state
                        .coordinator_chain
                        .exit_direct(inner_constraints, result.size, result.alignment_lines);
                    (
                        size,
                        accumulated,
                        explicit_lines,
                        runtime_state.take_moved_coordinators(),
                    )
                };
                Ok(SubcomposeChainMeasurement {
                    size,
                    offset,
                    content_offset: Point {
                        x: accumulated.x - offset.x,
                        y: accumulated.y - offset.y,
                    },
                    explicit_lines,
                    inner_constraints,
                    placements: result.placements,
                    moved_coordinators,
                    invalidations: 0,
                })
            }
            SubcomposeChainPlan::General { density, offset } => self
                .measure_subcompose_general_chain(
                    (node_handle, parts, node_id),
                    (density, offset),
                    constraints,
                    measured_children,
                    &measure_error,
                ),
        }
    }

    #[inline(never)]
    fn measure_subcompose_general_chain(
        self: &Rc<Self>,
        (node_handle, parts, node_id): (&SubcomposeLayoutNodeHandle, &LayoutParts, NodeId),
        (density, offset): (f32, Point),
        constraints: Constraints,
        measured_children: &MeasuredChildren,
        measure_error: &RefCell<Option<NodeError>>,
    ) -> Result<SubcomposeChainMeasurement, NodeError> {
        let leaf = SubcomposeLeaf {
            builder: self,
            node_handle,
            node_id,
            measured_children,
            measure_error,
            measured_policy: RefCell::new(None),
        };
        let frame = CoordinatorFrame::for_subcompose(density, &leaf);
        let (placeable, moved_coordinators) = {
            let runtime_state = parts.runtime_state().borrow();
            let placeable = runtime_state
                .coordinator_chain
                .measure_from(0, &frame, constraints);
            (placeable, runtime_state.take_moved_coordinators())
        };
        let invalidations = frame.take_invalidations();
        if let Some(err) = measure_error.borrow_mut().take() {
            return Err(err);
        }

        let (inner_constraints, placements) = leaf
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
            offset,
            content_offset: Point {
                x: content_x - offset.x,
                y: content_y - offset.y,
            },
            explicit_lines: placeable.alignment_lines(),
            inner_constraints,
            placements,
            moved_coordinators,
            invalidations,
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
