//! FPS monitoring for performance tracking.
//!
//! Designed for reactive systems (non-busy-loop):
//! - Tracks actual rendered frames, not idle time
//! - Separately tracks recompositions
//! - Provides stats meaningful for optimization

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;
use web_time::Instant;

/// Global FPS tracker singleton
static FPS_TRACKER: RwLock<Option<FpsTracker>> = RwLock::new(None);

/// Global recomposition counter (can be incremented from anywhere)
static RECOMPOSITION_COUNT: AtomicU64 = AtomicU64::new(0);

/// Number of frames to average for FPS calculation
const FRAME_HISTORY_SIZE: usize = 60;

/// Tracks frame times to calculate FPS.
pub struct FpsTracker {
    /// Timestamps of recent frames
    frame_times: VecDeque<Instant>,
    /// Cached FPS value
    last_fps: f32,
    /// Total frames rendered
    frame_count: u64,
    /// Rolling average of frame duration in ms
    avg_frame_ms: f32,
    /// Last recomposition count seen (for delta)
    last_recomp_count: u64,
    /// Recompositions in last second
    recomps_per_second: u64,
    /// Time of last recomp/sec calculation
    last_recomp_calc: Instant,
}

impl FpsTracker {
    fn new() -> Self {
        Self {
            frame_times: VecDeque::with_capacity(FRAME_HISTORY_SIZE + 1),
            last_fps: 0.0,
            frame_count: 0,
            avg_frame_ms: 0.0,
            last_recomp_count: 0,
            recomps_per_second: 0,
            last_recomp_calc: Instant::now(),
        }
    }

    fn record_frame(&mut self) {
        let now = Instant::now();

        self.frame_times.push_back(now);
        self.frame_count += 1;

        // Keep only recent frames
        while self.frame_times.len() > FRAME_HISTORY_SIZE {
            self.frame_times.pop_front();
        }

        // Calculate FPS from frame history
        if self.frame_times.len() >= 2 {
            let first = self.frame_times.front().unwrap();
            let last = self.frame_times.back().unwrap();
            let duration = last.duration_since(*first).as_secs_f32();
            if duration > 0.0 {
                self.last_fps = (self.frame_times.len() - 1) as f32 / duration;
                self.avg_frame_ms = duration * 1000.0 / (self.frame_times.len() - 1) as f32;
            }
        }

        // Calculate recompositions per second (update every second)
        let elapsed = now.duration_since(self.last_recomp_calc).as_secs_f32();
        if elapsed >= 1.0 {
            let current_recomp = RECOMPOSITION_COUNT.load(Ordering::Relaxed);
            self.recomps_per_second = current_recomp - self.last_recomp_count;
            self.last_recomp_count = current_recomp;
            self.last_recomp_calc = now;
        }
    }

    fn stats(&self) -> FpsStats {
        FpsStats {
            fps: self.last_fps,
            avg_ms: self.avg_frame_ms,
            frame_count: self.frame_count,
            recompositions: RECOMPOSITION_COUNT.load(Ordering::Relaxed),
            recomps_per_second: self.recomps_per_second,
        }
    }
}

/// Frame statistics snapshot.
#[derive(Clone, Copy, Debug, Default)]
pub struct FpsStats {
    /// Current FPS (frames per second)
    pub fps: f32,
    /// Average frame time in milliseconds
    pub avg_ms: f32,
    /// Total frame count since start
    pub frame_count: u64,
    /// Total recomposition count since start
    pub recompositions: u64,
    /// Recompositions in the last second
    pub recomps_per_second: u64,
}

/// Initialize the FPS tracker. Call once at app startup.
pub fn init_fps_tracker() {
    let mut tracker = FPS_TRACKER.write().unwrap();
    *tracker = Some(FpsTracker::new());
}

/// Record a frame. Call once per frame in the render loop.
pub fn record_frame() {
    if let Ok(mut tracker) = FPS_TRACKER.write() {
        if let Some(ref mut t) = *tracker {
            t.record_frame();
        }
    }
}

/// Increment the recomposition counter. Call when a scope is recomposed.
pub fn record_recomposition() {
    RECOMPOSITION_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Get current FPS.
pub fn current_fps() -> f32 {
    if let Ok(tracker) = FPS_TRACKER.read() {
        if let Some(ref t) = *tracker {
            return t.last_fps;
        }
    }
    0.0
}

/// Get detailed frame statistics.
pub fn fps_stats() -> FpsStats {
    if let Ok(tracker) = FPS_TRACKER.read() {
        if let Some(ref t) = *tracker {
            return t.stats();
        }
    }
    FpsStats::default()
}

/// Format FPS as a display string.
pub fn fps_display() -> String {
    let stats = fps_stats();
    format!("{:.0} FPS ({:.1}ms)", stats.fps, stats.avg_ms)
}

/// Format detailed stats as a display string.
pub fn fps_display_detailed() -> String {
    let stats = fps_stats();
    format!(
        "{:.0} FPS | {:.1}ms | recomp: {}/s",
        stats.fps, stats.avg_ms, stats.recomps_per_second
    )
}
