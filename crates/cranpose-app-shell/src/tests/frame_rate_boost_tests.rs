use std::time::Duration;

use web_time::Instant;

use super::*;

const REDRAWN: FrameUpdateResult = FrameUpdateResult {
    visual_changed: true,
    structure_changed: false,
    content_moved: false,
    content_redrawn: true,
};

#[test]
fn input_boosts_for_the_hold_off() {
    let start = Instant::now();
    let mut boost = FrameRateBoost::default();
    assert!(!boost.boosted(start));
    boost.note_input(start);
    assert!(boost.boosted(start + Duration::from_millis(2_900)));
    assert!(!boost.boosted(start + Duration::from_millis(3_000)));
}

#[test]
fn a_frame_that_moved_content_boosts() {
    let start = Instant::now();
    let mut boost = FrameRateBoost::default();
    boost.note_frame(
        FrameUpdateResult {
            content_moved: true,
            ..FrameUpdateResult::default()
        },
        start,
    );
    assert!(boost.boosted(start));
}

#[test]
fn continuous_redraws_boost_from_the_third_frame() {
    let start = Instant::now();
    let mut boost = FrameRateBoost::default();
    boost.note_frame(REDRAWN, start);
    boost.note_frame(REDRAWN, start + Duration::from_millis(16));
    assert!(
        !boost.boosted(start + Duration::from_millis(16)),
        "two frames make one interval, not a continuous redraw"
    );
    boost.note_frame(REDRAWN, start + Duration::from_millis(33));
    assert!(boost.boosted(start + Duration::from_millis(33)));
}

#[test]
fn intermittent_redraws_never_boost() {
    let start = Instant::now();
    let mut boost = FrameRateBoost::default();
    for blink in 0..8 {
        boost.note_frame(REDRAWN, start + Duration::from_millis(500) * blink);
    }
    assert!(!boost.boosted(start + Duration::from_millis(3_500)));
    boost.note_frame(REDRAWN, start + Duration::from_millis(4_000));
    boost.note_frame(REDRAWN, start + Duration::from_millis(4_060));
    assert!(
        !boost.boosted(start + Duration::from_millis(4_060)),
        "a blink then a redraw 60 ms later still spans more than 100 ms over two intervals"
    );
}

#[test]
fn a_frame_that_only_presents_does_not_count_as_a_redraw() {
    let start = Instant::now();
    let mut boost = FrameRateBoost::default();
    for frame in 0..6 {
        boost.note_frame(
            FrameUpdateResult {
                visual_changed: true,
                ..FrameUpdateResult::default()
            },
            start + Duration::from_millis(8) * frame,
        );
    }
    assert!(!boost.boosted(start + Duration::from_millis(40)));
}
