// The clock of an animation: playing, pausing, the end of a run, looping, speed and reverse.

use super::*;

fn clock(looping: bool, speed: f32) -> Clock {
    Clock {
        time: 0.0,
        posing: true,
        looping,
        speed,
    }
}

#[test]
fn a_clock_runs_forward_by_the_step_times_its_speed_and_ends_at_the_clips_end() {
    let mut clock = clock(false, 2.0);
    assert!(!clock.step(0.5, 4.0));
    assert_eq!(clock.time, 1.0);
    assert!(!clock.step(1.0, 4.0));
    assert_eq!(clock.time, 3.0);
    // The run reaches the end and holds there
    assert!(clock.step(1.0, 4.0));
    assert_eq!(clock.time, 4.0);
}

#[test]
fn a_looping_clock_wraps_and_never_ends() {
    let mut clock = clock(true, 1.0);
    assert!(!clock.step(5.5, 2.0));
    assert!((clock.time - 1.5).abs() < 1e-5, "{}", clock.time);
    clock.speed = -1.0;
    assert!(!clock.step(2.0, 2.0));
    assert!(
        (clock.time - 1.5).abs() < 1e-5,
        "backward by a whole clip: {}",
        clock.time
    );
    assert!(!clock.step(1.0, 2.0));
    assert!((clock.time - 0.5).abs() < 1e-5, "{}", clock.time);
}

#[test]
fn a_clock_at_speed_zero_holds_and_a_backward_run_from_the_start_ends_at_once() {
    let mut held = clock(false, 0.0);
    assert!(!held.step(10.0, 2.0));
    assert_eq!(held.time, 0.0);
    let mut backward = clock(false, -1.0);
    assert!(backward.step(0.1, 2.0));
    assert_eq!(backward.time, 0.0);
}
