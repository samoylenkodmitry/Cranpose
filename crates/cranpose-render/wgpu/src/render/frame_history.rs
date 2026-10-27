use std::sync::atomic::{AtomicU64, Ordering};

use crate::{
    frame_graph::FrameCommandStats,
    gpu_stats::{self, FrameStats, FrameStatsSnapshot, gpu_stats_enabled},
};

const CACHE_MISS_WARMUP_FRAMES: u8 = 1;

static PRESENTED_FRAMES: AtomicU64 = AtomicU64::new(0);

/// How many frames this process has drawn and presented since it started.
pub fn frames_presented() -> u64 {
    PRESENTED_FRAMES.load(Ordering::Relaxed)
}

// What a renderer keeps of the frames it drew: the last frame's stats, the
// frames a cache miss still asks for, and how many frames it printed.
#[derive(Default)]
pub(super) struct FrameHistory {
    last: Option<FrameStatsSnapshot>,
    warmup_frames: u8,
    count: u64,
}

impl FrameHistory {
    // Keeps the frame's stats, and logs and prints them as asked.
    pub(super) fn close(&mut self, stats: &FrameStats, device: &wgpu::Device) {
        let snapshot = stats.snapshot();
        if crate::frame_graph::frame_graph_pass_telemetry_threshold_ms().is_some() {
            log::warn!(
                "[wgpu-render-stage:frame-stats] layer_hit={} layer_miss={} miss_px={} \
                 offscreen_acq={} offscreen_new={} isolated={} draws={}",
                snapshot.layer_cache_hits,
                snapshot.layer_cache_misses,
                snapshot.layer_cache_miss_pixels,
                snapshot.offscreen_acquires,
                snapshot.offscreen_news,
                snapshot.isolated_layer_renders,
                snapshot.draw_calls,
            );
        }
        self.last = Some(snapshot);
        PRESENTED_FRAMES.fetch_add(1, Ordering::Relaxed);
        if self.warmup_frames > 0 {
            self.warmup_frames -= 1;
        } else if needs_warmup_frame(&snapshot) {
            self.warmup_frames = CACHE_MISS_WARMUP_FRAMES;
        }
        let gpu_stats_on = gpu_stats_enabled();
        stats.maybe_print_snapshot(snapshot, &mut self.count, gpu_stats_on);
        if gpu_stats_on && self.count.is_multiple_of(60) {
            gpu_stats::print_gpu_memory_report(device, self.count);
        }
    }

    // A frame the surface already showed takes the warm-up frame it was owed.
    pub(super) fn skip_frame(&mut self) {
        self.warmup_frames = self.warmup_frames.saturating_sub(1);
    }

    pub(super) fn last(&self) -> Option<FrameStatsSnapshot> {
        self.last
    }

    // Whether a cache the last frames missed still asks for a frame.
    pub(super) fn warming_up(&self) -> bool {
        self.warmup_frames > 0
    }

    // Adds the commands of a copy made after the frame closed.
    pub(super) fn add_command_stats(&mut self, stats: FrameCommandStats) {
        self.last = self
            .last
            .map(|snapshot| snapshot.with_command_stats_added(stats));
    }
}

fn needs_warmup_frame(snapshot: &FrameStatsSnapshot) -> bool {
    snapshot.layer_cache_misses > 0
        || snapshot.shadow_shape_cache_misses > 0
        || snapshot.text_image_cache_misses > 0
        || snapshot.text_glyph_atlas_misses > 0
}
