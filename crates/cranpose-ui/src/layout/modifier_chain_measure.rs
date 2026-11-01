//! The measure of a layout node through its modifier chain and measure
//! policy.

use cranpose_core::NodeId;
use cranpose_foundation::InvalidationKind;
use cranpose_ui_layout::{Constraints, Placement};

use super::{
    LayoutBuilderState, LayoutRuntimeState, ModifierChainInputs, ModifierChainMeasurement,
    coordinator_chain::CoordinatorFrame, inherited_alignment_lines,
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

        let frame = CoordinatorFrame::new(
            &runtime_state.measure_policy,
            &scope,
            runtime_state.child_measurables(),
            runtime_state.child_states(),
            placements,
            placement_indices,
        );
        let placeable = runtime_state
            .coordinator_chain
            .measure_from(0, &frame, constraints);
        let (content_x, content_y) = placeable.content_offset();

        let invalidations = frame.take_invalidations();
        if !invalidations.is_empty() {
            self.with_applier_result(|applier| {
                applier.with_node::<LayoutNode, _>(node_id, |layout_node| {
                    for kind in invalidations {
                        match kind {
                            InvalidationKind::Layout => layout_node.mark_needs_measure(),
                            InvalidationKind::Draw => layout_node.mark_needs_redraw(),
                            InvalidationKind::Semantics => layout_node.mark_needs_semantics(),
                            InvalidationKind::PointerInput => layout_node.mark_needs_pointer_pass(),
                            InvalidationKind::Focus => layout_node.mark_needs_focus_sync(),
                        }
                    }
                })
            })
            .ok();
        }

        ModifierChainMeasurement {
            alignment_lines: placeable.alignment_lines(),
            size: Size {
                width: placeable.width(),
                height: placeable.height(),
            },
            content_offset: Point {
                x: content_x - chain.offset.x,
                y: content_y - chain.offset.y,
            },
            offset: chain.offset,
            window_root: chain.window_root,
            hold: if chain.window_root {
                None
            } else {
                runtime_state.coordinator_chain.measured_hold(
                    cranpose_ui_layout::MeasureScope::density(&scope),
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
}
