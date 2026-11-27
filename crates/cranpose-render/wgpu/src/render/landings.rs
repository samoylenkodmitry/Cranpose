// What a frame does once pipelines that earlier frames waited for land.

use super::GpuRenderer;

impl GpuRenderer {
    /// Starts a frame: once a pipeline a placeholder waited for has landed,
    /// the retained layers, which may hold that placeholder, are dropped.
    pub(super) fn begin_placeholder_frame(&mut self) {
        self.pipelines.drew_placeholder.set(false);
        if self.pipelines.take_landing_at_frame_start() {
            self.drop_kept_drawing();
        }
    }

    /// Counts the landing of pipelines the browser compiled in the
    /// background, whose draws earlier frames left out: the next frame drops
    /// what those frames drew and kept, the layers and the blurred shadows.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn background_pipelines_landed(&mut self) {
        self.pipelines.compiler.note_landing();
    }

    // Drops the layers and the blurred shadows earlier frames drew and
    // kept, which may hold stand-ins, as pipelines landed.
    fn drop_kept_drawing(&mut self) {
        self.layer_cache.clear();
        self.shadow_surfaces.clear();
        self.pipelines.stand_ins_kept.set(false);
    }

    /// Queues every pipeline the cache file records, as the first frame of
    /// an updated build does, and returns whether any is left to build: for
    /// a renderer that prepares an app's pipelines without drawing.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn prepare_recorded(&mut self) -> bool {
        self.effect_renderer.warm_after_first_frame();
        if let Some(records) = self.pipelines.recent_after_update.take() {
            self.warm_recent(&records);
        }
        self.pipelines.compiling()
    }
}
