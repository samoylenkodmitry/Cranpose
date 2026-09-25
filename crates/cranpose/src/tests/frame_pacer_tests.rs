use super::*;

const VSYNC: i64 = 16_666_667;

/// When a pacer from [`buffered`] came down to two frames queued, after
/// the display showed its unpaced queue full.
const STUFFED: i64 = FULL_HISTORY as i64 * VSYNC;

/// When a pacer from [`shallow`] has settled at one frame queued: the hold
/// at two brings it down, and a stable stretch settles it there.
const SETTLED: i64 = STUFFED + FIRST_HOLD_NS + STABLE_LEVEL_NS;

/// A pacer paced at two frames queued since [`STUFFED`].
fn buffered() -> FramePacer {
    let mut pacer = FramePacer::default();
    shown_run(&mut pacer, 0, FULL_HISTORY as i64, 3);
    assert_eq!(pacer.level(STUFFED), Some(Level::Buffered));
    pacer
}

/// A pacer settled at one frame queued, the last frame shown at [`SETTLED`].
fn shallow() -> FramePacer {
    let mut pacer = buffered();
    assert_eq!(pacer.level(STUFFED + FIRST_HOLD_NS), Some(Level::Shallow));
    pacer.record_shown(SETTLED, 1, VSYNC);
    pacer
}

/// Records `count` frames shown one vsync apart from `start`, each with
/// `queued_behind` more frames queued behind it, and returns the vsync after
/// the last.
fn shown_run(pacer: &mut FramePacer, start: i64, count: i64, queued_behind: u32) -> i64 {
    for frame in 0..count {
        pacer.record_shown(start + frame * VSYNC, queued_behind, VSYNC);
    }
    start + count * VSYNC
}

/// Records one frame shown at `start`, then `misses` more two vsyncs apart,
/// and returns when the last was shown.
fn missed_run(pacer: &mut FramePacer, start: i64, misses: i64) -> i64 {
    pacer.record_shown(start, 1, VSYNC);
    for miss in 1..=misses {
        pacer.record_shown(start + 2 * VSYNC * miss, 1, VSYNC);
    }
    start + 2 * VSYNC * misses
}

/// Starts a frame just after the vsync at `vsync`.
fn begin_at(pacer: &mut FramePacer, vsync: i64) -> bool {
    pacer.begin_frame(vsync + 100_000, vsync, VSYNC)
}

fn level(pacer: &mut FramePacer, now: i64) -> Option<Level> {
    pacer.level(now)
}

#[test]
fn frames_start_unpaced_until_the_display_reports_one() {
    let mut pacer = FramePacer::default();
    assert!(pacer.slot_open(VSYNC + 100_000, VSYNC, VSYNC));
    assert!(begin_at(&mut pacer, VSYNC));
    assert!(
        pacer.slot_open(VSYNC + VSYNC / 2, VSYNC, VSYNC),
        "an unpaced loop starts each frame as soon as the renderer can take it"
    );
    assert!(pacer.begin_frame(VSYNC + VSYNC / 2, VSYNC, VSYNC));
    pacer.record_shown(VSYNC, 2, VSYNC);
    assert_eq!(level(&mut pacer, VSYNC), Some(Level::Unpaced));
    assert!(begin_at(&mut pacer, 2 * VSYNC));
    assert!(pacer.begin_frame(2 * VSYNC + VSYNC / 2, 2 * VSYNC, VSYNC));
}

#[test]
fn a_loop_that_leaves_the_queue_shallow_stays_unpaced() {
    let mut pacer = FramePacer::default();
    let now = shown_run(&mut pacer, 0, 60, 2);
    assert_eq!(
        level(&mut pacer, now + 60 * FIRST_HOLD_NS),
        Some(Level::Unpaced)
    );
}

#[test]
fn a_loop_that_fills_the_queue_is_paced_at_two_frames_queued() {
    let mut pacer = FramePacer::default();
    let now = shown_run(&mut pacer, 0, FULL_HISTORY as i64 - 1, 3);
    let now = shown_run(&mut pacer, now, 1, 2);
    let now = shown_run(&mut pacer, now, FULL_HISTORY as i64 - 1, 3);
    assert_eq!(
        level(&mut pacer, now),
        Some(Level::Unpaced),
        "a queue that fell to two now and then is not full"
    );
    let now = shown_run(&mut pacer, now, 1, 3);
    assert_eq!(level(&mut pacer, now), Some(Level::Buffered));
    assert!(
        !begin_at(&mut pacer, now),
        "the first paced slot drains the full queue"
    );
    assert!(begin_at(&mut pacer, now + VSYNC));
    assert!(!pacer.begin_frame(now + VSYNC * 3 / 2, now + VSYNC, VSYNC));
}

#[test]
fn one_frame_starts_per_vsync() {
    let mut pacer = shallow();
    let vsync = SETTLED + VSYNC;
    assert!(begin_at(&mut pacer, vsync));
    assert!(
        !pacer.begin_frame(vsync + VSYNC / 2, vsync, VSYNC),
        "a wake within the same vsync must not start a second frame"
    );
    assert!(begin_at(&mut pacer, vsync + VSYNC));
}

