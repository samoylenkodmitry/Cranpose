use super::*;

const MS: Duration = Duration::from_millis(1);

fn at(milliseconds: u64) -> Duration {
    MS * milliseconds as u32
}

#[test]
fn frames_with_no_target_show_the_newest_and_skip_the_rest() {
    let mut queue = FrameQueue::new();
    for id in 1..=3 {
        queue.push(id, None);
    }
    assert_eq!(queue.choose(at(100)), Some(3));
    assert_eq!(queue.choose(at(108)), None);
    assert_eq!(
        queue.counts(),
        StreamCounts {
            shown: 1,
            skipped: 2,
            repeated: 1
        }
    );
}

#[test]
fn a_30_hz_stream_on_a_120_hz_headset_shows_30_frames_and_repeats_90() {
    let mut queue = FrameQueue::new();
    // Frame k should reach the display at k * 33.3 ms and comes out of the decoder 10 ms before.
    // The headset's display times run 8.33 ms apart, and not in step with the frames.
    let target = |k: u64| Duration::from_micros(k * 33_333 + 5_000);
    let mut next = 0;
    for headset in 0..120_u64 {
        let display = Duration::from_micros(headset * 8_333);
        while next < 30 && target(next) < display + at(10) {
            queue.push(next, Some(target(next)));
            next += 1;
        }
        queue.choose(display);
    }
    let counts = queue.counts();
    assert_eq!(counts.skipped, 0);
    assert!((29..=31).contains(&counts.shown), "{counts:?}");
    assert_eq!(counts.shown + counts.repeated, 120);
}

#[test]
fn the_frame_closest_to_the_display_time_wins_and_older_frames_go() {
    let mut queue = FrameQueue::new();
    queue.push(1, Some(at(100)));
    queue.push(2, Some(at(133)));
    queue.push(3, Some(at(166)));
    // 140 ms is 7 ms from frame 2, which also drops frame 1. Frame 3 stays for a later frame.
    assert_eq!(queue.choose(at(140)), Some(2));
    assert_eq!(queue.counts().skipped, 1);
    assert_eq!(
        queue.choose(at(145)),
        None,
        "frame 2 is closer than frame 3"
    );
    assert_eq!(queue.choose(at(160)), Some(3));
}

#[test]
fn frames_with_targets_in_the_past_show_the_newest() {
    let mut queue = FrameQueue::new();
    for (id, target) in [(1, 10), (2, 20), (3, 30)] {
        queue.push(id, Some(at(target)));
    }
    assert_eq!(queue.choose(at(500)), Some(3));
    assert_eq!(queue.counts().skipped, 2);
}

#[test]
fn a_frame_shown_last_is_repeated_while_the_waiting_ones_are_farther() {
    let mut queue = FrameQueue::new();
    queue.push(1, Some(at(100)));
    assert_eq!(queue.choose(at(100)), Some(1));
    queue.push(2, Some(at(200)));
    queue.push(3, Some(at(233)));
    // 108 ms is 8 ms from frame 1 and 92 ms from frame 2
    assert_eq!(queue.choose(at(108)), None);
    assert_eq!(queue.counts().repeated, 1);
}

#[test]
fn a_full_queue_of_frames_ahead_of_the_display_does_not_freeze_the_picture() {
    let mut queue = FrameQueue::new();
    queue.push(0, Some(at(100)));
    assert_eq!(queue.choose(at(100)), Some(0));
    // The host's stamps run 200 ms ahead of the display times. The frame shown last stays
    // closer for a while, until the queue is full.
    for (id, target) in [(1, 300), (2, 333), (3, 366)] {
        queue.push(id, Some(at(target)));
    }
    assert_eq!(
        queue.choose(at(110)),
        Some(1),
        "a full queue lets go of the old frame"
    );
    // The queue holds frames 2 and 3. Frame 4 fills it, and frame 5 pushes frame 2 out.
    queue.push(4, Some(at(400)));
    queue.push(5, Some(at(433)));
    assert_eq!(queue.counts().skipped, 1);
    assert_eq!(queue.choose(at(120)), Some(3));
}

#[test]
fn frames_that_come_while_the_runtime_shows_nothing_are_skipped_and_the_last_picture_stays() {
    let mut queue = FrameQueue::new();
    queue.push(1, None);
    assert_eq!(queue.choose(at(0)), Some(1));
    queue.push(2, None);
    queue.push(3, None);
    queue.clear();
    queue.skipped();
    assert_eq!(queue.counts().skipped, 3);
    assert_eq!(queue.choose(at(10)), None);
    assert_eq!(queue.counts().repeated, 1);
}
