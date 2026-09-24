use std::collections::VecDeque;

use web_time::Instant;

const FRAME_HISTORY_SIZE: usize = 60;
const EVENT_DRIVEN_IDLE_GAP_MS: f32 = 50.0;

fn recomposition_diag_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("CRANPOSE_RECOMP_DIAG").is_some())
}

fn recomposition_diagnostic_line(rate: u64, frames_per_second: f32, total: u64) -> String {
    format!("[recomp] {rate}/s frames={frames_per_second:.1}/s total={total}")
}

#[derive(Debug)]
pub(crate) struct FpsMonitor {
    tracker: FpsTracker,
    recomposition_count: u64,
    recomposition_reset_baseline: u64,
}

impl FpsMonitor {
    pub(crate) fn new() -> Self {
        Self {
            tracker: FpsTracker::new(),
            recomposition_count: 0,
            recomposition_reset_baseline: 0,
        }
    }

    #[cfg(test)]
    pub(crate) fn record_frame(&mut self) {
        self.tracker.record_frame(self.recomposition_count);
    }

    pub(crate) fn record_frame_work(
        &mut self,
        frame_started_at: Instant,
        frame_finished_at: Instant,
    ) {
        self.tracker.record_frame_work(
            frame_started_at,
            frame_finished_at,
            self.recomposition_count,
        );
    }

    pub(crate) fn record_recomposition(&mut self) {
        self.recomposition_count = self.recomposition_count.saturating_add(1);
    }

    pub(crate) fn reset_stats(&mut self) {
        self.tracker.reset(self.recomposition_count);
        self.recomposition_reset_baseline = self.recomposition_count;
    }

    pub(crate) fn current_fps(&self) -> f32 {
        self.tracker.last_fps
    }

    pub(crate) fn stats(&self) -> FpsStats {
        self.tracker.stats(
            self.recomposition_count
                .saturating_sub(self.recomposition_reset_baseline),
        )
    }
}

impl Default for FpsMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
struct FpsTracker {
    frame_times: VecDeque<Instant>,
    frame_intervals_ms: VecDeque<f32>,
    frame_work_ms: VecDeque<f32>,
    last_fps: f32,
    frame_count: u64,
    intervals: FrameIntervalStats,
    work: FrameIntervalStats,
    last_recomp_count: u64,
    recomps_per_second: u64,
    last_recomp_calc: Instant,
}

impl FpsTracker {
    fn new() -> Self {
        Self {
            frame_times: VecDeque::with_capacity(FRAME_HISTORY_SIZE + 1),
            frame_intervals_ms: VecDeque::with_capacity(FRAME_HISTORY_SIZE),
            frame_work_ms: VecDeque::with_capacity(FRAME_HISTORY_SIZE),
            last_fps: 0.0,
            frame_count: 0,
            intervals: FrameIntervalStats::default(),
            work: FrameIntervalStats::default(),
            last_recomp_count: 0,
            recomps_per_second: 0,
            last_recomp_calc: Instant::now(),
        }
    }

    #[cfg(test)]
    fn record_frame(&mut self, recomposition_count: u64) {
        let now = Instant::now();
        self.record_frame_work(now, now, recomposition_count);
    }

    #[cfg(test)]
    fn record_frame_at(&mut self, now: Instant, recomposition_count: u64) {
        self.record_frame_work(now, now, recomposition_count);
    }

    fn reset(&mut self, recomposition_count: u64) {
        self.frame_times.clear();
        self.frame_intervals_ms.clear();
        self.frame_work_ms.clear();
        self.last_fps = 0.0;
        self.frame_count = 0;
        self.intervals = FrameIntervalStats::default();
        self.work = FrameIntervalStats::default();
        self.last_recomp_count = recomposition_count;
        self.recomps_per_second = 0;
        self.last_recomp_calc = Instant::now();
    }

