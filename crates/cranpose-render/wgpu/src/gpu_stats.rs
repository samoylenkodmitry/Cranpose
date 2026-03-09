//! Per-frame GPU debug counters.
//!
//! Enabled via `CRANPOSE_GPU_STATS=1` environment variable.
//! Prints a summary line every 60 frames to stderr.

use std::cell::Cell;

/// Per-frame debug counters for GPU work instrumentation.
/// Uses `Cell` fields so counters can be bumped through shared references.
#[derive(Default)]
pub(crate) struct FrameStats {
    pub submits: Cell<u32>,
    pub offscreen_acquires: Cell<u32>,
    pub offscreen_news: Cell<u32>,
    pub offscreen_total_bytes: Cell<u64>,
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

    pub fn print_and_reset(&self, frame_count: &mut u64) {
        *frame_count += 1;
        if (*frame_count).is_multiple_of(60) {
            let mb = self.offscreen_total_bytes.get() as f64 / (1024.0 * 1024.0);
            let pool_mb = self.offscreen_pool_bytes.get() as f64 / (1024.0 * 1024.0);
            let layer_cache_total = self.layer_cache_hits.get() + self.layer_cache_misses.get();
            let layer_cache_hit_rate = if layer_cache_total > 0 {
                (self.layer_cache_hits.get() as f64 / layer_cache_total as f64) * 100.0
            } else {
                0.0
            };
            let layer_cache_hit_mpx = self.layer_cache_hit_pixels.get() as f64 / 1_000_000.0;
            let layer_cache_miss_mpx = self.layer_cache_miss_pixels.get() as f64 / 1_000_000.0;
            let layer_cache_mb = self.layer_cache_bytes.get() as f64 / (1024.0 * 1024.0);
            eprintln!(
                "[GPU f#{}] submits={} | offscreen: acq={} new={} {:.1}MB pool={}({:.1}MB) | \
                 layer_cache: hit={} miss={} {:.1}% evict={} hit_px={:.2}MP miss_px={:.2}MP size={}({:.1}MB) | \
                 blur={} composite={} effect={} | shape={} image={} text={} | \
                 caches: text_pool={} img={} txt={}",
                frame_count,
                self.submits.get(),
                self.offscreen_acquires.get(),
                self.offscreen_news.get(),
                mb,
                self.offscreen_pool_size.get(),
                pool_mb,
                self.layer_cache_hits.get(),
                self.layer_cache_misses.get(),
                layer_cache_hit_rate,
                self.layer_cache_evictions.get(),
                layer_cache_hit_mpx,
                layer_cache_miss_mpx,
                self.layer_cache_size.get(),
                layer_cache_mb,
                self.blur_passes.get(),
                self.composite_passes.get(),
                self.effect_applies.get(),
                self.shape_passes.get(),
                self.image_passes.get(),
                self.text_passes.get(),
                self.text_pool_size.get(),
                self.image_cache_size.get(),
                self.text_cache_size.get(),
            );
        }
        self.submits.set(0);
        self.offscreen_acquires.set(0);
        self.offscreen_news.set(0);
        self.offscreen_total_bytes.set(0);
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
        stats.record_layer_cache_hit(10, 20);
        stats.record_layer_cache_hit(3, 4);
        stats.record_layer_cache_miss(5, 6);
        stats.record_layer_cache_eviction();

        assert_eq!(stats.layer_cache_hits.get(), 2);
        assert_eq!(stats.layer_cache_misses.get(), 1);
        assert_eq!(stats.layer_cache_evictions.get(), 1);
        assert_eq!(stats.layer_cache_hit_pixels.get(), 212);
        assert_eq!(stats.layer_cache_miss_pixels.get(), 30);

        let mut frame_count = 0;
        stats.print_and_reset(&mut frame_count);

        assert_eq!(frame_count, 1);
        assert_eq!(stats.layer_cache_hits.get(), 0);
        assert_eq!(stats.layer_cache_misses.get(), 0);
        assert_eq!(stats.layer_cache_evictions.get(), 0);
        assert_eq!(stats.layer_cache_hit_pixels.get(), 0);
        assert_eq!(stats.layer_cache_miss_pixels.get(), 0);
    }
}
