use std::cell::{Cell, RefCell};

use cranpose_core::NodeId;
use cranpose_render_common::raster_cache::{LAYER_RASTER_CACHE_KIND_COUNT, LayerRasterCacheKey};
use cranpose_ui_graphics::Rect;

use super::{
    FrameDamage, FrameStatsSnapshot, IsolatedLayerStat, TOP_ISOLATED_LAYER_LIMIT,
    shadow_cache_diagnostics_enabled,
};
use crate::{frame_graph::FrameCommandStats, run_geometry::ShapeFill};

#[derive(Default)]
pub(crate) struct FrameStats {
    pub submits: Cell<u32>,
    pub command_encoder_count: Cell<u32>,
    pub command_submit_count: Cell<u32>,
    pub command_pass_count: Cell<u32>,
    pub command_pass_pixels: Cell<u64>,
    pub command_copy_count: Cell<u32>,
    pub command_copy_pixels: Cell<u64>,
    pub command_transient_texture_bytes: Cell<u64>,
    pub command_upload_bytes: Cell<u64>,
    pub offscreen_acquires: Cell<u32>,
    pub offscreen_news: Cell<u32>,
    pub offscreen_total_bytes: Cell<u64>,
    pub upload_writes: Cell<u32>,
    pub isolated_layer_renders: Cell<u32>,
    pub isolated_layer_pixels: Cell<u64>,
    pub layer_cache_hits: Cell<u32>,
    pub capture_fixup_passes: Cell<u32>,
    pub layer_cache_misses: Cell<u32>,
    pub layer_cache_hit_pixels: Cell<u64>,
    pub layer_cache_miss_pixels: Cell<u64>,
    pub layer_cache_hits_by_kind: [Cell<u32>; LAYER_RASTER_CACHE_KIND_COUNT],
    pub layer_cache_misses_by_kind: [Cell<u32>; LAYER_RASTER_CACHE_KIND_COUNT],
    pub layer_cache_miss_pixels_by_kind: [Cell<u64>; LAYER_RASTER_CACHE_KIND_COUNT],
    pub shadow_shape_cache_hits: Cell<u32>,
    pub shadow_shape_cache_misses: Cell<u32>,
    pub shadow_shape_cache_hit_pixels: Cell<u64>,
    pub shadow_shape_cache_miss_pixels: Cell<u64>,
    pub shadow_text_blur_fallbacks: Cell<u32>,
    pub blur_passes: Cell<u32>,
    pub stages: Cell<u32>,
    pub backdrop_admissions: Cell<u32>,
    pub prefix_admissions: Cell<u32>,
    pub substrates: Cell<u32>,
    pub composite_passes: Cell<u32>,
    pub effect_applies: Cell<u32>,
    pub shader_pixels: Cell<u64>,
    pub glass_rasterized_pixels: Cell<u64>,
    pub blur_pixels: Cell<u64>,
    pub shape_fill_pixels: Cell<u64>,
    pub shape_fill_pixels_by_class: Cell<[u64; ShapeFill::CLASSES]>,
    pub shape_vertices: Cell<u64>,
    pub shape_passes: Cell<u32>,
    pub shape_pipeline_fallback_draws: Cell<u32>,
    pub instant_draws: Cell<u32>,
    pub shape_specialized_draws: Cell<u32>,
    pub shape_interior_draws: Cell<u32>,
    pub depth_passes: Cell<u32>,
    pub shader_pipeline_fallback_draws: Cell<u32>,
    pub shader_specialized_draws: Cell<u32>,
    pub placeholder_draws: Cell<u32>,
    pub image_passes: Cell<u32>,
    pub text_passes: Cell<u32>,
    pub draw_calls: Cell<u32>,
    pub damage: Cell<FrameDamage>,
    pub repainted_pixels: Cell<u64>,
    pub text_image_cache_hits: Cell<u32>,
    pub text_image_cache_misses: Cell<u32>,
    pub text_image_cache_hit_pixels: Cell<u64>,
    pub text_image_cache_miss_pixels: Cell<u64>,
    pub text_image_raster_bytes: Cell<u64>,
    pub text_glyph_atlas_hits: Cell<u32>,
    pub text_glyph_atlas_misses: Cell<u32>,
    pub text_glyph_atlas_miss_pixels: Cell<u64>,
    pub text_glyph_runs_collected: Cell<u32>,
    pub offscreen_pool_size: Cell<u32>,
    pub offscreen_pool_bytes: Cell<u64>,
    pub text_pool_size: Cell<u32>,
    pub layer_cache_size: Cell<u32>,
    pub layer_cache_bytes: Cell<u64>,
    pub image_cache_size: Cell<u32>,
    top_isolated_layers: RefCell<[Option<IsolatedLayerStat>; TOP_ISOLATED_LAYER_LIMIT]>,
    top_isolated_layer_count: Cell<usize>,
    shadow_shape_cache_miss_log_count: Cell<u32>,
}

