use web_time::Instant;

const DEFAULT_WGPU_RENDER_STAGE_TELEMETRY_THRESHOLD_MS: f64 = 4.0;

fn wgpu_render_stage_telemetry_threshold_ms() -> Option<f64> {
    let diagnostics = &cranpose_core::DIAGNOSTICS;
    diagnostics
        .wgpu_render_stage_telemetry_ms
        .threshold_ms()
        .or_else(|| {
            diagnostics
                .wgpu_render_stage_telemetry
                .is_set()
                .then_some(DEFAULT_WGPU_RENDER_STAGE_TELEMETRY_THRESHOLD_MS)
        })
}

pub(crate) fn instant_ms(start: Instant, end: Instant) -> f64 {
    end.duration_since(start).as_secs_f64() * 1000.0
}

pub(crate) fn should_log_wgpu_render_stage(start: Instant, end: Instant) -> Option<f64> {
    let threshold_ms = wgpu_render_stage_telemetry_threshold_ms()?;
    let total_ms = instant_ms(start, end);
    (total_ms >= threshold_ms).then_some(total_ms)
}
