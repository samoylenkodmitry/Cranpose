use super::{
    GpuRenderer, ImageTextures, RendererPipelines, ViewportUniforms, text_resources::TextResources,
};
use crate::{glyph_run_arena::GlyphRunArena, gpu_stats::FrameStats};

// What a frame's draws upload through: the pipelines whose layouts and
// samplers new bindings use, the viewport uniforms a draw claims a slot in,
// and the queue and stats a texture's upload goes through.
pub(crate) struct FrameUploads<'a> {
    pub(super) pipelines: &'a RendererPipelines,
    pub(super) uniforms: &'a mut ViewportUniforms,
    pub(super) queue: &'a wgpu::Queue,
    pub(super) stats: &'a mut FrameStats,
}

// The renderer's parts a draw of text or an image borrows apart: the text
// caches, the image textures and what their uploads go through.
pub(crate) struct DrawParts<'a> {
    pub(crate) text: &'a mut TextResources,
    pub(crate) images: &'a mut ImageTextures,
    pub(crate) upload: FrameUploads<'a>,
}

#[expect(clippy::multiple_inherent_impl, reason = "frame uploads in their file")]
impl GpuRenderer {
    // Drops what a frame that submitted nothing left behind: uploads no
    // command reached, and kept outputs no draw filled.
    pub(super) fn drop_unsubmitted_frame(&mut self) {
        self.layer_cache.disk.forget_unsubmitted();
        self.run_store.invalidate_uploads();
        self.text.glyph_gpu_run_cache.clear();
        self.text.glyph_run_arena = GlyphRunArena::new(self.copy_free);
    }
}
