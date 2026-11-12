use cranpose_ui_graphics::BlendMode;

use super::PassScratch;
use crate::{
    effect_renderer::{PreparedCompositeDraw, PreparedProjectiveComposite, PreparedShaderDraw},
    instant_triangles::InstantDraw,
    render::{PassDepth, RunStage, StoreRunBatch, TargetRect},
    run_store::RunDrawCall,
    scissor::intersect_target_rects,
};

pub(super) enum Batch<'a> {
    StoreRun {
        batch: StoreRunBatch,
        scissor: Option<(u32, u32, u32, u32)>,
    },
    /// Draws of an arena chunk, as ranges of the pass's arena draws: the
    /// ones it paints here, and on the batch that closes the chunk, every
    /// draw of the chunk, whose opaque interiors go down together.
    Arena {
        chunk: usize,
        uniform_slot: usize,
        paint: std::ops::Range<usize>,
        interiors: Option<std::ops::Range<usize>>,
        scissor: Option<(u32, u32, u32, u32)>,
        /// The scissor the painted draws' unturned clip puts on them.
        clip: Option<(u32, u32, u32, u32)>,
    },
    Images {
        cmds: std::ops::Range<usize>,
        blend_mode: BlendMode,
        uniform_slot: usize,
        scissor: Option<(u32, u32, u32, u32)>,
    },
    Glyphs {
        cmds: std::ops::Range<usize>,
        uniform_slot: usize,
        scissor: Option<(u32, u32, u32, u32)>,
    },
    Shadows {
        instances: std::ops::Range<u32>,
        uniform_slot: usize,
        scissor: Option<(u32, u32, u32, u32)>,
    },
    Composite(PreparedCompositeDraw<'a>),
    Shader(PreparedShaderDraw<'a>),
    Projective(PreparedProjectiveComposite<'a>),
    Instant(InstantDraw<'a>),
}

/// The scissor a shape batch's `stage` draws under: the batch's, and for its
/// paint only the pixels its unturned clip keeps, `None` when none are left.
/// The interiors keep the batch's: their pipeline cuts each to its clip.
pub(super) fn stage_scissor(
    scissor: TargetRect,
    clip: Option<TargetRect>,
    stage: RunStage,
) -> Option<TargetRect> {
    match (stage, clip) {
        (RunStage::Paint, Some(clip)) => intersect_target_rects(scissor, clip),
        _ => Some(scissor),
    }
}

// A batch of run draw calls: the uniform slot and scissor they draw under,
// the calls, the stage they draw and the meshes a call falls back to while
// its pipeline compiles, with the depth buffer or not.
#[derive(Clone, Copy)]
pub(crate) struct RunCalls<'a> {
    pub(crate) uniform_slot: usize,
    pub(crate) draws: &'a [RunDrawCall],
    pub(crate) scissor: (u32, u32, u32, u32),
    pub(crate) stage: RunStage,
    pub(crate) meshes: Option<(crate::mesh_fallback::RunMeshes<'a>, PassDepth)>,
}

/// The commands a pass's batches draw ranges of.
#[derive(Clone, Copy)]
pub(super) struct PassCmds<'a> {
    pub(super) images: &'a [crate::render::ImageDrawCmd],
    pub(super) glyphs: &'a [crate::render::GlyphDrawCmd],
    pub(super) arena: &'a [RunDrawCall],
    /// The pass's fallback triangles: the index range of each of `arena`'s
    /// draws that takes one, and the chunks the indices count in.
    pub(super) meshes: &'a crate::mesh_fallback::PassMeshes,
}

/// Draws each glyph batch's retained runs after its shared quads, and its
/// labels one kind, turned or upright, at a time, where their order allows:
/// see [`crate::render::defer_retained_glyph_runs`] and
/// [`crate::render::group_glyph_kinds`].
pub(super) fn group_glyph_batches(batches: &[Batch<'_>], scratch: &mut PassScratch) {
    for batch in batches {
        if let Batch::Glyphs { cmds, .. } = batch {
            let cmds = &mut scratch.glyph_cmds[cmds.clone()];
            let shared = crate::render::defer_retained_glyph_runs(cmds, &mut scratch.glyph_moved);
            crate::render::group_glyph_kinds(&mut cmds[..shared], &mut scratch.glyph_moved);
        }
    }
}