impl FrameStats {
    /// Whether the frame drew anything in place of what its pipelines
    /// draw: a placeholder, a left-out draw, fallback triangles or a
    /// shader's general pipeline.
    pub(crate) fn drew_stand_in(&self) -> bool {
        self.placeholder_draws.get() != 0
            || self.instant_draws.get() != 0
            || self.shader_pipeline_fallback_draws.get() != 0
    }

    pub(crate) fn record_capture_fixup_pass(&self) {
        self.capture_fixup_passes
            .set(self.capture_fixup_passes.get().saturating_add(1));
    }

    pub(crate) fn record_stages(&self, count: u32) {
        self.stages.set(self.stages.get().saturating_add(count));
    }

    pub(crate) fn record_backdrop_admission(&self) {
        self.backdrop_admissions
            .set(self.backdrop_admissions.get().saturating_add(1));
    }

    pub(crate) fn record_prefix_admission(&self) {
        self.prefix_admissions
            .set(self.prefix_admissions.get().saturating_add(1));
    }

    pub(crate) fn record_command_stats(&self, stats: FrameCommandStats) {
        self.submits
            .set(self.submits.get().saturating_add(stats.submit_count));
        self.command_encoder_count.set(
            self.command_encoder_count
                .get()
                .saturating_add(stats.encoder_count),
        );
        self.command_submit_count.set(
            self.command_submit_count
                .get()
                .saturating_add(stats.submit_count),
        );
        self.command_pass_count.set(
            self.command_pass_count
                .get()
                .saturating_add(stats.pass_count),
        );
        self.command_pass_pixels.set(
            self.command_pass_pixels
                .get()
                .saturating_add(stats.pass_pixels),
        );
        self.command_copy_count.set(
            self.command_copy_count
                .get()
                .saturating_add(stats.copy_count),
        );
        self.command_copy_pixels.set(
            self.command_copy_pixels
                .get()
                .saturating_add(stats.copy_pixels),
        );
        self.command_transient_texture_bytes.set(
            self.command_transient_texture_bytes
                .get()
                .saturating_add(stats.transient_texture_bytes),
        );
        self.command_upload_bytes.set(
            self.command_upload_bytes
                .get()
                .saturating_add(stats.upload_bytes),
        );
        self.upload_writes
            .set(self.upload_writes.get().saturating_add(stats.upload_writes));
        self.offscreen_acquires.set(
            self.offscreen_acquires
                .get()
                .saturating_add(stats.transient_acquires),
        );
        self.offscreen_news.set(
            self.offscreen_news
                .get()
                .saturating_add(stats.transient_news),
        );
    }

    pub(crate) fn record_offscreen_acquire(
        &self,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
        is_new: bool,
    ) {
        self.offscreen_acquires
            .set(self.offscreen_acquires.get() + 1);
        if is_new {
            self.offscreen_news.set(self.offscreen_news.get() + 1);
        }
        self.offscreen_total_bytes.set(
            self.offscreen_total_bytes.get()
                + (width as u64)
                    * (height as u64)
                    * crate::frame_graph::texture_format_bytes_per_pixel(format),
        );
    }

