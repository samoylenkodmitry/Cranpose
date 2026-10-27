use super::{HeldDraws, SegmentRun};
use crate::{
    instant_triangles::InstantTriangles, rrect_shadow::ShadowInstance, run_store::RunDrawCall,
    shape_pipelines::PipelineDemand,
};

#[derive(Default)]
pub(crate) struct PassScratch {
    pub(crate) images: crate::render::ImageBatch,
    pub(crate) glyph_instances: crate::render::GlyphInstances,
    pub(super) glyph_cmds: Vec<crate::render::GlyphDrawCmd>,
    /// Where a glyph batch's draws of one kind wait while it groups them.
    pub(super) glyph_moved: Vec<crate::render::GlyphDrawCmd>,
    pub(super) shadow_instances: Vec<ShadowInstance>,
    pub(super) arena_draws: Vec<RunDrawCall>,
    pub(super) pipelines: PipelineDemand,
    pub(super) held: HeldDraws,
    pub(crate) meshes: crate::mesh_fallback::PassMeshes,
    pub(super) instant: InstantTriangles,
}

/// Where the image draws an item appends start in a pass's scratch.
#[derive(Clone, Copy)]
pub(super) struct ImageMark {
    vertices: usize,
    clips: usize,
    pub(super) cmds: usize,
}

impl PassScratch {
    // Opens a frame: stand-in triangles keep their storage only while
    // frames lay them.
    pub(crate) fn begin_frame(&mut self) {
        self.meshes.begin_frame();
        self.instant.begin_frame();
    }

    pub(super) fn image_mark(&self) -> ImageMark {
        ImageMark {
            vertices: self.images.vertices.len(),
            clips: self.images.clips.len(),
            cmds: self.images.cmds.len(),
        }
    }

    /// Moves the image draws appended since `mark` into a member's binding
    /// space and keeps them within its slot; a segment of its own keeps
    /// them as they are.
    pub(super) fn place_images(&mut self, mark: ImageMark, run: &SegmentRun<'_, '_>) {
        if let Some(slot) = run.slot {
            crate::render::place_image_draws(
                (
                    &mut self.images.vertices[mark.vertices..],
                    &mut self.images.clips[mark.clips..],
                    &mut self.images.cmds[mark.cmds..],
                ),
                run.shift,
                slot,
            );
        }
    }

    /// Moves the glyph quads appended from `instances` on, for the glyph
    /// commands from `cmds` on, into a member's binding space and keeps
    /// the commands within its slot.
    pub(super) fn place_glyphs(
        &mut self,
        (instances, cmds): (usize, usize),
        run: &SegmentRun<'_, '_>,
    ) {
        if let Some(slot) = run.slot {
            self.glyph_instances.shift_plain(instances, run.shift);
            for cmd in &mut self.glyph_cmds[cmds..] {
                cmd.confine(slot);
            }
        }
    }
}
