//! Per-frame GPU render counters.
//!
//! Counters are always collected so tests and perf harnesses can assert them.
//! Setting `CRANPOSE_GPU_STATS=1` prints a summary line every 60 frames to stderr.

use crate::frame_graph::FrameCommandStats;
use crate::surface_requirements::{SurfaceRequirement, SurfaceRequirementSet};
use std::cell::{Cell, RefCell};

use cranpose_core::NodeId;
use cranpose_ui_graphics::Rect;

const TOP_ISOLATED_LAYER_LIMIT: usize = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayerSurfaceReasons {
    pub explicit_offscreen: bool,
    pub effect: bool,
    pub backdrop: bool,
    pub group_opacity: bool,
    pub blend_mode: bool,
    pub immediate_shadow: bool,
    pub text_local_surface: bool,
    pub motion_stable_capture: bool,
    pub mixed_direct_content: bool,
    pub non_translation_transform: bool,
    pub pixel_stable_composite: bool,
}

impl LayerSurfaceReasons {
    /// Returns true if any isolating requirement is set.
    /// Delegates to `SurfaceRequirementSet::has_isolating_requirement` to
    /// keep the semantics in sync (e.g. `mixed_direct_content` alone does
    /// not count as isolating).
    pub fn has_any(self) -> bool {
        self.explicit_offscreen
            || self.effect
            || self.backdrop
            || self.group_opacity
            || self.blend_mode
            || self.text_local_surface
            || self.motion_stable_capture
            || self.non_translation_transform
    }

    pub fn labels(self) -> impl Iterator<Item = &'static str> {
        let mut labels = [None; 11];
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
    pub blur_passes: u32,
    pub composite_passes: u32,
    pub effect_applies: u32,
    pub shape_passes: u32,
    pub image_passes: u32,
    pub text_passes: u32,
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

