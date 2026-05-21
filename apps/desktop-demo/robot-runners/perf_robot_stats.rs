use cranpose_render_wgpu::RenderStatsSnapshot;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RenderStatsAccumulator {
    pub samples: u64,
    pub submits: u64,
    pub offscreen_acquires: u64,
    pub offscreen_news: u64,
    pub offscreen_total_bytes: u64,
    pub upload_bytes: u64,
    pub isolated_layer_renders: u64,
    pub isolated_layer_pixels: u64,
    pub layer_cache_hits: u64,
    pub layer_cache_misses: u64,
    pub layer_cache_evictions: u64,
    pub layer_cache_hit_pixels: u64,
    pub layer_cache_miss_pixels: u64,
    pub blur_passes: u64,
    pub composite_passes: u64,
    pub effect_applies: u64,
    pub shape_passes: u64,
    pub image_passes: u64,
    pub text_passes: u64,
    pub max_upload_bytes: u64,
    pub max_isolated_layer_pixels: u64,
}

impl RenderStatsAccumulator {
    pub(crate) fn record(&mut self, stats: RenderStatsSnapshot) {
        self.samples = self.samples.saturating_add(1);
        self.submits = self.submits.saturating_add(stats.submits as u64);
        self.offscreen_acquires = self
            .offscreen_acquires
            .saturating_add(stats.offscreen_acquires as u64);
        self.offscreen_news = self
            .offscreen_news
            .saturating_add(stats.offscreen_news as u64);
        self.offscreen_total_bytes = self
            .offscreen_total_bytes
            .saturating_add(stats.offscreen_total_bytes);
        self.upload_bytes = self.upload_bytes.saturating_add(stats.upload_bytes);
        self.isolated_layer_renders = self
            .isolated_layer_renders
            .saturating_add(stats.isolated_layer_renders as u64);
        self.isolated_layer_pixels = self
            .isolated_layer_pixels
            .saturating_add(stats.isolated_layer_pixels);
        self.layer_cache_hits = self
            .layer_cache_hits
            .saturating_add(stats.layer_cache_hits as u64);
        self.layer_cache_misses = self
            .layer_cache_misses
            .saturating_add(stats.layer_cache_misses as u64);
        self.layer_cache_evictions = self
            .layer_cache_evictions
            .saturating_add(stats.layer_cache_evictions as u64);
        self.layer_cache_hit_pixels = self
            .layer_cache_hit_pixels
            .saturating_add(stats.layer_cache_hit_pixels);
        self.layer_cache_miss_pixels = self
            .layer_cache_miss_pixels
            .saturating_add(stats.layer_cache_miss_pixels);
        self.blur_passes = self.blur_passes.saturating_add(stats.blur_passes as u64);
        self.composite_passes = self
            .composite_passes
            .saturating_add(stats.composite_passes as u64);
        self.effect_applies = self
            .effect_applies
            .saturating_add(stats.effect_applies as u64);
        self.shape_passes = self.shape_passes.saturating_add(stats.shape_passes as u64);
        self.image_passes = self.image_passes.saturating_add(stats.image_passes as u64);
        self.text_passes = self.text_passes.saturating_add(stats.text_passes as u64);
        self.max_upload_bytes = self.max_upload_bytes.max(stats.upload_bytes);
        self.max_isolated_layer_pixels = self
            .max_isolated_layer_pixels
            .max(stats.isolated_layer_pixels);
    }

    pub(crate) fn average_u64(&self, total: u64) -> u64 {
        total.checked_div(self.samples).unwrap_or(0)
    }

    pub(crate) fn cache_hit_rate_pct(self) -> f64 {
        let total = self.layer_cache_hits + self.layer_cache_misses;
        if total == 0 {
            0.0
        } else {
            (self.layer_cache_hits as f64 / total as f64) * 100.0
        }
    }
}

pub(crate) fn print_render_summary(scenario: &str, stats: RenderStatsAccumulator) {
    println!(
        "PERF_RENDER_SUMMARY scenario={} samples={} avg_submits={} avg_offscreen_acquires={} avg_offscreen_bytes={} avg_upload_bytes={} max_upload_bytes={} avg_isolated_layers={} avg_isolated_pixels={} max_isolated_pixels={} cache_hits={} cache_misses={} cache_hit_rate_pct={:.2} cache_evictions={} avg_blur_passes={} avg_composite_passes={} avg_effect_applies={} avg_shape_passes={} avg_image_passes={} avg_text_passes={}",
        scenario,
        stats.samples,
        stats.average_u64(stats.submits),
        stats.average_u64(stats.offscreen_acquires),
        stats.average_u64(stats.offscreen_total_bytes),
        stats.average_u64(stats.upload_bytes),
        stats.max_upload_bytes,
        stats.average_u64(stats.isolated_layer_renders),
        stats.average_u64(stats.isolated_layer_pixels),
        stats.max_isolated_layer_pixels,
        stats.layer_cache_hits,
        stats.layer_cache_misses,
        stats.cache_hit_rate_pct(),
        stats.layer_cache_evictions,
        stats.average_u64(stats.blur_passes),
        stats.average_u64(stats.composite_passes),
        stats.average_u64(stats.effect_applies),
        stats.average_u64(stats.shape_passes),
        stats.average_u64(stats.image_passes),
        stats.average_u64(stats.text_passes),
    );
}
