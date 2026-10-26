const DEFAULT_FRAME_STAGE_TELEMETRY_THRESHOLD_MS: f64 = 4.0;

pub(super) fn frame_stage_telemetry_threshold_ms() -> Option<f64> {
    static THRESHOLD_MS: std::sync::OnceLock<Option<f64>> = std::sync::OnceLock::new();
    *THRESHOLD_MS.get_or_init(|| {
        let explicit = std::env::var("CRANPOSE_FRAME_STAGE_TELEMETRY_MS")
            .ok()
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| value.is_finite() && *value >= 0.0);
        explicit.or_else(|| {
            std::env::var_os("CRANPOSE_FRAME_STAGE_TELEMETRY")
                .is_some()
                .then_some(DEFAULT_FRAME_STAGE_TELEMETRY_THRESHOLD_MS)
        })
    })
}

/// The stages of a frame that its telemetry times: start, initial layout,
/// post-layout recomposition, dispatch and render.
pub(super) type FrameStages = cranpose_core::StageTimes<5>;

pub(super) fn log_frame_stage_telemetry(stages: &FrameStages) {
    let Some(threshold_ms) = frame_stage_telemetry_threshold_ms() else {
        return;
    };
    let Some(
        [
            frame_start,
            after_initial_layout,
            after_layout,
            after_dispatch,
            after_render,
        ],
    ) = stages.times()
    else {
        return;
    };
    let ms = cranpose_core::stage_ms;
    let total_ms = ms(frame_start, after_render);
    if total_ms < threshold_ms {
        return;
    }
    log::warn!(
        "[frame-stage-telemetry] total_ms={total_ms:.2} layout_ms={:.2} initial_layout_ms={:.2} post_layout_ms={:.2} dispatch_ms={:.2} scene_ms={:.2}",
        ms(frame_start, after_layout),
        ms(frame_start, after_initial_layout),
        ms(after_initial_layout, after_layout),
        ms(after_layout, after_dispatch),
        ms(after_dispatch, after_render),
    );
}

pub(super) fn render_phase_dirty_diagnostics_enabled() -> bool {
    cranpose_core::env_flag!("CRANPOSE_RENDER_PHASE_DIRTY_DIAG")
}

pub(super) struct RenderPhaseDirtyDiagnostics<'a> {
    pub(super) render_dirty: bool,
    pub(super) pointer_dirty: bool,
    pub(super) scene_dirty: bool,
    pub(super) draw_repass_pending: bool,
    pub(super) draw_dirty_nodes: usize,
    pub(super) layer_property_nodes: usize,
    pub(super) layout_dirty_nodes: usize,
    pub(super) moved_nodes: usize,
    pub(super) structural_dirty_nodes: usize,
    pub(super) scene_updates: usize,
    pub(super) dirty_node_ids: Option<String>,
    pub(super) render_only_dirty: bool,
    pub(super) recomposed_this_frame: bool,
    pub(super) path: &'a str,
}

pub(super) fn log_render_phase_dirty_diagnostics(diagnostics: RenderPhaseDirtyDiagnostics<'_>) {
    if !render_phase_dirty_diagnostics_enabled() {
        return;
    }

    let RenderPhaseDirtyDiagnostics {
        render_dirty,
        pointer_dirty,
        scene_dirty,
        draw_repass_pending,
        draw_dirty_nodes,
        layer_property_nodes,
        layout_dirty_nodes,
        moved_nodes,
        structural_dirty_nodes,
        scene_updates,
        dirty_node_ids,
        render_only_dirty,
        recomposed_this_frame,
        path,
    } = diagnostics;
    if let Some(dirty_node_ids) = dirty_node_ids {
        log::warn!(
            "[render-phase-dirty] path={path} render_dirty={render_dirty} pointer_dirty={pointer_dirty} scene_dirty={scene_dirty} draw_repass_pending={draw_repass_pending} draw_dirty_nodes={draw_dirty_nodes} layer_property_nodes={layer_property_nodes} layout_dirty_nodes={layout_dirty_nodes} moved_nodes={moved_nodes} structural_dirty_nodes={structural_dirty_nodes} scene_updates={scene_updates} render_only_dirty={render_only_dirty} recomposed_this_frame={recomposed_this_frame} ids={dirty_node_ids}",
        );
    } else {
        log::warn!(
            "[render-phase-dirty] path={path} render_dirty={render_dirty} pointer_dirty={pointer_dirty} scene_dirty={scene_dirty} draw_repass_pending={draw_repass_pending} draw_dirty_nodes={draw_dirty_nodes} layer_property_nodes={layer_property_nodes} layout_dirty_nodes={layout_dirty_nodes} moved_nodes={moved_nodes} structural_dirty_nodes={structural_dirty_nodes} scene_updates={scene_updates} render_only_dirty={render_only_dirty} recomposed_this_frame={recomposed_this_frame}",
        );
    }
}