    pub(crate) fn record_isolated_layer_render(
        &self,
        width: u32,
        height: u32,
        node_id: Option<NodeId>,
        logical_rect: Rect,
    ) {
        self.isolated_layer_renders
            .set(self.isolated_layer_renders.get().saturating_add(1));
        self.isolated_layer_pixels.set(
            self.isolated_layer_pixels
                .get()
                .saturating_add((width as u64) * (height as u64)),
        );
        self.record_top_isolated_layer(IsolatedLayerStat {
            node_id,
            logical_rect,
            width,
            height,
        });
    }

    pub(crate) fn record_layer_cache_hit(
        &self,
        key: &LayerRasterCacheKey,
        width: u32,
        height: u32,
    ) {
        let by_kind = &self.layer_cache_hits_by_kind[key.kind_slot()];
        by_kind.set(by_kind.get().saturating_add(1));
        self.layer_cache_hits
            .set(self.layer_cache_hits.get().saturating_add(1));
        self.layer_cache_hit_pixels.set(
            self.layer_cache_hit_pixels
                .get()
                .saturating_add((width as u64) * (height as u64)),
        );
    }

    pub(crate) fn record_layer_cache_miss(
        &self,
        key: &LayerRasterCacheKey,
        width: u32,
        height: u32,
    ) {
        let slot = key.kind_slot();
        let by_kind = &self.layer_cache_misses_by_kind[slot];
        by_kind.set(by_kind.get().saturating_add(1));
        let pixels_by_kind = &self.layer_cache_miss_pixels_by_kind[slot];
        pixels_by_kind.set(
            pixels_by_kind
                .get()
                .saturating_add((width as u64) * (height as u64)),
        );
        self.layer_cache_misses
            .set(self.layer_cache_misses.get().saturating_add(1));
        self.layer_cache_miss_pixels.set(
            self.layer_cache_miss_pixels
                .get()
                .saturating_add((width as u64) * (height as u64)),
        );
    }

    pub(crate) fn record_shadow_shape_cache_hit(&self, composited_pixels: u64) {
        self.shadow_shape_cache_hits
            .set(self.shadow_shape_cache_hits.get().saturating_add(1));
        self.shadow_shape_cache_hit_pixels.set(
            self.shadow_shape_cache_hit_pixels
                .get()
                .saturating_add(composited_pixels),
        );
    }

    pub(crate) fn record_shadow_shape_cache_miss(&self, width: u32, height: u32) {
        self.shadow_shape_cache_misses
            .set(self.shadow_shape_cache_misses.get().saturating_add(1));
        self.shadow_shape_cache_miss_pixels.set(
            self.shadow_shape_cache_miss_pixels
                .get()
                .saturating_add((width as u64) * (height as u64)),
        );
    }

    pub(crate) fn maybe_print_shadow_shape_cache_miss(
        &self,
        bounds: crate::geometry::DevicePixelBounds,
        content_hash: u64,
        blur_radius: f32,
        shape_count: usize,
        clip: Option<Rect>,
    ) {
        if !shadow_cache_diagnostics_enabled() {
            return;
        }

        let count = self.shadow_shape_cache_miss_log_count.get();
        if count >= 16 {
            return;
        }
        self.shadow_shape_cache_miss_log_count.set(count + 1);

        let clip_text = clip.map_or_else(
            || "none".to_string(),
            |clip| {
                format!(
                    "({:.1},{:.1},{:.1},{:.1})",
                    clip.x, clip.y, clip.width, clip.height
                )
            },
        );
        stats_line!(
            "[shadow-cache-miss #{count}] size={}x{} content_hash={content_hash} blur={:.2} viewport_offset=({:.1},{:.1}) shapes={} clip={}",
            bounds.width,
            bounds.height,
            blur_radius,
            bounds.x,
            bounds.y,
            shape_count,
            clip_text,
        );
    }

    pub(crate) fn record_shadow_text_blur_fallback(&self) {
        self.shadow_text_blur_fallbacks
            .set(self.shadow_text_blur_fallbacks.get().saturating_add(1));
    }

    pub(crate) fn add_shape_fill(&self, fill: ShapeFill) {
        self.shape_fill_pixels.set(
            self.shape_fill_pixels
                .get()
                .saturating_add(fill.total().round() as u64),
        );
        let mut by_class = self.shape_fill_pixels_by_class.get();
        for (total, pixels) in by_class.iter_mut().zip(fill.pixels) {
            *total = total.saturating_add(pixels.round() as u64);
        }
        self.shape_fill_pixels_by_class.set(by_class);
        self.shape_vertices
            .set(self.shape_vertices.get().saturating_add(fill.vertices));
    }