#[test]
fn a_late_frame_starts_its_successor_once_the_next_slot_opened() {
    let mut pacer = shallow();
    let vsync = SETTLED + VSYNC;
    assert!(begin_at(&mut pacer, vsync));
    assert!(
        pacer.begin_frame(vsync + VSYNC + VSYNC / 5, vsync, VSYNC),
        "a frame that ran past the next vsync must not wait for another"
    );
    assert!(!pacer.begin_frame(vsync + VSYNC + VSYNC / 2, vsync + VSYNC, VSYNC));
}

#[test]
fn a_wake_just_before_the_vsync_counts_as_its_slot() {
    let mut pacer = shallow();
    let vsync = SETTLED + VSYNC;
    assert!(begin_at(&mut pacer, vsync));
    assert!(pacer.begin_frame(vsync + VSYNC - SLOT_TOLERANCE_NS / 2, vsync, VSYNC));
}

#[test]
fn a_vsync_that_drifts_from_the_extrapolated_grid_still_opens_its_slot() {
    let mut pacer = shallow();
    let vsync = SETTLED + VSYNC;
    assert!(pacer.begin_frame(vsync + VSYNC + VSYNC / 5, vsync, VSYNC));
    let early_callback = vsync + VSYNC - 200_000;
    assert!(
        !pacer.begin_frame(early_callback + 100_000, early_callback, VSYNC),
        "the callback for a slot already started must not open it again"
    );
    assert!(begin_at(&mut pacer, early_callback + VSYNC));
}

#[test]
fn without_a_known_vsync_frames_are_not_held() {
    let mut pacer = shallow();
    assert!(pacer.begin_frame(SETTLED + 1_000, 0, VSYNC));
    assert!(pacer.begin_frame(SETTLED + 2_000, 0, VSYNC));
}

#[test]
fn a_slot_is_open_until_a_frame_starts_in_it() {
    let mut pacer = shallow();
    let vsync = SETTLED + VSYNC;
    let now = vsync + 100_000;
    assert!(pacer.slot_open(now, vsync, VSYNC));
    assert!(pacer.begin_frame(now, vsync, VSYNC));
    assert!(!pacer.slot_open(now, vsync, VSYNC));
    assert!(
        pacer.slot_open(vsync + VSYNC + 100_000, vsync, VSYNC),
        "a frame that ran past the vsync leaves the next slot open"
    );
}

#[test]
fn a_queue_deeper_than_meant_behind_every_shown_frame_gives_up_one_slot() {
    let mut pacer = shallow();
    let now = shown_run(&mut pacer, SETTLED + VSYNC, 3, 2);
    assert!(
        !begin_at(&mut pacer, now),
        "a full queue gives up this slot"
    );
    assert!(
        !pacer.begin_frame(now + VSYNC / 2, now, VSYNC),
        "and no other frame takes it"
    );
    assert!(
        begin_at(&mut pacer, now + VSYNC),
        "a skip needs fresh evidence the queue is still full"
    );
}

#[test]
fn a_queue_as_deep_as_meant_is_left_alone() {
    let mut pacer = shallow();
    let now = shown_run(&mut pacer, SETTLED + VSYNC, 2, 2);
    let now = shown_run(&mut pacer, now, 1, 1);
    assert!(begin_at(&mut pacer, now));
}

#[test]
fn frames_shown_long_ago_say_nothing_about_the_queue_now() {
    let mut pacer = shallow();
    let now = shown_run(&mut pacer, SETTLED + VSYNC, 3, 2);
    assert!(begin_at(&mut pacer, now + SHOWN_FRESH_VSYNCS * VSYNC));
}

#[test]
fn slots_are_given_up_at_least_half_a_second_apart() {
    let mut pacer = shallow();
    let now = shown_run(&mut pacer, SETTLED + VSYNC, 3, 2);
    assert!(!begin_at(&mut pacer, now));
    let soon = shown_run(&mut pacer, now, 3, 2);
    assert!(begin_at(&mut pacer, soon));
    let later = shown_run(&mut pacer, now + SKIP_SPACING_NS, 3, 2);
    assert!(!begin_at(&mut pacer, later));
}

#[test]
fn a_shallow_queue_buffers_a_frame_at_its_first_missed_vsync() {
    let mut pacer = shallow();
    let now = shown_run(&mut pacer, SETTLED + VSYNC, 30, 1);
    assert_eq!(level(&mut pacer, now), Some(Level::Shallow));
    let now = missed_run(&mut pacer, now, 1);
    assert_eq!(level(&mut pacer, now), Some(Level::Buffered));
}

#[test]
fn a_buffered_queue_starts_an_extra_frame_while_it_holds_fewer_than_two() {
    let mut pacer = buffered();
    let now = shown_run(&mut pacer, STUFFED, 1, 1);
    assert!(begin_at(&mut pacer, now));
    assert!(
        pacer.begin_frame(now + VSYNC / 2, now, VSYNC),
        "a queue one frame deep takes a second frame in the same slot"
    );
    assert!(
        !pacer.begin_frame(now + VSYNC * 3 / 4, now, VSYNC),
        "and only one until the display reports it"
    );
    let now = shown_run(&mut pacer, now, 1, 2);
    assert!(begin_at(&mut pacer, now));
    assert!(
        !pacer.begin_frame(now + VSYNC / 2, now, VSYNC),
        "two deep is enough"
    );
}

