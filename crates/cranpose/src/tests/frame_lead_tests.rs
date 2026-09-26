use super::*;

const PERIOD: i64 = 8_333_333;

/// The lead at `index` of [`LEADS_TENTHS`], in nanoseconds of [`PERIOD`].
fn lead_at(index: usize) -> i64 {
    PERIOD * LEADS_TENTHS[index] / 10
}

/// Records `frames` frames each `latency_ns` from queued to shown, the last
/// shown at `now_ns`.
fn run(lead: &mut FrameLead, frames: i64, latency_ns: i64, now_ns: i64) {
    for _ in 0..frames {
        lead.record(latency_ns, now_ns);
    }
}

/// Runs a trial's settle and window at `latency_ns`, ending at `now_ns`.
fn run_trial(lead: &mut FrameLead, latency_ns: i64, now_ns: i64) {
    run(lead, i64::from(SETTLE) + WINDOW, latency_ns, now_ns);
}

/// A lead whose first window is done and whose trial of the next lead has
/// just begun.
fn in_trial(latency_ns: i64) -> FrameLead {
    let mut lead = FrameLead::default();
    run(&mut lead, WINDOW, latency_ns, 0);
    assert_eq!(lead.lead_ns(PERIOD), lead_at(1));
    lead
}

#[test]
fn the_first_window_is_followed_by_a_trial_of_the_next_lead() {
    let mut lead = FrameLead::default();
    assert_eq!(lead.lead_ns(PERIOD), 0);
    run(&mut lead, WINDOW - 1, 20_000_000, 0);
    assert_eq!(lead.lead_ns(PERIOD), 0, "the window is not over");
    lead.record(20_000_000, 0);
    assert_eq!(lead.lead_ns(PERIOD), lead_at(1));
}

#[test]
fn a_trial_that_shows_frames_sooner_keeps_its_lead() {
    let mut lead = in_trial(20_000_000);
    run_trial(&mut lead, 12_000_000, 1);
    assert_eq!(lead.lead_ns(PERIOD), lead_at(1));
    run(&mut lead, WINDOW, 12_000_000, 1 + FIRST_HOLD_NS);
    assert_eq!(
        lead.lead_ns(PERIOD),
        lead_at(2),
        "after its hold the kept lead gives way to a trial of the next"
    );
}

#[test]
fn a_failed_trial_reverts_and_doubles_the_hold_before_another_lead() {
    let mut lead = in_trial(20_000_000);
    run_trial(&mut lead, 20_000_000 - MARGIN_NS / 2, 1);
    assert_eq!(lead.lead_ns(PERIOD), 0, "less than the margin is noise");
    run_trial(&mut lead, 20_000_000, 1 + FIRST_HOLD_NS);
    assert_eq!(lead.lead_ns(PERIOD), 0, "the failure doubled the hold");
    run(&mut lead, WINDOW, 20_000_000, 1 + 2 * FIRST_HOLD_NS);
    assert_eq!(
        lead.lead_ns(PERIOD),
        lead_at(2),
        "the next trial takes the next lead"
    );
}

#[test]
fn a_longer_lead_is_found_past_a_shorter_one_that_does_not_help() {
    let mut lead = in_trial(33_000_000);
    run_trial(&mut lead, 34_000_000, 1);
    run_trial(&mut lead, 33_000_000, 1 + 2 * FIRST_HOLD_NS);
    assert_eq!(lead.lead_ns(PERIOD), lead_at(2));
    run_trial(&mut lead, 22_000_000, 2 + 2 * FIRST_HOLD_NS);
    assert_eq!(
        lead.lead_ns(PERIOD),
        lead_at(2),
        "the frames that reached the screen a refresh sooner keep their lead"
    );
}

#[test]
fn trials_take_every_other_lead_in_turn() {
    let mut lead = in_trial(20_000_000);
    let mut now = 1;
    let mut hold = FIRST_HOLD_NS;
    for next in [2, 3, 1] {
        run_trial(&mut lead, 21_000_000, now);
        hold *= 2;
        now += hold;
        run_trial(&mut lead, 20_000_000, now);
        assert_eq!(lead.lead_ns(PERIOD), lead_at(next));
    }
}

#[test]
fn reset_abandons_a_trial_and_its_window() {
    let mut lead = in_trial(20_000_000);
    lead.reset();
    assert_eq!(lead.lead_ns(PERIOD), 0);
    run(&mut lead, i64::from(SETTLE) + WINDOW - 1, 20_000_000, 0);
    assert_eq!(lead.lead_ns(PERIOD), 0, "the settle and window start over");
    lead.record(20_000_000, 0);
    assert_eq!(lead.lead_ns(PERIOD), lead_at(2));
}