    pub(crate) fn bump_shapes(&self) {
        self.shape_passes.set(self.shape_passes.get() + 1);
    }

    pub(crate) fn bump_images(&self) {
        self.image_passes.set(self.image_passes.get() + 1);
    }

    pub(crate) fn add_draw_calls(&self, count: u32) {
        self.draw_calls
            .set(self.draw_calls.get().saturating_add(count));
    }

    pub(crate) fn bump_text(&self) {
        self.text_passes.set(self.text_passes.get() + 1);
    }

    pub(crate) fn record_text_image_cache_hit(&self, width: u32, height: u32) {
        self.text_image_cache_hits
            .set(self.text_image_cache_hits.get().saturating_add(1));
        self.text_image_cache_hit_pixels.set(
            self.text_image_cache_hit_pixels
                .get()
                .saturating_add((width as u64) * (height as u64)),
        );
    }

    pub(crate) fn record_text_image_cache_miss(&self, width: u32, height: u32) {
        let pixels = (width as u64) * (height as u64);
        self.text_image_cache_misses
            .set(self.text_image_cache_misses.get().saturating_add(1));
        self.text_image_cache_miss_pixels.set(
            self.text_image_cache_miss_pixels
                .get()
                .saturating_add(pixels),
        );
        self.text_image_raster_bytes.set(
            self.text_image_raster_bytes
                .get()
                .saturating_add(pixels * 4),
        );
    }

    pub(crate) fn record_text_glyph_atlas_hits(&self, count: u32) {
        self.text_glyph_atlas_hits
            .set(self.text_glyph_atlas_hits.get().saturating_add(count));
    }

    pub(crate) fn record_text_glyph_run_collected(&self) {
        self.text_glyph_runs_collected
            .set(self.text_glyph_runs_collected.get().saturating_add(1));
    }

    pub(crate) fn record_text_glyph_atlas_miss(&self, width: u32, height: u32) {
        self.text_glyph_atlas_misses
            .set(self.text_glyph_atlas_misses.get().saturating_add(1));
        self.text_glyph_atlas_miss_pixels.set(
            self.text_glyph_atlas_miss_pixels
                .get()
                .saturating_add((width as u64) * (height as u64)),
        );
    }

