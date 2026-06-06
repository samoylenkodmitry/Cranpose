//! FPS monitoring for performance tracking.

use std::collections::VecDeque;
use web_time::Instant;

const FRAME_HISTORY_SIZE: usize = 60;
const EVENT_DRIVEN_IDLE_GAP_MS: f32 = 50.0;

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
        sorted.sort_by(|a, b| a.total_cmp(b));

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
    if avg_ms > 0.0 {
        1000.0 / avg_ms
    } else {
        0.0
    }
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
mod tests {
    use super::{nearest_rank_percentile, FpsMonitor, FpsTracker};
    use std::time::Duration;

    #[test]
    fn monitors_do_not_share_recomposition_or_frame_counts() {
        let mut first = FpsMonitor::new();
        let mut second = FpsMonitor::new();

        first.record_recomposition();
        first.record_recomposition();
        first.record_frame();
        second.record_frame();

        let first_stats = first.stats();
        let second_stats = second.stats();

        assert_eq!(first_stats.recompositions, 2);
        assert_eq!(second_stats.recompositions, 0);
        assert_eq!(first_stats.frame_count, 1);
        assert_eq!(second_stats.frame_count, 1);
    }

    #[test]
    fn reset_stats_reports_recompositions_since_reset() {
        let mut monitor = FpsMonitor::new();
        monitor.record_recomposition();
        monitor.record_recomposition();

        monitor.reset_stats();
        assert_eq!(monitor.stats().recompositions, 0);

        monitor.record_recomposition();
        assert_eq!(monitor.stats().recompositions, 1);
    }

    #[test]
    fn nearest_rank_percentile_reports_tail_samples() {
        let samples = [1.0, 2.0, 3.0, 40.0];

        assert_eq!(nearest_rank_percentile(&samples, 50), 2.0);
        assert_eq!(nearest_rank_percentile(&samples, 95), 40.0);
        assert_eq!(nearest_rank_percentile(&samples, 99), 40.0);
    }

    #[test]
    fn frame_stats_report_pacing_jank_not_just_average_fps() {
        let mut tracker = FpsTracker::new();
        let start = web_time::Instant::now();
        let offsets = [0u64, 8, 16, 24, 64, 72];

        for offset in offsets {
            tracker.record_frame_at(start + Duration::from_millis(offset), 0);
        }

        let stats = tracker.stats(0);

        assert_eq!(stats.interval_count, 5);
        assert_eq!(stats.frame_count, offsets.len() as u64);
        assert!((stats.latest_ms - 8.0).abs() < 0.1);
        assert!((stats.max_ms - 40.0).abs() < 0.1);
        assert!((stats.p95_ms - 40.0).abs() < 0.1);
        assert_eq!(stats.missed_120hz_budget, 1);
        assert_eq!(stats.missed_60hz_budget, 1);
        assert_eq!(stats.stalled_50ms_frames, 0);
        assert!(
            stats.fps > 60.0,
            "average FPS can stay plausible while the p95 frame is bad"
        );
    }

    #[test]
    fn frame_stats_report_frame_work_separately_from_pacing_gaps() {
        let mut tracker = FpsTracker::new();
        let start = web_time::Instant::now();
        let starts = [0u64, 40, 80];
        let work = [2u64, 3, 4];

        for (start_offset, work_ms) in starts.into_iter().zip(work) {
            let frame_start = start + Duration::from_millis(start_offset);
            let frame_end = frame_start + Duration::from_millis(work_ms);
            tracker.record_frame_work(frame_start, frame_end, 0);
        }

        let stats = tracker.stats(0);

        assert!((stats.p95_ms - 40.0).abs() < 0.1);
        assert!((stats.work_avg_ms - 3.0).abs() < 0.1);
        assert!((stats.work_p95_ms - 4.0).abs() < 0.1);
        assert!((stats.work_max_ms - 4.0).abs() < 0.1);
        assert_eq!(stats.missed_120hz_budget, 2);
        assert_eq!(stats.work_missed_120hz_budget, 0);
        assert!(
            stats.work_fps > 300.0,
            "work FPS must measure renderer capacity, not input cadence: {stats:?}"
        );
    }

    #[test]
    fn reset_stats_drops_active_history_before_measurement_window() {
        let mut tracker = FpsTracker::new();
        let start = web_time::Instant::now();

        tracker.record_frame_at(start, 3);
        tracker.record_frame_at(start + Duration::from_millis(8), 3);
        tracker.record_frame_at(start + Duration::from_secs(4), 3);
        let before_reset = tracker.stats(3);
        assert_eq!(before_reset.interval_count, 1);
        assert!((before_reset.max_ms - 8.0).abs() < 0.1);

        tracker.reset(3);
        tracker.record_frame_at(start + Duration::from_secs(4) + Duration::from_millis(8), 3);
        tracker.record_frame_at(
            start + Duration::from_secs(4) + Duration::from_millis(16),
            3,
        );

        let stats = tracker.stats(3);
        assert_eq!(stats.frame_count, 2);
        assert_eq!(stats.interval_count, 1);
        assert!((stats.max_ms - 8.0).abs() < 0.1);
        assert_eq!(stats.recomps_per_second, 0);
    }

    #[test]
    fn frame_stats_ignore_idle_gap_between_event_driven_frames() {
        let mut tracker = FpsTracker::new();
        let start = web_time::Instant::now();

        tracker.record_frame_work(start, start + Duration::from_millis(2), 0);
        tracker.record_frame_work(
            start + Duration::from_millis(8),
            start + Duration::from_millis(10),
            0,
        );
        tracker.record_frame_work(
            start + Duration::from_secs(4),
            start + Duration::from_secs(4) + Duration::from_millis(1),
            0,
        );
        tracker.record_frame_work(
            start + Duration::from_secs(4) + Duration::from_millis(8),
            start + Duration::from_secs(4) + Duration::from_millis(9),
            0,
        );

        let stats = tracker.stats(0);

        assert_eq!(stats.interval_count, 2);
        assert!(
            stats.max_ms < 10.0,
            "idle wait must not be reported as active frame pacing: {stats:?}"
        );
        assert!(
            stats.fps > 120.0,
            "cheap event-driven frames should report active rendering capacity: {stats:?}"
        );
        assert!(
            stats.work_fps > 500.0,
            "cheap event-driven work should keep separate capacity stats: {stats:?}"
        );
        assert!((stats.work_max_ms - 2.0).abs() < 0.1);
        assert_eq!(stats.work_missed_120hz_budget, 0);
    }

    #[test]
    fn frame_stats_ignore_post_interaction_idle_gap_before_next_redraw() {
        let mut tracker = FpsTracker::new();
        let start = web_time::Instant::now();

        tracker.record_frame_work(start, start + Duration::from_millis(3), 0);
        tracker.record_frame_work(
            start + Duration::from_millis(8),
            start + Duration::from_millis(11),
            0,
        );
        tracker.record_frame_work(
            start + Duration::from_millis(16),
            start + Duration::from_millis(19),
            0,
        );
        tracker.record_frame_work(
            start + Duration::from_millis(165),
            start + Duration::from_millis(168),
            0,
        );

        let stats = tracker.stats(0);

        assert_eq!(
            stats.interval_count, 2,
            "post-interaction idle gaps must not dilute active redraw cadence: {stats:?}"
        );
        assert!((stats.max_ms - 8.0).abs() < 0.1);
        assert_eq!(stats.stalled_50ms_frames, 0);
        assert_eq!(stats.work_stalled_50ms_frames, 0);
    }
}
