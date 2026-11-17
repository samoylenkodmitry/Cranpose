use cranpose_core::NodeId;
use cranpose_ui_layout::Constraints;
use web_time::Instant;

use super::MeasuredNode;

pub(super) fn layout_measure_telemetry_threshold_ms() -> Option<f64> {
    let diagnostics = &cranpose_core::DIAGNOSTICS;
    diagnostics
        .layout_measure_telemetry_ms
        .threshold_ms()
        .or_else(|| diagnostics.layout_measure_telemetry.is_set().then_some(4.0))
}

/// The stages of a layout pass that its telemetry times: start, repasses,
/// applier guard, builder, measure, root placement, layout and semantics
/// trees, builder drop and guard drop.
pub(super) type LayoutStages = cranpose_core::StageTimes<9>;

pub(super) fn log_layout_measure_telemetry(root: NodeId, stages: &LayoutStages) {
    let Some(threshold_ms) = layout_measure_telemetry_threshold_ms() else {
        return;
    };
    let Some(
        [
            start,
            after_repasses,
            after_guard,
            after_builder,
            after_measure,
            after_root_place,
            after_aux,
            after_builder_drop,
            after_guard_drop,
        ],
    ) = stages.times()
    else {
        return;
    };
    let ms = cranpose_core::stage_ms;
    let total_ms = ms(start, after_guard_drop);
    if total_ms < threshold_ms {
        return;
    }
    log::warn!(
        "[layout-measure-telemetry] root={root} total_ms={total_ms:.2} repass_ms={:.2} guard_ms={:.2} builder_ms={:.2} measure_ms={:.2} root_place_ms={:.2} aux_ms={:.2} builder_drop_ms={:.2} guard_drop_ms={:.2}",
        ms(start, after_repasses),
        ms(after_repasses, after_guard),
        ms(after_guard, after_builder),
        ms(after_builder, after_measure),
        ms(after_measure, after_root_place),
        ms(after_root_place, after_aux),
        ms(after_aux, after_builder_drop),
        ms(after_builder_drop, after_guard_drop),
    );
}

pub(super) fn log_node_measure_telemetry(
    kind: &'static str,
    node_id: NodeId,
    constraints: Constraints,
    measured: &MeasuredNode,
    (threshold_ms, start): (f64, Instant),
) {
    let size = measured.size;
    let children = measured.children.len();
    let total_ms = start.elapsed().as_secs_f64() * 1000.0;
    if total_ms < threshold_ms {
        return;
    }

    log::warn!(
        "[layout-node-telemetry] kind={kind} node={} total_ms={total_ms:.2} constraints=({:.1},{:.1},{:.1},{:.1}) size=({:.1},{:.1}) children={children}",
        node_id,
        constraints.min_width,
        constraints.max_width,
        constraints.min_height,
        constraints.max_height,
        size.width,
        size.height,
    );
}