    fn record_frame_work(
        &mut self,
        frame_started_at: Instant,
        frame_finished_at: Instant,
        recomposition_count: u64,
    ) {
        if let Some(previous) = self.frame_times.back() {
            let interval_ms = frame_started_at.duration_since(*previous).as_secs_f32() * 1000.0;
            if interval_ms <= EVENT_DRIVEN_IDLE_GAP_MS {
                self.frame_intervals_ms.push_back(interval_ms);
                while self.frame_intervals_ms.len() > FRAME_HISTORY_SIZE {
                    self.frame_intervals_ms.pop_front();
                }
                self.intervals = FrameIntervalStats::from_samples(&self.frame_intervals_ms);
                self.last_fps = fps_from_avg_ms(self.intervals.avg_ms);
            }
        }

        let work_ms = frame_finished_at
            .duration_since(frame_started_at)
            .as_secs_f32()
            * 1000.0;
        self.frame_work_ms.push_back(work_ms);
        while self.frame_work_ms.len() > FRAME_HISTORY_SIZE {
            self.frame_work_ms.pop_front();
        }
        self.work = FrameIntervalStats::from_samples(&self.frame_work_ms);

        self.frame_times.push_back(frame_started_at);
        self.frame_count += 1;

        while self.frame_times.len() > FRAME_HISTORY_SIZE + 1 {
            self.frame_times.pop_front();
        }

        let elapsed = frame_finished_at
            .duration_since(self.last_recomp_calc)
            .as_secs_f32();
        if elapsed >= 1.0 {
            self.recomps_per_second = recomposition_count.saturating_sub(self.last_recomp_count);
            self.last_recomp_count = recomposition_count;
            self.last_recomp_calc = frame_finished_at;
            if recomposition_diag_enabled() {
                log::info!(
                    target: "cranpose::recomposition",
                    "{}",
                    recomposition_diagnostic_line(
                        self.recomps_per_second,
                        self.last_fps,
                        recomposition_count,
                    )
                );
            }
        }
    }

