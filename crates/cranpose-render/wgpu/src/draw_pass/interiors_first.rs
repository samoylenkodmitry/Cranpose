use super::{PassSegment, ResolvedCompositeKind};
use crate::scene::DrawOpKind;

/// Whether passes on `backend` may lay opaque interiors down first. On GL
/// and in a browser's WebGPU the depth buffer is memory of its own and the
/// pre-pass's draws each cross into JavaScript: in Chrome on a Mate 20 X the
/// gauntlet at tier 8 held 11 MB more and spent 1-2% more CPU a frame with
/// it, for 0.6% fewer GPU cycles.
pub(crate) fn lays_interiors_first(backend: wgpu::Backend) -> bool {
    !matches!(backend, wgpu::Backend::Gl | wgpu::Backend::BrowserWebGpu)
}

/// Whether a pass of `segments` lays its opaque interiors down in a depth
/// pre-pass: one with an opaque fill worth laying down, unless it composites
/// through a runtime shader, whose pipelines draw into passes without a
/// depth buffer alone. A blit takes its place in the pass's order as glyphs
/// and images do.
pub(super) fn takes_depth(segments: &[PassSegment<'_>]) -> bool {
    !cranpose_core::DIAGNOSTICS.no_interiors_first.equals("1")
        && segments.iter().all(|segment| {
            segment
                .composites
                .iter()
                .all(|composite| !matches!(composite.kind, ResolvedCompositeKind::Shader { .. }))
        })
        && segments.iter().any(segment_has_occluders)
}

/// Whether a run `segment` draws holds an opaque fill with an interior worth
/// laying down ahead of the paint.
fn segment_has_occluders(segment: &PassSegment<'_>) -> bool {
    segment
        .ops
        .iter()
        .enumerate()
        .any(|(op_index, op)| match op.kind {
            DrawOpKind::Run(index) => {
                let window = (op_index == 0)
                    .then(|| segment.first_run_window.clone())
                    .flatten()
                    .unwrap_or(0..u32::MAX);
                let run = &segment.scene.runs[index];
                let shapes = &run.tables().shapes;
                let mut first = 0;
                run.segment_records().any(|records| {
                    let start = first;
                    first += records.count;
                    let mut drawn = start.max(window.start)..first.min(window.end);
                    if !records.occluders || drawn.is_empty() {
                        return false;
                    }
                    drawn == (start..first)
                        || drawn.any(|ordinal| {
                            shapes.occludes((records.start + ordinal - start) as usize)
                        })
                })
            }
            _ => false,
        })
}