#[test]
fn a_buffered_queue_that_keeps_missing_vsyncs_goes_unpaced() {
    let mut pacer = shallow();
    let now = missed_run(&mut pacer, SETTLED + VSYNC, 1);
    assert_eq!(level(&mut pacer, now), Some(Level::Buffered));
    let now = missed_run(&mut pacer, now + VSYNC, 2);
    assert_eq!(
        level(&mut pacer, now),
        Some(Level::Buffered),
        "two misses are not enough"
    );
    let now = missed_run(&mut pacer, now + VSYNC, 1);
    assert_eq!(level(&mut pacer, now), Some(Level::Unpaced));
    assert!(pacer.begin_frame(now + VSYNC, now, VSYNC));
    assert!(pacer.begin_frame(now + VSYNC + 1, now, VSYNC));
    assert!(pacer.slot_open(now + VSYNC + 2, now, VSYNC));
}

#[test]
fn the_level_below_is_tried_after_the_hold() {
    let mut pacer = buffered();
    let back = STUFFED + FIRST_HOLD_NS;
    assert_eq!(level(&mut pacer, back - 1), Some(Level::Buffered));
    assert_eq!(level(&mut pacer, back), Some(Level::Shallow));
    let failed = missed_run(&mut pacer, back, 1);
    assert_eq!(level(&mut pacer, failed), Some(Level::Buffered));
    assert_eq!(
        level(&mut pacer, failed + FIRST_HOLD_NS),
        Some(Level::Shallow),
        "a level failing for the first time does not lengthen the hold"
    );
}

#[test]
fn a_retry_gives_up_at_its_first_miss_and_waits_four_times_as_long_for_the_next() {
    let mut pacer = buffered();
    let back = STUFFED + FIRST_HOLD_NS;
    assert_eq!(level(&mut pacer, back), Some(Level::Shallow));
    let failed = missed_run(&mut pacer, back, 2);
    let retry = failed + FIRST_HOLD_NS;
    assert_eq!(level(&mut pacer, retry), Some(Level::Shallow));
    let failed_again = missed_run(&mut pacer, retry, 1);
    assert_eq!(level(&mut pacer, failed_again), Some(Level::Buffered));
    assert_eq!(
        level(&mut pacer, failed_again + 4 * FIRST_HOLD_NS - 1),
        Some(Level::Buffered)
    );
    assert_eq!(
        level(&mut pacer, failed_again + 4 * FIRST_HOLD_NS),
        Some(Level::Shallow)
    );
}

#[test]
fn a_level_that_held_for_a_while_waits_the_first_hold_again() {
    let mut pacer = shallow();
    let failed = missed_run(&mut pacer, SETTLED + VSYNC, 2);
    assert_eq!(
        level(&mut pacer, failed + FIRST_HOLD_NS),
        Some(Level::Shallow)
    );
}

#[test]
fn an_unpaced_loop_comes_down_only_once_its_hold_is_over_and_the_queue_is_full() {
    let mut pacer = shallow();
    let now = missed_run(&mut pacer, SETTLED + VSYNC, 2);
    let now = missed_run(&mut pacer, now + VSYNC, 3);
    assert_eq!(level(&mut pacer, now), Some(Level::Unpaced));
    let now = shown_run(&mut pacer, now + VSYNC, FULL_HISTORY as i64, 3);
    assert_eq!(
        level(&mut pacer, now),
        Some(Level::Unpaced),
        "a full queue waits for the hold"
    );
    let over = now + FIRST_HOLD_NS;
    assert_eq!(level(&mut pacer, over), Some(Level::Buffered));
    assert_eq!(
        level(&mut pacer, over + FIRST_HOLD_NS),
        Some(Level::Shallow)
    );
}

#[test]
fn a_shown_frame_counts_the_presents_queued_behind_it() {
    let mut log = PresentLog::default();
    for id in 0..4 {
        log.record(id, i64::from(id) * VSYNC);
    }
    assert_eq!(
        log.queued_behind(0, VSYNC * 5 / 2),
        Some(2),
        "presents 1 and 2 had returned by then; 3 had not"
    );
    assert_eq!(
        log.queued_behind(0, 0),
        None,
        "a reported frame is forgotten"
    );
    assert_eq!(log.queued_behind(3, 4 * VSYNC), Some(0));
}

#[test]
fn a_present_log_forgets_the_oldest_beyond_its_capacity() {
    let mut log = PresentLog::default();
    let capacity = u32::try_from(PRESENT_LOG).expect("the log is small");
    for id in 0..=capacity {
        log.record(id, i64::from(id));
    }
    assert_eq!(log.queued_behind(0, i64::MAX), None);
    assert_eq!(log.queued_behind(1, 1), Some(0));
}
