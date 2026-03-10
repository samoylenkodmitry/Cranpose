//! Per-frame GPU render counters.
//!
//! Counters are always collected so tests and perf harnesses can assert them.
//! Setting `CRANPOSE_GPU_STATS=1` prints a summary line every 60 frames to stderr.

use std::cell::Cell;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameStatsSnapshot {
    pub submits: u32,
    pub offscreen_acquires: u32,
    pub offscreen_news: u32,
    pub offscreen_total_bytes: u64,
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
}

impl FrameStatsSnapshot {
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
        let pool_mb = self.offscreen_pool_bytes as f64 / (1024.0 * 1024.0);
        let layer_cache_hit_mpx = self.layer_cache_hit_pixels as f64 / 1_000_000.0;
        let layer_cache_miss_mpx = self.layer_cache_miss_pixels as f64 / 1_000_000.0;
        let layer_cache_mb = self.layer_cache_bytes as f64 / (1024.0 * 1024.0);
        let isolated_layer_mpx = self.isolated_layer_pixels as f64 / 1_000_000.0;
        eprintln!(
            "[GPU f#{}] submits={} | offscreen: acq={} new={} {:.1}MB pool={}({:.1}MB) | \
             uploads={:.2}MB | \
             isolated_layers={} area={:.2}MP | \
             layer_cache: hit={} miss={} {:.1}% evict={} hit_px={:.2}MP miss_px={:.2}MP size={}({:.1}MB) | \
             blur={} composite={} effect={} | shape={} image={} text={} | \
             caches: text_pool={} img={} txt={}",
            frame_count,
            self.submits,
            self.offscreen_acquires,
            self.offscreen_news,
            mb,
            self.offscreen_pool_size,
            pool_mb,
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
    }
}

/// Per-frame debug counters for GPU work instrumentation.
/// Uses `Cell` fields so counters can be bumped through shared references.
#[derive(Default)]
pub(crate) struct FrameStats {
    pub submits: Cell<u32>,
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
}

impl FrameStats {
    pub fn bump_submits(&self) {
        self.submits.set(self.submits.get() + 1);
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

    pub fn record_isolated_layer_render(&self, width: u32, height: u32) {
        self.isolated_layer_renders
            .set(self.isolated_layer_renders.get().saturating_add(1));
        self.isolated_layer_pixels.set(
            self.isolated_layer_pixels
                .get()
                .saturating_add((width as u64) * (height as u64)),
        );
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
        FrameStatsSnapshot {
            submits: self.submits.get(),
            offscreen_acquires: self.offscreen_acquires.get(),
            offscreen_news: self.offscreen_news.get(),
            offscreen_total_bytes: self.offscreen_total_bytes.get(),
            upload_bytes: self.upload_bytes.get(),
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
        }
    }

    pub fn reset(&self) {
        self.submits.set(0);
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
}

pub(crate) fn gpu_stats_enabled() -> bool {
    use std::sync::OnceLock;
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("CRANPOSE_GPU_STATS")
            .map(|v| matches!(v.as_str(), "1" | "true" | "yes"))
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_cache_counters_accumulate_and_reset() {
        let stats = FrameStats::default();
        stats.record_upload_bytes(512);
        stats.record_layer_cache_hit(10, 20);
        stats.record_layer_cache_hit(3, 4);
        stats.record_layer_cache_miss(5, 6);
        stats.record_layer_cache_eviction();

        assert_eq!(stats.layer_cache_hits.get(), 2);
        assert_eq!(stats.layer_cache_misses.get(), 1);
        assert_eq!(stats.layer_cache_evictions.get(), 1);
        assert_eq!(stats.layer_cache_hit_pixels.get(), 212);
        assert_eq!(stats.layer_cache_miss_pixels.get(), 30);

        stats.record_isolated_layer_render(7, 8);
        let snapshot = stats.snapshot();

        assert_eq!(snapshot.isolated_layer_renders, 1);
        assert_eq!(snapshot.isolated_layer_pixels, 56);
        assert_eq!(snapshot.upload_bytes, 512);
        assert_eq!(snapshot.layer_cache_hits, 2);
        assert_eq!(snapshot.layer_cache_misses, 1);
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
}
