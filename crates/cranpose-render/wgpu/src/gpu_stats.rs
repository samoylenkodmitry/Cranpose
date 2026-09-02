use std::cell::{Cell, RefCell};

use cranpose_core::NodeId;
use cranpose_render_common::raster_cache::{
    LAYER_RASTER_CACHE_KIND_COUNT, LAYER_RASTER_CACHE_KIND_LABELS, LayerRasterCacheKey,
};
use cranpose_ui_graphics::Rect;

use crate::{
    frame_graph::FrameCommandStats,
    surface_requirements::{SurfaceRequirement, SurfaceRequirementSet},
};

const TOP_ISOLATED_LAYER_LIMIT: usize = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayerSurfaceReasons {
    pub explicit_offscreen: bool,
    pub effect: bool,
    pub backdrop: bool,
    pub group_opacity: bool,
    pub blend_mode: bool,
    pub shape_clip: bool,
    pub immediate_shadow: bool,
    pub text_local_surface: bool,
    pub motion_stable_capture: bool,
    pub mixed_direct_content: bool,
    pub non_translation_transform: bool,
    pub pixel_stable_composite: bool,
}

impl LayerSurfaceReasons {
    pub fn has_any(self) -> bool {
        self.explicit_offscreen
            || self.effect
            || self.backdrop
            || self.group_opacity
            || self.blend_mode
            || self.shape_clip
            || self.text_local_surface
            || self.motion_stable_capture
            || self.non_translation_transform
    }

    pub fn labels(self) -> impl Iterator<Item = &'static str> {
        let mut labels = [None; 12];
        let mut len = 0usize;

        if self.explicit_offscreen {
            labels[len] = Some("explicit_offscreen");
            len += 1;
        }
        if self.effect {
            labels[len] = Some("effect");
            len += 1;
        }
        if self.backdrop {
            labels[len] = Some("backdrop");
            len += 1;
        }
        if self.group_opacity {
            labels[len] = Some("group_opacity");
            len += 1;
        }
        if self.blend_mode {
            labels[len] = Some("blend_mode");
            len += 1;
        }
        if self.shape_clip {
            labels[len] = Some("shape_clip");
            len += 1;
        }
        if self.immediate_shadow {
            labels[len] = Some("immediate_shadow");
            len += 1;
        }
        if self.text_local_surface {
            labels[len] = Some("text_local_surface");
            len += 1;
        }
        if self.motion_stable_capture {
            labels[len] = Some("motion_stable_capture");
            len += 1;
        }
        if self.mixed_direct_content {
            labels[len] = Some("mixed_direct_content");
            len += 1;
        }
        if self.non_translation_transform {
            labels[len] = Some("non_translation_transform");
            len += 1;
        }
        if self.pixel_stable_composite {
            labels[len] = Some("pixel_stable_composite");
            len += 1;
        }

        labels.into_iter().flatten().take(len)
    }

    pub fn display(self) -> String {
        let mut joined = String::new();
        for (index, label) in self.labels().enumerate() {
            if index > 0 {
                joined.push('+');
            }
            joined.push_str(label);
        }
        if joined.is_empty() {
            joined.push_str("none");
        }
        joined
    }

    pub fn has_renderer_forced_surface(self) -> bool {
        self.text_local_surface || self.non_translation_transform
    }
}