    fn stats(&self, recomposition_count: u64) -> FpsStats {
        FpsStats {
            fps: self.last_fps,
            avg_ms: self.intervals.avg_ms,
            latest_ms: self.intervals.latest_ms,
            min_ms: self.intervals.min_ms,
            max_ms: self.intervals.max_ms,
            p95_ms: self.intervals.p95_ms,
            p99_ms: self.intervals.p99_ms,
            work_fps: fps_from_avg_ms(self.work.avg_ms),
            work_avg_ms: self.work.avg_ms,
            work_p95_ms: self.work.p95_ms,
            work_max_ms: self.work.max_ms,
            work_missed_120hz_budget: self.work.missed_120hz_budget,
            work_missed_60hz_budget: self.work.missed_60hz_budget,
            work_stalled_50ms_frames: self.work.stalled_50ms_frames,
            interval_count: self.intervals.count,
            missed_120hz_budget: self.intervals.missed_120hz_budget,
            missed_60hz_budget: self.intervals.missed_60hz_budget,
            stalled_50ms_frames: self.intervals.stalled_50ms_frames,
            frame_count: self.frame_count,
            recompositions: recomposition_count,
            recomps_per_second: self.recomps_per_second,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct FrameIntervalStats {
    count: u32,
    latest_ms: f32,
    avg_ms: f32,
    min_ms: f32,
    max_ms: f32,
    p95_ms: f32,
    p99_ms: f32,
    missed_120hz_budget: u32,
    missed_60hz_budget: u32,
    stalled_50ms_frames: u32,
}

impl FrameIntervalStats {
    const FRAME_120HZ_MS: f32 = 1000.0 / 120.0;
    const FRAME_60HZ_MS: f32 = 1000.0 / 60.0;
    const STALL_MS: f32 = 50.0;

    fn from_samples(samples: &VecDeque<f32>) -> Self {
        let count = samples.len();
        if count == 0 {
            return Self::default();
        }

        let mut sorted = [0.0f32; FRAME_HISTORY_SIZE];
        let mut sum = 0.0f32;
        let mut min_ms = f32::INFINITY;
        let mut max_ms = 0.0f32;
        let mut missed_120hz_budget = 0u32;
        let mut missed_60hz_budget = 0u32;
        let mut stalled_50ms_frames = 0u32;

        for (index, interval_ms) in samples.iter().copied().enumerate() {
            sorted[index] = interval_ms;
            sum += interval_ms;
            min_ms = min_ms.min(interval_ms);
            max_ms = max_ms.max(interval_ms);
            if interval_ms > Self::FRAME_120HZ_MS {
                missed_120hz_budget = missed_120hz_budget.saturating_add(1);
            }
            if interval_ms > Self::FRAME_60HZ_MS {
                missed_60hz_budget = missed_60hz_budget.saturating_add(1);
            }
            if interval_ms > Self::STALL_MS {
                stalled_50ms_frames = stalled_50ms_frames.saturating_add(1);
            }
        }

        let sorted = &mut sorted[..count];
        sorted.sort_by(f32::total_cmp);

        Self {
            count: count as u32,
            latest_ms: samples.back().copied().unwrap_or_default(),
            avg_ms: sum / count as f32,
            min_ms,
            max_ms,
            p95_ms: nearest_rank_percentile(sorted, 95),
            p99_ms: nearest_rank_percentile(sorted, 99),
            missed_120hz_budget,
            missed_60hz_budget,
            stalled_50ms_frames,
        }
    }
}

fn fps_from_avg_ms(avg_ms: f32) -> f32 {
    if avg_ms > 0.0 { 1000.0 / avg_ms } else { 0.0 }
}

fn nearest_rank_percentile(sorted_samples: &[f32], percentile: usize) -> f32 {
    if sorted_samples.is_empty() {
        return 0.0;
    }
    let rank = sorted_samples
        .len()
        .saturating_mul(percentile)
        .div_ceil(100)
        .saturating_sub(1);
    sorted_samples[rank.min(sorted_samples.len() - 1)]
}

/// Frame statistics snapshot.
#[derive(Clone, Copy, Debug, Default)]
pub struct FpsStats {
    /// Current presented-frame cadence in frames per second.
    pub fps: f32,
    /// Average presented-frame interval in milliseconds.
    pub avg_ms: f32,
    /// Last recorded frame interval in milliseconds.
    pub latest_ms: f32,
    /// Minimum frame interval in the rolling history.
    pub min_ms: f32,
    /// Maximum frame interval in the rolling history.
    pub max_ms: f32,
    /// 95th percentile frame interval in the rolling history.
    pub p95_ms: f32,
    /// 99th percentile frame interval in the rolling history.
    pub p99_ms: f32,
    /// AppShell work capacity in frames per second.
    pub work_fps: f32,
    /// Average measured AppShell frame work in milliseconds.
    pub work_avg_ms: f32,
    /// 95th percentile measured AppShell frame work in milliseconds.
    pub work_p95_ms: f32,
    /// Maximum measured AppShell frame work in milliseconds.
    pub work_max_ms: f32,
    /// Rolling count of measured AppShell work samples above the 120 Hz frame budget.
    pub work_missed_120hz_budget: u32,
    /// Rolling count of measured AppShell work samples above the 60 Hz frame budget.
    pub work_missed_60hz_budget: u32,
    /// Rolling count of measured AppShell work samples above 50 ms.
    pub work_stalled_50ms_frames: u32,
    /// Number of frame intervals in the rolling history.
    pub interval_count: u32,
    /// Rolling count of intervals above the 120 Hz frame budget.
    pub missed_120hz_budget: u32,
    /// Rolling count of intervals above the 60 Hz frame budget.
    pub missed_60hz_budget: u32,
    /// Rolling count of intervals above 50 ms.
    pub stalled_50ms_frames: u32,
    /// Total frame count since monitor creation.
    pub frame_count: u64,
    /// Recomposition count since the last stats reset.
    pub recompositions: u64,
    /// Recompositions in the last second.
    pub recomps_per_second: u64,
}

#[cfg(test)]
#[path = "tests/fps_monitor_tests.rs"]
mod tests;
