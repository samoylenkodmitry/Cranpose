use std::time::{Duration, Instant};

use super::SceneHold;

fn after(start: Instant, seconds: f32) -> Instant {
    start + Duration::from_secs_f32(seconds)
}

#[test]
fn the_scene_lets_its_window_go_once_five_seconds_after_its_last_content() {
    let start = Instant::now();
    let mut hold = SceneHold::default();
    assert!(
        !hold.frame(0, start),
        "a scene that never drew holds nothing"
    );
    assert!(!hold.frame(1, start));
    assert!(!hold.frame(0, after(start, 1.0)));
    assert!(!hold.frame(0, after(start, 5.9)));
    assert!(hold.frame(0, after(start, 6.0)));
    assert!(!hold.frame(0, after(start, 20.0)), "the hold ends once");
}

#[test]
fn content_that_comes_back_within_the_hold_counts_the_hold_from_its_own_end() {
    let start = Instant::now();
    let mut hold = SceneHold::default();
    assert!(!hold.frame(1, start));
    assert!(!hold.frame(0, after(start, 1.0)));
    assert!(!hold.frame(1, after(start, 4.0)));
    assert!(!hold.frame(0, after(start, 5.0)));
    assert!(!hold.frame(0, after(start, 9.9)));
    assert!(hold.frame(0, after(start, 10.0)));
}