impl From<SurfaceRequirementSet> for LayerSurfaceReasons {
    fn from(requirements: SurfaceRequirementSet) -> Self {
        Self {
            explicit_offscreen: requirements.contains(SurfaceRequirement::ExplicitOffscreen),
            effect: requirements.contains(SurfaceRequirement::RenderEffect),
            backdrop: requirements.contains(SurfaceRequirement::Backdrop),
            group_opacity: requirements.contains(SurfaceRequirement::GroupOpacity),
            blend_mode: requirements.contains(SurfaceRequirement::BlendMode),
            shape_clip: requirements.contains(SurfaceRequirement::ShapeClip),
            immediate_shadow: requirements.contains(SurfaceRequirement::ImmediateShadow),
            text_local_surface: requirements.contains(SurfaceRequirement::TextMaterialMask),
            motion_stable_capture: requirements.contains(SurfaceRequirement::MotionStableCapture),
            mixed_direct_content: requirements.contains(SurfaceRequirement::MixedDirectContent),
            non_translation_transform: requirements
                .contains(SurfaceRequirement::NonTranslationTransform),
            pixel_stable_composite: requirements.contains(SurfaceRequirement::PixelStableComposite),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IsolatedLayerStat {
    pub node_id: Option<NodeId>,
    pub logical_rect: Rect,
    pub width: u32,
    pub height: u32,
    pub reasons: LayerSurfaceReasons,
}

impl IsolatedLayerStat {
    fn pixel_area(self) -> u64 {
        (self.width as u64) * (self.height as u64)
    }
}

impl Default for IsolatedLayerStat {
    fn default() -> Self {
        Self {
            node_id: None,
            logical_rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            },
            width: 0,
            height: 0,
            reasons: LayerSurfaceReasons::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameStatsSnapshot {
    pub submits: u32,
    pub encoder_count: u32,
    pub submit_count: u32,
    pub pass_count: u32,
    pub offscreen_acquires: u32,
    pub offscreen_news: u32,
    pub offscreen_total_bytes: u64,
    pub transient_texture_bytes: u64,
    pub retained_texture_bytes: u64,
    pub upload_bytes: u64,
    pub isolated_layer_renders: u32,
    pub isolated_layer_pixels: u64,
    pub layer_cache_hits: u32,
    pub layer_cache_misses: u32,
    pub layer_cache_evictions: u32,
    pub layer_cache_hit_pixels: u64,
    pub layer_cache_miss_pixels: u64,
    /// Hits per key kind, indexed by `LayerRasterCacheKey::kind_slot`.
    pub layer_cache_hits_by_kind: [u32; LAYER_RASTER_CACHE_KIND_COUNT],
    /// Stores per key kind, indexed by `LayerRasterCacheKey::kind_slot`.
    pub layer_cache_misses_by_kind: [u32; LAYER_RASTER_CACHE_KIND_COUNT],
    /// Stored pixels per key kind, indexed by `LayerRasterCacheKey::kind_slot`.
    pub layer_cache_miss_pixels_by_kind: [u64; LAYER_RASTER_CACHE_KIND_COUNT],
    pub shadow_shape_cache_hits: u32,
    pub shadow_shape_cache_misses: u32,
    pub shadow_shape_cache_hit_pixels: u64,
    pub shadow_shape_cache_miss_pixels: u64,
    /// Converted shadows whose caster occluded every visible coverage pixel
    /// this frame: they composite nothing and their draw item is dropped.
    pub shadow_fully_occluded_composites: u32,
    pub shadow_text_blur_fallbacks: u32,
    pub blur_passes: u32,
    pub composite_passes: u32,
    pub effect_applies: u32,
    pub shape_passes: u32,
    pub image_passes: u32,
    pub text_passes: u32,
    /// Shape, image and glyph `draw_indexed` calls recorded this frame. The
    /// `*_passes` counters above count *batches*, so a single image batch
    /// reports `image_passes=1` however many images it draws; this counts what
    /// the driver actually sees. Composite and effect quads are not included —
    /// they are one draw each and already counted by `composite_passes`,
    /// `blur_passes` and `effect_applies`.
    pub draw_calls: u32,
    pub text_image_cache_hits: u32,
    pub text_image_cache_misses: u32,
    pub text_image_cache_hit_pixels: u64,
    pub text_image_cache_miss_pixels: u64,
    pub text_image_raster_bytes: u64,
    pub text_glyph_atlas_hits: u32,
    pub text_glyph_atlas_misses: u32,
    pub text_glyph_atlas_miss_pixels: u64,
    pub offscreen_pool_size: u32,
    pub offscreen_pool_bytes: u64,
    pub text_pool_size: u32,
    pub layer_cache_size: u32,
    pub layer_cache_bytes: u64,
    pub image_cache_size: u32,
    pub text_cache_size: u32,
    pub top_isolated_layers: [Option<IsolatedLayerStat>; TOP_ISOLATED_LAYER_LIMIT],
    pub top_isolated_layer_count: usize,
}

impl FrameStatsSnapshot {
    pub(crate) fn with_command_stats_added(mut self, stats: FrameCommandStats) -> Self {
        self.submits = self.submits.saturating_add(stats.submit_count);
        self.encoder_count = self.encoder_count.saturating_add(stats.encoder_count);
        self.submit_count = self.submit_count.saturating_add(stats.submit_count);
        self.pass_count = self.pass_count.saturating_add(stats.pass_count);
        self.transient_texture_bytes = self
            .transient_texture_bytes
            .saturating_add(stats.transient_texture_bytes);
        self.retained_texture_bytes = self
            .retained_texture_bytes
            .max(stats.retained_texture_bytes);
        self.upload_bytes = self.upload_bytes.saturating_add(stats.upload_bytes);
        self
    }

    pub fn top_isolated_layers(self) -> impl Iterator<Item = IsolatedLayerStat> {
        self.top_isolated_layers
            .into_iter()
            .flatten()
            .take(self.top_isolated_layer_count)
    }

    fn layer_cache_hit_rate(self) -> f64 {
        let total = self.layer_cache_hits + self.layer_cache_misses;
        if total > 0 {
            (self.layer_cache_hits as f64 / total as f64) * 100.0
        } else {
            0.0
        }
    }

    fn miss_pixels_by_kind_display(&self) -> String {
        LAYER_RASTER_CACHE_KIND_LABELS
            .iter()
            .zip(self.layer_cache_miss_pixels_by_kind.iter())
            .filter(|(_, pixels)| **pixels > 0)
            .map(|(label, pixels)| format!("{label}={:.2}MP", *pixels as f64 / 1_000_000.0))
            .collect::<Vec<_>>()
            .join(",")
    }

    fn print(self, frame_count: u64) {
        let mb = self.offscreen_total_bytes as f64 / (1024.0 * 1024.0);
        let upload_mb = self.upload_bytes as f64 / (1024.0 * 1024.0);
        let retained_mb = self.retained_texture_bytes as f64 / (1024.0 * 1024.0);
        let pool_mb = self.offscreen_pool_bytes as f64 / (1024.0 * 1024.0);
        let layer_cache_hit_mpx = self.layer_cache_hit_pixels as f64 / 1_000_000.0;
        let layer_cache_miss_mpx = self.layer_cache_miss_pixels as f64 / 1_000_000.0;
        let shadow_cache_hit_mpx = self.shadow_shape_cache_hit_pixels as f64 / 1_000_000.0;
        let shadow_cache_miss_mpx = self.shadow_shape_cache_miss_pixels as f64 / 1_000_000.0;
        let layer_cache_mb = self.layer_cache_bytes as f64 / (1024.0 * 1024.0);
        let isolated_layer_mpx = self.isolated_layer_pixels as f64 / 1_000_000.0;
        eprintln!(
            "[GPU f#{}] encoders={} submits={} passes={} | offscreen: acq={} new={} {:.1}MB pool={}({:.1}MB) retained={:.1}MB | \
             uploads={:.2}MB | \
             isolated_layers={} area={:.2}MP | \
             layer_cache: hit={} miss={} {:.1}% evict={} hit_px={:.2}MP miss_px={:.2}MP size={}({:.1}MB) miss_px_by_kind={} | \
             shadow_cache: shape_hit={} shape_miss={} hit_px={:.2}MP miss_px={:.2}MP text_blur_fallback={} | \
             blur={} composite={} effect={} | shape={} image={} text={} draws={} | \
             text_img_cache: hit={} miss={} hit_px={:.2}MP miss_px={:.2}MP raster={:.2}MB | \
             text_glyph_atlas: hit={} miss={} miss_px={:.2}MP | \
             caches: text_pool={} img={} txt={}",
            frame_count,
            self.encoder_count,
            self.submit_count,
            self.pass_count,
            self.offscreen_acquires,
            self.offscreen_news,
            mb,
            self.offscreen_pool_size,
            pool_mb,
            retained_mb,
            upload_mb,
            self.isolated_layer_renders,
            isolated_layer_mpx,
            self.layer_cache_hits,
            self.layer_cache_misses,
            self.layer_cache_hit_rate(),
            self.layer_cache_evictions,
            layer_cache_hit_mpx,
            layer_cache_miss_mpx,
            self.layer_cache_size,
            layer_cache_mb,
            self.miss_pixels_by_kind_display(),
            self.shadow_shape_cache_hits,
            self.shadow_shape_cache_misses,
            shadow_cache_hit_mpx,
            shadow_cache_miss_mpx,
            self.shadow_text_blur_fallbacks,
            self.blur_passes,
            self.composite_passes,
            self.effect_applies,
            self.shape_passes,
            self.image_passes,
            self.text_passes,
            self.draw_calls,
            self.text_image_cache_hits,
            self.text_image_cache_misses,
            self.text_image_cache_hit_pixels as f64 / 1_000_000.0,
            self.text_image_cache_miss_pixels as f64 / 1_000_000.0,
            self.text_image_raster_bytes as f64 / (1024.0 * 1024.0),
            self.text_glyph_atlas_hits,
            self.text_glyph_atlas_misses,
            self.text_glyph_atlas_miss_pixels as f64 / 1_000_000.0,
            self.text_pool_size,
            self.image_cache_size,
            self.text_cache_size,
        );
        for (index, layer) in self.top_isolated_layers().enumerate() {
            eprintln!(
                "  [isolated #{index}] node={:?} rect=({:.1},{:.1},{:.1},{:.1}) target={}x{} reasons={}",
                layer.node_id,
                layer.logical_rect.x,
                layer.logical_rect.y,
                layer.logical_rect.width,
                layer.logical_rect.height,
                layer.width,
                layer.height,
                layer.reasons.display(),
            );
        }
    }
}

#[derive(Default)]
pub(crate) struct FrameStats {
    pub submits: Cell<u32>,
    pub command_encoder_count: Cell<u32>,
    pub command_submit_count: Cell<u32>,
    pub command_pass_count: Cell<u32>,
    pub command_transient_texture_bytes: Cell<u64>,
    pub command_retained_texture_bytes: Cell<u64>,
    pub command_upload_bytes: Cell<u64>,
    pub offscreen_acquires: Cell<u32>,
    pub offscreen_news: Cell<u32>,
    pub offscreen_total_bytes: Cell<u64>,
    pub upload_bytes: Cell<u64>,
    pub isolated_layer_renders: Cell<u32>,
    pub isolated_layer_pixels: Cell<u64>,
    pub layer_cache_hits: Cell<u32>,
    pub layer_cache_misses: Cell<u32>,
    pub layer_cache_evictions: Cell<u32>,
    pub layer_cache_hit_pixels: Cell<u64>,
    pub layer_cache_miss_pixels: Cell<u64>,
    pub layer_cache_hits_by_kind: [Cell<u32>; LAYER_RASTER_CACHE_KIND_COUNT],
    pub layer_cache_misses_by_kind: [Cell<u32>; LAYER_RASTER_CACHE_KIND_COUNT],
    pub layer_cache_miss_pixels_by_kind: [Cell<u64>; LAYER_RASTER_CACHE_KIND_COUNT],
    pub shadow_shape_cache_hits: Cell<u32>,
    pub shadow_shape_cache_misses: Cell<u32>,
    pub shadow_shape_cache_hit_pixels: Cell<u64>,
    pub shadow_shape_cache_miss_pixels: Cell<u64>,
    pub shadow_fully_occluded_composites: Cell<u32>,
    pub shadow_text_blur_fallbacks: Cell<u32>,
    pub blur_passes: Cell<u32>,
    pub composite_passes: Cell<u32>,
    pub effect_applies: Cell<u32>,
    pub shape_passes: Cell<u32>,
    pub image_passes: Cell<u32>,
    pub text_passes: Cell<u32>,
    pub draw_calls: Cell<u32>,
    pub text_image_cache_hits: Cell<u32>,
    pub text_image_cache_misses: Cell<u32>,
    pub text_image_cache_hit_pixels: Cell<u64>,
    pub text_image_cache_miss_pixels: Cell<u64>,
    pub text_image_raster_bytes: Cell<u64>,
    pub text_glyph_atlas_hits: Cell<u32>,
    pub text_glyph_atlas_misses: Cell<u32>,
    pub text_glyph_atlas_miss_pixels: Cell<u64>,
    pub offscreen_pool_size: Cell<u32>,
    pub offscreen_pool_bytes: Cell<u64>,
    pub text_pool_size: Cell<u32>,
    pub layer_cache_size: Cell<u32>,
    pub layer_cache_bytes: Cell<u64>,
    pub image_cache_size: Cell<u32>,
    pub text_cache_size: Cell<u32>,
    top_isolated_layers: RefCell<[Option<IsolatedLayerStat>; TOP_ISOLATED_LAYER_LIMIT]>,
    top_isolated_layer_count: Cell<usize>,
    shadow_shape_cache_miss_log_count: Cell<u32>,
    #[cfg(debug_assertions)]
    missed_layer_cache_keys: RefCell<Vec<LayerRasterCacheKey>>,
}

impl FrameStats {
    pub fn record_command_stats(&self, stats: FrameCommandStats) {
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
        self.command_transient_texture_bytes.set(
            self.command_transient_texture_bytes
                .get()
                .saturating_add(stats.transient_texture_bytes),
        );
        self.command_retained_texture_bytes.set(
            self.command_retained_texture_bytes
                .get()
                .max(stats.retained_texture_bytes),
        );
        self.command_upload_bytes.set(
            self.command_upload_bytes
                .get()
                .saturating_add(stats.upload_bytes),
        );
    }

    pub fn record_offscreen_acquire(
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

    pub fn record_upload_bytes(&self, bytes: u64) {
        self.upload_bytes
            .set(self.upload_bytes.get().saturating_add(bytes));
    }

    pub fn record_isolated_layer_render(
        &self,
        width: u32,
        height: u32,
        node_id: Option<NodeId>,
        logical_rect: Rect,
        reasons: LayerSurfaceReasons,
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
            reasons,
        });
    }

    pub fn record_layer_cache_hit(&self, key: &LayerRasterCacheKey, width: u32, height: u32) {
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

    pub fn record_layer_cache_miss(&self, key: &LayerRasterCacheKey, width: u32, height: u32) {
        #[cfg(debug_assertions)]
        self.missed_layer_cache_keys.borrow_mut().push(*key);
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

    #[cfg(debug_assertions)]
    pub fn take_missed_layer_cache_keys(&self) -> Vec<LayerRasterCacheKey> {
        self.missed_layer_cache_keys.take()
    }

    pub fn record_layer_cache_eviction(&self) {
        self.layer_cache_evictions
            .set(self.layer_cache_evictions.get().saturating_add(1));
    }

    pub fn record_shadow_shape_cache_hit(&self, composited_pixels: u64) {
        self.shadow_shape_cache_hits
            .set(self.shadow_shape_cache_hits.get().saturating_add(1));
        self.shadow_shape_cache_hit_pixels.set(
            self.shadow_shape_cache_hit_pixels
                .get()
                .saturating_add(composited_pixels),
        );
    }

    pub fn record_shadow_fully_occluded(&self) {
        self.shadow_fully_occluded_composites.set(
            self.shadow_fully_occluded_composites
                .get()
                .saturating_add(1),
        );
    }

    pub fn record_shadow_shape_cache_miss(&self, width: u32, height: u32) {
        self.shadow_shape_cache_misses
            .set(self.shadow_shape_cache_misses.get().saturating_add(1));
        self.shadow_shape_cache_miss_pixels.set(
            self.shadow_shape_cache_miss_pixels
                .get()
                .saturating_add((width as u64) * (height as u64)),
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn maybe_print_shadow_shape_cache_miss(
        &self,
        width: u32,
        height: u32,
        content_hash: u64,
        blur_radius: f32,
        viewport_offset: [f32; 2],
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

        let clip_text = clip
            .map(|clip| {
                format!(
                    "({:.1},{:.1},{:.1},{:.1})",
                    clip.x, clip.y, clip.width, clip.height
                )
            })
            .unwrap_or_else(|| "none".to_string());
        eprintln!(
            "[shadow-cache-miss #{count}] size={}x{} content_hash={content_hash} blur={:.2} viewport_offset=({:.1},{:.1}) shapes={} clip={}",
            width,
            height,
            blur_radius,
            viewport_offset[0],
            viewport_offset[1],
            shape_count,
            clip_text,
        );
    }

    pub fn record_shadow_text_blur_fallback(&self) {
        self.shadow_text_blur_fallbacks
            .set(self.shadow_text_blur_fallbacks.get().saturating_add(1));
    }

    pub fn bump_shapes(&self) {
        self.shape_passes.set(self.shape_passes.get() + 1);
    }

    pub fn bump_images(&self) {
        self.image_passes.set(self.image_passes.get() + 1);
    }

    pub fn add_draw_calls(&self, count: u32) {
        self.draw_calls
            .set(self.draw_calls.get().saturating_add(count));
    }

    pub fn bump_text(&self) {
        self.text_passes.set(self.text_passes.get() + 1);
    }

    pub fn record_text_image_cache_hit(&self, width: u32, height: u32) {
        self.text_image_cache_hits
            .set(self.text_image_cache_hits.get().saturating_add(1));
        self.text_image_cache_hit_pixels.set(
            self.text_image_cache_hit_pixels
                .get()
                .saturating_add((width as u64) * (height as u64)),
        );
    }

    pub fn record_text_image_cache_miss(&self, width: u32, height: u32) {
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

    pub fn record_text_glyph_atlas_hit(&self) {
        self.text_glyph_atlas_hits
            .set(self.text_glyph_atlas_hits.get().saturating_add(1));
    }

    pub fn record_text_glyph_atlas_miss(&self, width: u32, height: u32) {
        self.text_glyph_atlas_misses
            .set(self.text_glyph_atlas_misses.get().saturating_add(1));
        self.text_glyph_atlas_miss_pixels.set(
            self.text_glyph_atlas_miss_pixels
                .get()
                .saturating_add((width as u64) * (height as u64)),
        );
    }

    pub fn snapshot(&self) -> FrameStatsSnapshot {
        let retained_texture_bytes = self
            .offscreen_pool_bytes
            .get()
            .saturating_add(self.layer_cache_bytes.get());
        FrameStatsSnapshot {
            submits: self.submits.get(),
            encoder_count: self.command_encoder_count.get(),
            submit_count: self.command_submit_count.get(),
            pass_count: self.command_pass_count.get(),
            offscreen_acquires: self.offscreen_acquires.get(),
            offscreen_news: self.offscreen_news.get(),
            offscreen_total_bytes: self.offscreen_total_bytes.get(),
            transient_texture_bytes: self
                .offscreen_total_bytes
                .get()
                .saturating_add(self.command_transient_texture_bytes.get()),
            retained_texture_bytes: retained_texture_bytes
                .saturating_add(self.command_retained_texture_bytes.get()),
            upload_bytes: self
                .upload_bytes
                .get()
                .saturating_add(self.command_upload_bytes.get()),
            isolated_layer_renders: self.isolated_layer_renders.get(),
            isolated_layer_pixels: self.isolated_layer_pixels.get(),
            layer_cache_hits: self.layer_cache_hits.get(),
            layer_cache_misses: self.layer_cache_misses.get(),
            layer_cache_evictions: self.layer_cache_evictions.get(),
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
            shadow_fully_occluded_composites: self.shadow_fully_occluded_composites.get(),
            shadow_text_blur_fallbacks: self.shadow_text_blur_fallbacks.get(),
            blur_passes: self.blur_passes.get(),
            composite_passes: self.composite_passes.get(),
            effect_applies: self.effect_applies.get(),
            shape_passes: self.shape_passes.get(),
            image_passes: self.image_passes.get(),
            text_passes: self.text_passes.get(),
            draw_calls: self.draw_calls.get(),
            text_image_cache_hits: self.text_image_cache_hits.get(),
            text_image_cache_misses: self.text_image_cache_misses.get(),
            text_image_cache_hit_pixels: self.text_image_cache_hit_pixels.get(),
            text_image_cache_miss_pixels: self.text_image_cache_miss_pixels.get(),
            text_image_raster_bytes: self.text_image_raster_bytes.get(),
            text_glyph_atlas_hits: self.text_glyph_atlas_hits.get(),
            text_glyph_atlas_misses: self.text_glyph_atlas_misses.get(),
            text_glyph_atlas_miss_pixels: self.text_glyph_atlas_miss_pixels.get(),
            offscreen_pool_size: self.offscreen_pool_size.get(),
            offscreen_pool_bytes: self.offscreen_pool_bytes.get(),
            text_pool_size: self.text_pool_size.get(),
            layer_cache_size: self.layer_cache_size.get(),
            layer_cache_bytes: self.layer_cache_bytes.get(),
            image_cache_size: self.image_cache_size.get(),
            text_cache_size: self.text_cache_size.get(),
            top_isolated_layers: *self.top_isolated_layers.borrow(),
            top_isolated_layer_count: self.top_isolated_layer_count.get(),
        }
    }

    pub fn reset(&self) {
        self.submits.set(0);
        self.command_encoder_count.set(0);
        self.command_submit_count.set(0);
        self.command_pass_count.set(0);
        self.command_transient_texture_bytes.set(0);
        self.command_retained_texture_bytes.set(0);
        self.command_upload_bytes.set(0);
        self.offscreen_acquires.set(0);
        self.offscreen_news.set(0);
        self.offscreen_total_bytes.set(0);
        self.upload_bytes.set(0);
        self.isolated_layer_renders.set(0);
        self.isolated_layer_pixels.set(0);
        self.layer_cache_hits.set(0);
        self.layer_cache_misses.set(0);
        self.layer_cache_evictions.set(0);
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
        self.shadow_fully_occluded_composites.set(0);
        self.shadow_text_blur_fallbacks.set(0);
        self.blur_passes.set(0);
        self.composite_passes.set(0);
        self.effect_applies.set(0);
        self.shape_passes.set(0);
        self.image_passes.set(0);
        self.text_passes.set(0);
        self.draw_calls.set(0);
        self.text_image_cache_hits.set(0);
        self.text_image_cache_misses.set(0);
        self.text_image_cache_hit_pixels.set(0);
        self.text_image_cache_miss_pixels.set(0);
        self.text_image_raster_bytes.set(0);
        self.text_glyph_atlas_hits.set(0);
        self.text_glyph_atlas_misses.set(0);
        self.text_glyph_atlas_miss_pixels.set(0);
        *self.top_isolated_layers.borrow_mut() = [None; TOP_ISOLATED_LAYER_LIMIT];
        self.top_isolated_layer_count.set(0);
        self.shadow_shape_cache_miss_log_count.set(0);
    }

    pub fn maybe_print_snapshot(
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
        if !layer.reasons.has_any() {
            return;
        }

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

pub(crate) fn gpu_stats_enabled() -> bool {
    crate::debug_toggles::debug_toggle("CRANPOSE_GPU_STATS")
        .map(|v| matches!(v.as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

pub(crate) fn print_gpu_memory_report(device: &wgpu::Device, frame_count: u64) {
    let Some(report) = device.generate_allocator_report() else {
        return;
    };

    const MB: f64 = 1024.0 * 1024.0;
    let mut blocks = String::new();
    for block in &report.blocks {
        if !blocks.is_empty() {
            blocks.push('+');
        }
        blocks.push_str(&format!("{:.1}", block.size as f64 / MB));
    }

    eprintln!(
        "[GPU-MEM f#{}] reserved={:.1}MB allocated={:.1}MB blocks={}[{}MB] allocations={} | largest={:.6?}",
        frame_count,
        report.total_reserved_bytes as f64 / MB,
        report.total_allocated_bytes as f64 / MB,
        report.blocks.len(),
        blocks,
        report.allocations.len(),
        report,
    );
}

fn shadow_cache_diagnostics_enabled() -> bool {
    crate::debug_toggles::debug_toggle("CRANPOSE_GPU_SHADOW_CACHE_DIAG")
        .map(|v| matches!(v.as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_layer_cache_key() -> LayerRasterCacheKey {
        LayerRasterCacheKey::source_content(
            None,
            0,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 5.0,
                height: 6.0,
            },
            (5, 6),
            cranpose_render_common::raster_cache::ScaleBucket::from_scale(1.0),
        )
    }

    #[test]
    fn layer_cache_counters_accumulate_and_reset() {
        let stats = FrameStats::default();
        stats.record_upload_bytes(512);
        stats.record_command_stats(FrameCommandStats {
            encoder_count: 1,
            submit_count: 1,
            pass_count: 2,
            transient_texture_bytes: 256,
            retained_texture_bytes: 128,
            upload_bytes: 64,
        });
        stats.bump_shapes();
        stats.blur_passes.set(1);
        stats.offscreen_total_bytes.set(1024);
        stats.offscreen_pool_bytes.set(2048);
        stats.record_layer_cache_hit(&test_layer_cache_key(), 10, 20);
        stats.record_layer_cache_hit(&test_layer_cache_key(), 3, 4);
        stats.record_layer_cache_miss(&test_layer_cache_key(), 5, 6);
        stats.record_layer_cache_eviction();
        stats.record_shadow_shape_cache_hit(72);
        stats.record_shadow_shape_cache_miss(10, 11);
        stats.record_shadow_text_blur_fallback();
        stats.record_text_image_cache_hit(13, 17);
        stats.record_text_image_cache_miss(19, 23);

        assert_eq!(stats.layer_cache_hits.get(), 2);
        assert_eq!(stats.layer_cache_misses.get(), 1);
        assert_eq!(stats.layer_cache_evictions.get(), 1);
        assert_eq!(stats.layer_cache_hit_pixels.get(), 212);
        assert_eq!(stats.layer_cache_miss_pixels.get(), 30);
        assert_eq!(stats.shadow_shape_cache_hits.get(), 1);
        assert_eq!(stats.shadow_shape_cache_misses.get(), 1);
        assert_eq!(stats.shadow_shape_cache_hit_pixels.get(), 72);
        assert_eq!(stats.shadow_shape_cache_miss_pixels.get(), 110);
        assert_eq!(stats.shadow_text_blur_fallbacks.get(), 1);

        stats.record_isolated_layer_render(
            7,
            8,
            Some(9),
            Rect {
                x: 2.0,
                y: 3.0,
                width: 4.0,
                height: 5.0,
            },
            LayerSurfaceReasons {
                text_local_surface: true,
                ..LayerSurfaceReasons::default()
            },
        );
        let snapshot = stats.snapshot();

        assert_eq!(snapshot.isolated_layer_renders, 1);
        assert_eq!(snapshot.isolated_layer_pixels, 56);
        assert_eq!(snapshot.upload_bytes, 576);
        assert_eq!(snapshot.encoder_count, 1);
        assert_eq!(snapshot.submit_count, 1);
        assert_eq!(snapshot.pass_count, 2);
        assert_eq!(snapshot.transient_texture_bytes, 1280);
        assert_eq!(snapshot.retained_texture_bytes, 2176);
        assert_eq!(snapshot.layer_cache_hits, 2);
        assert_eq!(snapshot.layer_cache_misses, 1);
        assert_eq!(snapshot.shadow_shape_cache_hits, 1);
        assert_eq!(snapshot.shadow_shape_cache_misses, 1);
        assert_eq!(snapshot.shadow_shape_cache_hit_pixels, 72);
        assert_eq!(snapshot.shadow_shape_cache_miss_pixels, 110);
        assert_eq!(snapshot.shadow_text_blur_fallbacks, 1);
        assert_eq!(snapshot.text_image_cache_hits, 1);
        assert_eq!(snapshot.text_image_cache_misses, 1);
        assert_eq!(snapshot.text_image_cache_hit_pixels, 221);
        assert_eq!(snapshot.text_image_cache_miss_pixels, 437);
        assert_eq!(snapshot.text_image_raster_bytes, 1748);
        let top_layers = snapshot.top_isolated_layers().collect::<Vec<_>>();
        assert_eq!(top_layers.len(), 1);
        assert_eq!(top_layers[0].node_id, Some(9));
        assert_eq!(stats.layer_cache_hits.get(), 2);
        assert_eq!(stats.layer_cache_misses.get(), 1);

        stats.reset();

        assert_eq!(stats.layer_cache_hits.get(), 0);
        assert_eq!(stats.layer_cache_misses.get(), 0);
        assert_eq!(stats.layer_cache_evictions.get(), 0);
        assert_eq!(stats.layer_cache_hit_pixels.get(), 0);
        assert_eq!(stats.layer_cache_miss_pixels.get(), 0);
        assert_eq!(stats.shadow_shape_cache_hits.get(), 0);
        assert_eq!(stats.shadow_shape_cache_misses.get(), 0);
        assert_eq!(stats.shadow_shape_cache_hit_pixels.get(), 0);
        assert_eq!(stats.shadow_shape_cache_miss_pixels.get(), 0);
        assert_eq!(stats.shadow_text_blur_fallbacks.get(), 0);
        assert_eq!(stats.text_image_cache_hits.get(), 0);
        assert_eq!(stats.text_image_cache_misses.get(), 0);
        assert_eq!(stats.text_image_cache_hit_pixels.get(), 0);
        assert_eq!(stats.text_image_cache_miss_pixels.get(), 0);
        assert_eq!(stats.text_image_raster_bytes.get(), 0);
        assert_eq!(stats.upload_bytes.get(), 0);
        assert_eq!(stats.isolated_layer_renders.get(), 0);
        assert_eq!(stats.isolated_layer_pixels.get(), 0);
        assert_eq!(stats.top_isolated_layer_count.get(), 0);
    }

    #[test]
    fn command_stats_accumulate_and_reset() {
        let stats = FrameStats::default();

        stats.record_command_stats(FrameCommandStats {
            encoder_count: 2,
            submit_count: 2,
            pass_count: 5,
            transient_texture_bytes: 1024,
            retained_texture_bytes: 2048,
            upload_bytes: 512,
        });
        stats.bump_shapes();

        let snapshot = stats.snapshot();
        assert_eq!(snapshot.submits, 2);
        assert_eq!(snapshot.encoder_count, 2);
        assert_eq!(snapshot.submit_count, 2);
        assert_eq!(snapshot.pass_count, 5);
        assert_eq!(snapshot.transient_texture_bytes, 1024);
        assert_eq!(snapshot.retained_texture_bytes, 2048);
        assert_eq!(snapshot.upload_bytes, 512);

        stats.reset();
        let reset = stats.snapshot();
        assert_eq!(reset.submits, 0);
        assert_eq!(reset.encoder_count, 0);
        assert_eq!(reset.submit_count, 0);
        assert_eq!(reset.pass_count, 0);
        assert_eq!(reset.transient_texture_bytes, 0);
        assert_eq!(reset.retained_texture_bytes, 0);
        assert_eq!(reset.upload_bytes, 0);
    }

    #[test]
    fn snapshot_adds_explicit_readback_command_stats() {
        let stats = FrameStats::default();
        stats.record_command_stats(FrameCommandStats {
            encoder_count: 1,
            submit_count: 1,
            pass_count: 2,
            transient_texture_bytes: 128,
            retained_texture_bytes: 512,
            upload_bytes: 64,
        });
        let snapshot = stats
            .snapshot()
            .with_command_stats_added(FrameCommandStats {
                encoder_count: 1,
                submit_count: 1,
                pass_count: 1,
                transient_texture_bytes: 0,
                retained_texture_bytes: 0,
                upload_bytes: 0,
            });

        assert_eq!(snapshot.submits, 2);
        assert_eq!(snapshot.encoder_count, 2);
        assert_eq!(snapshot.submit_count, 2);
        assert_eq!(snapshot.pass_count, 3);
        assert_eq!(snapshot.transient_texture_bytes, 128);
        assert_eq!(snapshot.retained_texture_bytes, 512);
        assert_eq!(snapshot.upload_bytes, 64);
    }

    #[test]
    fn maybe_print_snapshot_only_advances_frame_counter_when_enabled() {
        let stats = FrameStats::default();
        let snapshot = stats.snapshot();
        let mut frame_count = 0;

        stats.maybe_print_snapshot(snapshot, &mut frame_count, false);
        assert_eq!(frame_count, 0);

        stats.maybe_print_snapshot(snapshot, &mut frame_count, true);
        assert_eq!(frame_count, 1);
    }

    #[test]
    fn layer_surface_reasons_report_runtime_only_bits() {
        let reasons = LayerSurfaceReasons {
            immediate_shadow: true,
            text_local_surface: true,
            mixed_direct_content: true,
            ..LayerSurfaceReasons::default()
        };

        assert!(reasons.has_any());
        assert!(reasons.has_renderer_forced_surface());
        assert_eq!(
            reasons.labels().collect::<Vec<_>>(),
            vec![
                "immediate_shadow",
                "text_local_surface",
                "mixed_direct_content"
            ]
        );
        assert_eq!(
            reasons.display(),
            "immediate_shadow+text_local_surface+mixed_direct_content"
        );
    }

    #[test]
    fn immediate_shadow_only_is_diagnostic_not_isolating() {
        let reasons = LayerSurfaceReasons {
            immediate_shadow: true,
            ..LayerSurfaceReasons::default()
        };

        assert!(!reasons.has_any());
        assert!(!reasons.has_renderer_forced_surface());
        assert_eq!(
            reasons.labels().collect::<Vec<_>>(),
            vec!["immediate_shadow"]
        );
        assert_eq!(reasons.display(), "immediate_shadow");
    }

    #[test]
    fn mixed_direct_content_only_is_diagnostic_not_isolating() {
        let reasons = LayerSurfaceReasons {
            mixed_direct_content: true,
            ..LayerSurfaceReasons::default()
        };

        assert!(!reasons.has_any());
        assert!(!reasons.has_renderer_forced_surface());
        assert_eq!(
            reasons.labels().collect::<Vec<_>>(),
            vec!["mixed_direct_content"]
        );
        assert_eq!(reasons.display(), "mixed_direct_content");
    }

    #[test]
    fn has_any_matches_has_isolating_requirement_for_each_requirement() {
        let all_requirements = [
            SurfaceRequirement::ExplicitOffscreen,
            SurfaceRequirement::RenderEffect,
            SurfaceRequirement::Backdrop,
            SurfaceRequirement::GroupOpacity,
            SurfaceRequirement::BlendMode,
            SurfaceRequirement::ShapeClip,
            SurfaceRequirement::ImmediateShadow,
            SurfaceRequirement::TextMaterialMask,
            SurfaceRequirement::MotionStableCapture,
            SurfaceRequirement::NonTranslationTransform,
            SurfaceRequirement::MixedDirectContent,
            SurfaceRequirement::PixelStableComposite,
        ];
        for requirement in all_requirements {
            let set = SurfaceRequirementSet::default().with(requirement);
            let reasons = LayerSurfaceReasons::from(set);
            assert_eq!(
                reasons.has_any(),
                set.has_isolating_requirement(),
                "has_any vs has_isolating_requirement mismatch for {requirement:?}"
            );
        }
    }

    #[test]
    fn top_isolated_layers_keep_largest_runtime_surfaces() {
        let stats = FrameStats::default();
        for index in 0..(TOP_ISOLATED_LAYER_LIMIT + 2) {
            stats.record_isolated_layer_render(
                16 + index as u32,
                8 + index as u32,
                Some(index),
                Rect {
                    x: index as f32,
                    y: 0.0,
                    width: 10.0,
                    height: 10.0,
                },
                LayerSurfaceReasons {
                    text_local_surface: true,
                    ..LayerSurfaceReasons::default()
                },
            );
        }

        let snapshot = stats.snapshot();
        let top_layers = snapshot.top_isolated_layers().collect::<Vec<_>>();
        assert_eq!(top_layers.len(), TOP_ISOLATED_LAYER_LIMIT);
        assert_eq!(top_layers[0].node_id, Some(TOP_ISOLATED_LAYER_LIMIT + 1));
        assert_eq!(top_layers[1].node_id, Some(TOP_ISOLATED_LAYER_LIMIT));
    }
}
