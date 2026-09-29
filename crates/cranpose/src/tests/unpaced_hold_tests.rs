use super::*;

const PERIOD: i64 = 16_666_667;
const MS: i64 = 1_000_000;

/// Records a window of frames that each waited `wait_ns`.
fn window(hold: &mut UnpacedHold, wait_ns: i64) {
    for _ in 0..WINDOW {
        hold.record_wait(wait_ns, PERIOD);
    }
}

#[test]
fn frames_that_wait_long_for_the_renderer_are_held_back_by_half_the_surplus() {
    let mut hold = UnpacedHold::default();
    assert_eq!(hold.hold_ns(), 0);
    window(&mut hold, 12 * MS);
    assert_eq!(hold.hold_ns(), 5 * MS);
}

#[test]
fn the_hold_settles_where_the_shortest_waits_meet_the_target() {
    // The renderer frees up 12 ms after a frame is handed off unheld, so a
    // frame held `h` waits `12 ms - h`.
    let mut hold = UnpacedHold::default();
    for _ in 0..40 {
        let wait = 12 * MS - hold.hold_ns();
        window(&mut hold, wait);
    }
    assert!(
        (hold.hold_ns() - (12 * MS - TARGET_WAIT_NS)).abs() < MS / 100,
        "{}",
        hold.hold_ns()
    );
}

#[test]
fn a_window_whose_shortest_waits_fall_short_shrinks_the_hold_by_the_whole_shortfall() {
    let mut hold = UnpacedHold::default();
    window(&mut hold, 12 * MS);
    assert_eq!(hold.hold_ns(), 5 * MS);
    // The tenth-shortest wait moves the hold: three frames of thirty that
    // found the renderer free leave it at 6 ms, a surplus of 4.
    for index in 0..WINDOW {
        hold.record_wait(if index < 3 { 0 } else { 6 * MS }, PERIOD);
    }
    assert_eq!(hold.hold_ns(), 7 * MS);
    // A fourth takes it to nothing: the whole 2 ms shortfall comes off.
    for index in 0..WINDOW {
        hold.record_wait(if index < 4 { 0 } else { 6 * MS }, PERIOD);
    }
    assert_eq!(hold.hold_ns(), 7 * MS - TARGET_WAIT_NS);
}

#[test]
fn the_hold_stays_within_a_refresh_period_and_never_goes_negative() {
    let mut hold = UnpacedHold::default();
    for _ in 0..10 {
        window(&mut hold, 200 * MS);
    }
    assert_eq!(hold.hold_ns(), PERIOD);
    for _ in 0..20 {
        window(&mut hold, 0);
    }
    assert_eq!(hold.hold_ns(), 0);
}

#[test]
fn reset_stops_holding_and_forgets_the_window_in_progress() {
    let mut hold = UnpacedHold::default();
    window(&mut hold, 12 * MS);
    for _ in 0..WINDOW - 1 {
        hold.record_wait(12 * MS, PERIOD);
    }
    hold.reset();
    assert_eq!(hold.hold_ns(), 0);
    hold.record_wait(12 * MS, PERIOD);
    assert_eq!(hold.hold_ns(), 0, "a new window starts after a reset");
}
