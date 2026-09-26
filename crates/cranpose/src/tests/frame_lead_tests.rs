use super::*;

const PERIOD: i64 = 8_333_333;

/// Records `frames` frames each `latency_ns` from queued to shown, the last
/// shown at `now_ns`.
fn run(lead: &mut FrameLead, frames: i64, latency_ns: i64, now_ns: i64) {
    for _ in 0..frames {
        lead.record(latency_ns, now_ns);
    }
}

/// A lead whose first window is done and whose trial has just begun.
fn in_trial(latency_ns: i64) -> FrameLead {
    let mut lead = FrameLead::default();
    run(&mut lead, WINDOW, latency_ns, 0);
    assert_eq!(lead.lead_ns(PERIOD), PERIOD * LEAD_TENTHS / 10);
    lead
}

#[test]
fn the_first_window_is_followed_by_a_trial_of_the_lead() {
    let mut lead = FrameLead::default();
    assert_eq!(lead.lead_ns(PERIOD), 0);
    run(&mut lead, WINDOW - 1, 20_000_000, 0);
    assert_eq!(lead.lead_ns(PERIOD), 0, "the window is not over");
    lead.record(20_000_000, 0);
    assert_eq!(lead.lead_ns(PERIOD), PERIOD * LEAD_TENTHS / 10);
}

#[test]
fn a_trial_that_shows_frames_sooner_keeps_its_lead() {
    let mut lead = in_trial(20_000_000);
    run(&mut lead, i64::from(SETTLE) + WINDOW, 12_000_000, 1);
    assert_eq!(lead.lead_ns(PERIOD), PERIOD * LEAD_TENTHS / 10);
    run(&mut lead, WINDOW, 12_000_000, 1 + FIRST_HOLD_NS);
    assert_eq!(
        lead.lead_ns(PERIOD),
        0,
        "after its hold the kept lead gives way to a trial without one"
    );
}

#[test]
fn a_trial_that_gains_less_than_the_margin_reverts_and_waits_longer() {
    let mut lead = in_trial(20_000_000);
    run(
        &mut lead,
        i64::from(SETTLE) + WINDOW,
        20_000_000 - MARGIN_NS / 2,
        1,
    );
    assert_eq!(lead.lead_ns(PERIOD), 0);
    run(
        &mut lead,
        i64::from(SETTLE) + WINDOW,
        20_000_000,
        FIRST_HOLD_NS,
    );
    assert_eq!(
        lead.lead_ns(PERIOD),
        0,
        "a failed trial doubles the hold before the next"
    );
    run(&mut lead, WINDOW, 20_000_000, 1 + 2 * FIRST_HOLD_NS);
    assert_eq!(lead.lead_ns(PERIOD), PERIOD * LEAD_TENTHS / 10);
}

#[test]
fn reset_abandons_a_trial_and_its_window() {
    let mut lead = in_trial(20_000_000);
    lead.reset();
    assert_eq!(lead.lead_ns(PERIOD), 0);
    run(&mut lead, i64::from(SETTLE) + WINDOW - 1, 20_000_000, 0);
    assert_eq!(lead.lead_ns(PERIOD), 0, "the settle and window start over");
    lead.record(20_000_000, 0);
    assert_eq!(lead.lead_ns(PERIOD), PERIOD * LEAD_TENTHS / 10);
}
