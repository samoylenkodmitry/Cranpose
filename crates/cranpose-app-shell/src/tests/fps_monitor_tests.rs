use std::time::Duration;

use super::{FpsMonitor, FpsTracker, nearest_rank_percentile, recomposition_diagnostic_line};

#[test]
fn recomposition_diagnostic_reports_rate_frames_and_total() {
    assert_eq!(
        recomposition_diagnostic_line(3, 59.94, 17),
        "[recomp] 3/s frames=59.9/s total=17"
    );
}

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
