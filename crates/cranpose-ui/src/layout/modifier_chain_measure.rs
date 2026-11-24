//! The measure of a layout node through its modifier chain and measure
//! policy.

use cranpose_core::{Applier, NodeId};
use cranpose_ui_layout::{Constraints, Placement};

use super::{
    LayoutBuilderState, LayoutNodeContext, LayoutRuntimeState, ModifierChainInputs,
    ModifierChainMeasurement, coordinator_chain::CoordinatorFrame, inherited_alignment_lines,
};
use crate::{
    modifier::{Point, Size},
    widgets::nodes::LayoutNode,
};

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the chain measure in its file"
)]
impl LayoutBuilderState {
    pub(super) fn measure_through_modifier_chain(
        &self,
        node_id: NodeId,
        runtime_state: &LayoutRuntimeState,
        chain: ModifierChainInputs,
        constraints: Constraints,
        placements: &mut Vec<Placement>,
        placement_indices: &mut Vec<usize>,
    ) -> ModifierChainMeasurement {
        let _reading = self.context.measure_reads.reading(node_id);
        let scope = crate::density::DensityMeasureScope::new(chain.density);

        if !chain.uses_chain {
            let measurement = runtime_state.measure_policy.measure_into(
                &scope,
                runtime_state.child_measurables(),
                constraints,
                placements,
            );
            return ModifierChainMeasurement {
                size: measurement.size,
                alignment_lines: inherited_alignment_lines(
                    runtime_state.child_states(),
                    placements,
                    placement_indices,
                )
                .with_overrides(measurement.alignment_lines),
                content_offset: Point::default(),
                offset: chain.offset,
                window_root: chain.window_root,
                hold: if chain.window_root {
                    None
                } else {
                    runtime_state.measure_policy.measure_hold(
                        runtime_state.child_measurables(),
                        constraints,
                        measurement.size,
                    )
                },
                ignores_child_sizes: !chain.window_root
                    && runtime_state
                        .measure_policy
                        .ignores_child_sizes(runtime_state.child_measurables(), constraints),
            };
        }

        if runtime_state.coordinator_chain.is_direct() {
            let density = cranpose_ui_layout::MeasureScope::density(&scope);
            let inner_constraints = runtime_state
                .coordinator_chain
                .enter_direct(constraints, density);
            let measurement = runtime_state.measure_policy.measure_into(
                &scope,
                runtime_state.child_measurables(),
                inner_constraints,
                placements,
            );
            let inner_lines = inherited_alignment_lines(
                runtime_state.child_states(),
                placements,
                placement_indices,
            )
            .with_overrides(measurement.alignment_lines);
            let (size, accumulated, alignment_lines) = runtime_state.coordinator_chain.exit_direct(
                inner_constraints,
                measurement.size,
                inner_lines,
            );
            return finish_chain_measurement(
                runtime_state,
                chain,
                density,
                size,
                accumulated,
                alignment_lines,
            );
        }

        self.measure_through_general_chain(
            (node_id, runtime_state, chain),
            &scope,
            constraints,
            placements,
            placement_indices,
        )
    }

    #[inline(never)]
    fn measure_through_general_chain(
        &self,
        (node_id, runtime_state, chain): (NodeId, &LayoutRuntimeState, ModifierChainInputs),
        scope: &crate::density::DensityMeasureScope,
        constraints: Constraints,
        placements: &mut Vec<Placement>,
        placement_indices: &mut Vec<usize>,
    ) -> ModifierChainMeasurement {
        let frame = CoordinatorFrame::new(
            &runtime_state.measure_policy,
            scope,
            runtime_state.child_measurables(),
            runtime_state.child_states(),
            placements,
            placement_indices,
        );
        let placeable = runtime_state
            .coordinator_chain
            .measure_from(0, &frame, constraints);
        let (content_x, content_y) = placeable.content_offset();
        self.mark_invalidations(node_id, frame.take_invalidations());

        finish_chain_measurement(
            runtime_state,
            chain,
            cranpose_ui_layout::MeasureScope::density(scope),
            Size {
                width: placeable.width(),
                height: placeable.height(),
            },
            Point {
                x: content_x,
                y: content_y,
            },
            placeable.alignment_lines(),
        )
    }
    // Marks the node `node_id` with what its modifier chain invalidated while
    // it measured.
    #[inline]
    pub(super) fn mark_invalidations(&self, node_id: NodeId, invalidations: u8) {
        if invalidations != 0 {
            self.mark_invalidations_slow(node_id, invalidations);
        }
    }

    #[inline(never)]
    fn mark_invalidations_slow(&self, node_id: NodeId, invalidations: u8) {
        self.with_applier_result(|applier| {
            let Ok(node) = applier.get_mut(node_id) else {
                return Ok(());
            };
            let any = node.as_any_mut();
            if let Some(layout_node) = any.downcast_mut::<LayoutNode>() {
                if invalidations & LayoutNodeContext::LAYOUT_BIT != 0 {
                    layout_node.mark_needs_measure();
                }
                if invalidations & LayoutNodeContext::DRAW_BIT != 0 {
                    layout_node.mark_needs_redraw();
                }
                if invalidations & LayoutNodeContext::SEMANTICS_BIT != 0 {
                    layout_node.mark_needs_semantics();
                }
                if invalidations & LayoutNodeContext::POINTER_INPUT_BIT != 0 {
                    layout_node.mark_needs_pointer_pass();
                }
                if invalidations & LayoutNodeContext::FOCUS_BIT != 0 {
                    layout_node.mark_needs_focus_sync();
                }
            } else if let Some(subcompose) =
                any.downcast_mut::<crate::subcompose_layout::SubcomposeLayoutNode>()
            {
                if invalidations & LayoutNodeContext::LAYOUT_BIT != 0 {
                    subcompose.mark_needs_measure();
                }
                if invalidations & LayoutNodeContext::DRAW_BIT != 0 {
                    subcompose.mark_needs_redraw();
                }
                if invalidations & LayoutNodeContext::SEMANTICS_BIT != 0 {
                    subcompose.mark_needs_semantics();
                }
                if invalidations & LayoutNodeContext::POINTER_INPUT_BIT != 0 {
                    subcompose.mark_needs_pointer_pass();
                }
                if invalidations & LayoutNodeContext::FOCUS_BIT != 0 {
                    subcompose.mark_needs_focus_sync();
                }
            }
            Ok(())
        })
        .ok();
    }
}

#[inline]
fn finish_chain_measurement(
    runtime_state: &LayoutRuntimeState,
    chain: ModifierChainInputs,
    density: f32,
    size: Size,
    accumulated: Point,
    alignment_lines: cranpose_ui_layout::AlignmentLines,
) -> ModifierChainMeasurement {
    ModifierChainMeasurement {
        alignment_lines,
        size,
        content_offset: Point {
            x: accumulated.x - chain.offset.x,
            y: accumulated.y - chain.offset.y,
        },
        offset: chain.offset,
        window_root: chain.window_root,
        hold: if chain.window_root {
            None
        } else {
            runtime_state.coordinator_chain.measured_hold(
                density,
                runtime_state.measure_policy.as_ref(),
                runtime_state.child_measurables(),
            )
        },
        ignores_child_sizes: !chain.window_root
            && runtime_state.coordinator_chain.ignores_child_sizes(
                runtime_state.measure_policy.as_ref(),
                runtime_state.child_measurables(),
            ),
    }
}