    pub(crate) fn snapshot(&self) -> FrameStatsSnapshot {
        let retained_texture_bytes = self
            .offscreen_pool_bytes
            .get()
            .saturating_add(self.layer_cache_bytes.get());
        FrameStatsSnapshot {
            submits: self.submits.get(),
            encoder_count: self.command_encoder_count.get(),
            submit_count: self.command_submit_count.get(),
            pass_count: self.command_pass_count.get(),
            pass_pixels: self.command_pass_pixels.get(),
            copy_count: self.command_copy_count.get(),
            copy_pixels: self.command_copy_pixels.get(),
            offscreen_acquires: self.offscreen_acquires.get(),
            offscreen_news: self.offscreen_news.get(),
            offscreen_total_bytes: self.offscreen_total_bytes.get(),
            transient_texture_bytes: self
                .offscreen_total_bytes
                .get()
                .saturating_add(self.command_transient_texture_bytes.get()),
            retained_texture_bytes,
            upload_bytes: self.command_upload_bytes.get(),
            upload_writes: self.upload_writes.get(),
            isolated_layer_renders: self.isolated_layer_renders.get(),
            isolated_layer_pixels: self.isolated_layer_pixels.get(),
            layer_cache_hits: self.layer_cache_hits.get(),
            capture_fixup_passes: self.capture_fixup_passes.get(),
            layer_cache_misses: self.layer_cache_misses.get(),
            layer_cache_hit_pixels: self.layer_cache_hit_pixels.get(),
            layer_cache_miss_pixels: self.layer_cache_miss_pixels.get(),
            layer_cache_hits_by_kind: self.layer_cache_hits_by_kind.each_ref().map(Cell::get),
            layer_cache_misses_by_kind: self.layer_cache_misses_by_kind.each_ref().map(Cell::get),
            layer_cache_miss_pixels_by_kind: self
                .layer_cache_miss_pixels_by_kind
                .each_ref()
                .map(Cell::get),
            shadow_shape_cache_hits: self.shadow_shape_cache_hits.get(),
            shadow_shape_cache_misses: self.shadow_shape_cache_misses.get(),
            shadow_shape_cache_hit_pixels: self.shadow_shape_cache_hit_pixels.get(),
            shadow_shape_cache_miss_pixels: self.shadow_shape_cache_miss_pixels.get(),
            shadow_text_blur_fallbacks: self.shadow_text_blur_fallbacks.get(),
            blur_passes: self.blur_passes.get(),
            stages: self.stages.get(),
            backdrop_admissions: self.backdrop_admissions.get(),
            prefix_admissions: self.prefix_admissions.get(),
            substrates: self.substrates.get(),
            composite_passes: self.composite_passes.get(),
            effect_applies: self.effect_applies.get(),
            shader_pixels: self.shader_pixels.get(),
            glass_rasterized_pixels: self.glass_rasterized_pixels.get(),
            blur_pixels: self.blur_pixels.get(),
            shape_fill_pixels: self.shape_fill_pixels.get(),
            shape_fill_pixels_by_class: self.shape_fill_pixels_by_class.get(),
            shape_vertices: self.shape_vertices.get(),
            shape_passes: self.shape_passes.get(),
            shape_pipeline_fallback_draws: self.shape_pipeline_fallback_draws.get(),
            instant_draws: self.instant_draws.get(),
            shape_specialized_draws: self.shape_specialized_draws.get(),
            shape_interior_draws: self.shape_interior_draws.get(),
            depth_passes: self.depth_passes.get(),
            pipelines_created: crate::render::pipelines_created(),
            shader_pipeline_fallback_draws: self.shader_pipeline_fallback_draws.get(),
            shader_specialized_draws: self.shader_specialized_draws.get(),
            placeholder_draws: self.placeholder_draws.get(),
            image_passes: self.image_passes.get(),
            text_passes: self.text_passes.get(),
            draw_calls: self.draw_calls.get(),
            damage: self.damage.get(),
            repainted_pixels: self.repainted_pixels.get(),
            text_image_cache_hits: self.text_image_cache_hits.get(),
            text_image_cache_misses: self.text_image_cache_misses.get(),
            text_image_cache_hit_pixels: self.text_image_cache_hit_pixels.get(),
            text_image_cache_miss_pixels: self.text_image_cache_miss_pixels.get(),
            text_image_raster_bytes: self.text_image_raster_bytes.get(),
            text_glyph_atlas_hits: self.text_glyph_atlas_hits.get(),
            text_glyph_atlas_misses: self.text_glyph_atlas_misses.get(),
            text_glyph_atlas_miss_pixels: self.text_glyph_atlas_miss_pixels.get(),
            text_glyph_runs_collected: self.text_glyph_runs_collected.get(),
            offscreen_pool_size: self.offscreen_pool_size.get(),
            offscreen_pool_bytes: self.offscreen_pool_bytes.get(),
            text_pool_size: self.text_pool_size.get(),
            layer_cache_size: self.layer_cache_size.get(),
            layer_cache_bytes: self.layer_cache_bytes.get(),
            image_cache_size: self.image_cache_size.get(),
            top_isolated_layers: *self.top_isolated_layers.borrow(),
            top_isolated_layer_count: self.top_isolated_layer_count.get(),
        }
    }