    fn print(self, frame_count: u64) {
        let mb = self.offscreen_total_bytes as f64 / (1024.0 * 1024.0);
        let upload_mb = self.upload_bytes as f64 / (1024.0 * 1024.0);
        let retained_mb = self.retained_texture_bytes as f64 / (1024.0 * 1024.0);
        let pool_mb = self.offscreen_pool_bytes as f64 / (1024.0 * 1024.0);
        let layer_cache_hit_mpx = self.layer_cache_hit_pixels as f64 / 1_000_000.0;
        let layer_cache_miss_mpx = self.layer_cache_miss_pixels as f64 / 1_000_000.0;
        let layer_cache_mb = self.layer_cache_bytes as f64 / (1024.0 * 1024.0);
        let isolated_layer_mpx = self.isolated_layer_pixels as f64 / 1_000_000.0;
        eprintln!(
            "[GPU f#{}] encoders={} submits={} passes={} | offscreen: acq={} new={} {:.1}MB pool={}({:.1}MB) retained={:.1}MB | \
             uploads={:.2}MB | \
             isolated_layers={} area={:.2}MP | \
             layer_cache: hit={} miss={} {:.1}% evict={} hit_px={:.2}MP miss_px={:.2}MP size={}({:.1}MB) | \
             blur={} composite={} effect={} | shape={} image={} text={} | \
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
            self.blur_passes,
            self.composite_passes,
            self.effect_applies,
            self.shape_passes,
            self.image_passes,
            self.text_passes,
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

/// Per-frame debug counters for GPU work instrumentation.
/// Uses `Cell` fields so counters can be bumped through shared references.
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
    pub blur_passes: Cell<u32>,
    pub composite_passes: Cell<u32>,
    pub effect_applies: Cell<u32>,
    pub shape_passes: Cell<u32>,
    pub image_passes: Cell<u32>,
    pub text_passes: Cell<u32>,
    // Pool/cache sizes snapshotted at end of frame
    pub offscreen_pool_size: Cell<u32>,
    pub offscreen_pool_bytes: Cell<u64>,
    pub text_pool_size: Cell<u32>,
    pub layer_cache_size: Cell<u32>,
    pub layer_cache_bytes: Cell<u64>,
    pub image_cache_size: Cell<u32>,
    pub text_cache_size: Cell<u32>,
    top_isolated_layers: RefCell<[Option<IsolatedLayerStat>; TOP_ISOLATED_LAYER_LIMIT]>,
    top_isolated_layer_count: Cell<usize>,
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

    pub fn record_offscreen_acquire(&self, width: u32, height: u32, is_new: bool) {
        self.offscreen_acquires
            .set(self.offscreen_acquires.get() + 1);
        if is_new {
            self.offscreen_news.set(self.offscreen_news.get() + 1);
        }
        self.offscreen_total_bytes
            .set(self.offscreen_total_bytes.get() + (width as u64) * (height as u64) * 4);
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

    pub fn record_layer_cache_hit(&self, width: u32, height: u32) {
        self.layer_cache_hits
            .set(self.layer_cache_hits.get().saturating_add(1));
        self.layer_cache_hit_pixels.set(
            self.layer_cache_hit_pixels
                .get()
                .saturating_add((width as u64) * (height as u64)),
        );
    }

    pub fn record_layer_cache_miss(&self, width: u32, height: u32) {
        self.layer_cache_misses
            .set(self.layer_cache_misses.get().saturating_add(1));
        self.layer_cache_miss_pixels.set(
            self.layer_cache_miss_pixels
                .get()
                .saturating_add((width as u64) * (height as u64)),
        );
    }

    pub fn record_layer_cache_eviction(&self) {
        self.layer_cache_evictions
            .set(self.layer_cache_evictions.get().saturating_add(1));
    }

    pub fn bump_shapes(&self) {
        self.shape_passes.set(self.shape_passes.get() + 1);
    }

    pub fn bump_images(&self) {
        self.image_passes.set(self.image_passes.get() + 1);
    }

    pub fn bump_text(&self) {
        self.text_passes.set(self.text_passes.get() + 1);
    }

    pub fn snapshot(&self) -> FrameStatsSnapshot {
        let pass_count = self
            .blur_passes
            .get()
            .saturating_add(self.composite_passes.get())
            .saturating_add(self.effect_applies.get())
            .saturating_add(self.shape_passes.get())
            .saturating_add(self.image_passes.get())
            .saturating_add(self.text_passes.get());
        let retained_texture_bytes = self
            .offscreen_pool_bytes
            .get()
            .saturating_add(self.layer_cache_bytes.get());
        FrameStatsSnapshot {
            submits: self.submits.get(),
            encoder_count: self.command_encoder_count.get(),
            submit_count: self.command_submit_count.get(),
            pass_count: self.command_pass_count.get().saturating_add(pass_count),
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
            blur_passes: self.blur_passes.get(),
            composite_passes: self.composite_passes.get(),
            effect_applies: self.effect_applies.get(),
            shape_passes: self.shape_passes.get(),
            image_passes: self.image_passes.get(),
            text_passes: self.text_passes.get(),
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
        self.blur_passes.set(0);
        self.composite_passes.set(0);
        self.effect_applies.set(0);
        self.shape_passes.set(0);
        self.image_passes.set(0);
        self.text_passes.set(0);
        *self.top_isolated_layers.borrow_mut() = [None; TOP_ISOLATED_LAYER_LIMIT];
        self.top_isolated_layer_count.set(0);
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
    std::env::var("CRANPOSE_GPU_STATS")
        .map(|v| matches!(v.as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_cache_counters_accumulate_and_reset() {
        let stats = FrameStats::default();
        stats.record_upload_bytes(512);
        stats.record_command_stats(FrameCommandStats {
            encoder_count: 1,
            submit_count: 1,
            pass_count: 0,
            transient_texture_bytes: 256,
            retained_texture_bytes: 128,
            upload_bytes: 64,
        });
        stats.bump_shapes();
        stats.blur_passes.set(1);
        stats.offscreen_total_bytes.set(1024);
        stats.offscreen_pool_bytes.set(2048);
        stats.record_layer_cache_hit(10, 20);
        stats.record_layer_cache_hit(3, 4);
        stats.record_layer_cache_miss(5, 6);
        stats.record_layer_cache_eviction();

        assert_eq!(stats.layer_cache_hits.get(), 2);
        assert_eq!(stats.layer_cache_misses.get(), 1);
        assert_eq!(stats.layer_cache_evictions.get(), 1);
        assert_eq!(stats.layer_cache_hit_pixels.get(), 212);
        assert_eq!(stats.layer_cache_miss_pixels.get(), 30);

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
        assert_eq!(snapshot.pass_count, 6);
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
