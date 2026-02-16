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
    pub blur_passes: Cell<u32>,
    pub composite_passes: Cell<u32>,
    pub effect_applies: Cell<u32>,
    pub shape_passes: Cell<u32>,
    pub image_passes: Cell<u32>,
    pub text_passes: Cell<u32>,
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
            eprintln!(
                "[GPU f#{}] submits={} | offscreen: acq={} new={} {:.1}MB | \
                 blur={} composite={} effect={} | shape={} image={} text={}",
                frame_count,
                self.submits.get(),
                self.offscreen_acquires.get(),
                self.offscreen_news.get(),
                mb,
                self.blur_passes.get(),
                self.composite_passes.get(),
                self.effect_applies.get(),
                self.shape_passes.get(),
                self.image_passes.get(),
                self.text_passes.get(),
            );
        }
        self.submits.set(0);
        self.offscreen_acquires.set(0);
        self.offscreen_news.set(0);
        self.offscreen_total_bytes.set(0);
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