    pub(crate) fn reset(&self) {
        self.submits.set(0);
        self.command_encoder_count.set(0);
        self.command_submit_count.set(0);
        self.command_pass_count.set(0);
        self.command_pass_pixels.set(0);
        self.command_copy_count.set(0);
        self.command_copy_pixels.set(0);
        self.command_transient_texture_bytes.set(0);
        self.command_upload_bytes.set(0);
        self.upload_writes.set(0);
        self.offscreen_acquires.set(0);
        self.offscreen_news.set(0);
        self.offscreen_total_bytes.set(0);
        self.isolated_layer_renders.set(0);
        self.isolated_layer_pixels.set(0);
        self.layer_cache_hits.set(0);
        self.capture_fixup_passes.set(0);
        self.layer_cache_misses.set(0);
        self.layer_cache_hit_pixels.set(0);
        self.layer_cache_miss_pixels.set(0);
        for slot in 0..LAYER_RASTER_CACHE_KIND_COUNT {
            self.layer_cache_hits_by_kind[slot].set(0);
            self.layer_cache_misses_by_kind[slot].set(0);
            self.layer_cache_miss_pixels_by_kind[slot].set(0);
        }
        self.shadow_shape_cache_hits.set(0);
        self.shadow_shape_cache_misses.set(0);
        self.shadow_shape_cache_hit_pixels.set(0);
        self.shadow_shape_cache_miss_pixels.set(0);
        self.shadow_text_blur_fallbacks.set(0);
        self.blur_passes.set(0);
        self.stages.set(0);
        self.backdrop_admissions.set(0);
        self.prefix_admissions.set(0);
        self.substrates.set(0);
        self.composite_passes.set(0);
        self.effect_applies.set(0);
        self.shader_pixels.set(0);
        self.glass_rasterized_pixels.set(0);
        self.blur_pixels.set(0);
        self.shape_fill_pixels.set(0);
        self.shape_fill_pixels_by_class.set([0; ShapeFill::CLASSES]);
        self.shape_vertices.set(0);
        self.shape_passes.set(0);
        self.shape_pipeline_fallback_draws.set(0);
        self.instant_draws.set(0);
        self.shape_specialized_draws.set(0);
        self.shape_interior_draws.set(0);
        self.depth_passes.set(0);
        self.shader_pipeline_fallback_draws.set(0);
        self.shader_specialized_draws.set(0);
        self.placeholder_draws.set(0);
        self.image_passes.set(0);
        self.text_passes.set(0);
        self.draw_calls.set(0);
        self.damage.set(FrameDamage::default());
        self.repainted_pixels.set(0);
        self.text_image_cache_hits.set(0);
        self.text_image_cache_misses.set(0);
        self.text_image_cache_hit_pixels.set(0);
        self.text_image_cache_miss_pixels.set(0);
        self.text_image_raster_bytes.set(0);
        self.text_glyph_atlas_hits.set(0);
        self.text_glyph_atlas_misses.set(0);
        self.text_glyph_atlas_miss_pixels.set(0);
        self.text_glyph_runs_collected.set(0);
        *self.top_isolated_layers.borrow_mut() = [None; TOP_ISOLATED_LAYER_LIMIT];
        self.top_isolated_layer_count.set(0);
        self.shadow_shape_cache_miss_log_count.set(0);
    }

    pub(crate) fn maybe_print_snapshot(
        &self,
        snapshot: FrameStatsSnapshot,
        frame_count: &mut u64,
        enabled: bool,
    ) {
        if !enabled {
            return;
        }
        *frame_count += 1;
        if (*frame_count).is_multiple_of(60) {
            snapshot.print(*frame_count);
        }
    }

    fn record_top_isolated_layer(&self, layer: IsolatedLayerStat) {
        let mut top_layers = self.top_isolated_layers.borrow_mut();
        let len = self.top_isolated_layer_count.get();
        let insert_at = top_layers[..len]
            .iter()
            .enumerate()
            .find_map(|(index, existing)| {
                existing
                    .filter(|existing| layer.pixel_area() > existing.pixel_area())
                    .map(|_| index)
            })
            .unwrap_or(len);

        if insert_at >= TOP_ISOLATED_LAYER_LIMIT {
            return;
        }

        let new_len = if len < TOP_ISOLATED_LAYER_LIMIT {
            len + 1
        } else {
            TOP_ISOLATED_LAYER_LIMIT
        };

        let mut index = new_len.saturating_sub(1);
        while index > insert_at {
            top_layers[index] = top_layers[index - 1];
            index -= 1;
        }
        top_layers[insert_at] = Some(layer);
        self.top_isolated_layer_count.set(new_len);
    }
}
