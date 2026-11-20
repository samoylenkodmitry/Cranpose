use std::{cell::Cell, time::Duration};

use crate::{
    Entitlement, OnNotEntitled,
    entitlement_watch::{Answer, DEADLINE, EntitlementWatch},
};

const SECOND: Duration = Duration::from_secs(1);

// A watch that the platform answers `answer` at every ask, with the count of the asks.
fn step(
    watch: &mut EntitlementWatch,
    now: Duration,
    policy: OnNotEntitled,
    answer: Answer,
    asks: &Cell<u32>,
) -> Option<crate::entitlement_watch::Verdict> {
    watch.step(now, policy, || {
        asks.set(asks.get() + 1);
        answer
    })
}

#[test]
fn an_entitled_viewer_ends_the_check_with_no_quit() {
    let (mut watch, asks) = (EntitlementWatch::default(), Cell::new(0));
    let verdict = step(
        &mut watch,
        SECOND,
        OnNotEntitled::Quit,
        Answer::Entitled,
        &asks,
    )
    .unwrap();
    assert_eq!(verdict.entitlement, Entitlement::Entitled);
    assert!(!verdict.quit && !verdict.timed_out);
    assert!(watch.settled());
}

#[test]
fn a_failed_check_quits_by_default_and_goes_on_by_the_apps_choice() {
    for (policy, quit) in [
        (OnNotEntitled::Quit, true),
        (OnNotEntitled::Continue, false),
    ] {
        let (mut watch, asks) = (EntitlementWatch::default(), Cell::new(0));
        let verdict = step(&mut watch, SECOND, policy, Answer::NotEntitled, &asks).unwrap();
        assert_eq!(verdict.entitlement, Entitlement::NotEntitled);
        assert_eq!(verdict.quit, quit, "{policy:?}");
        assert!(!verdict.timed_out);
    }
}

#[test]
fn a_running_check_asks_a_few_times_a_second_and_reports_nothing() {
    let (mut watch, asks) = (EntitlementWatch::default(), Cell::new(0));
    // 120 turns in one second, as a headset at 120 Hz runs them
    for turn in 0..120 {
        let now = Duration::from_micros(turn * 8_333);
        assert_eq!(
            step(
                &mut watch,
                now,
                OnNotEntitled::Quit,
                Answer::Checking,
                &asks
            ),
            None
        );
    }
    assert!(!watch.settled());
    assert!(
        (9..=11).contains(&asks.get()),
        "asked {} times in a second",
        asks.get()
    );
}

#[test]
fn a_check_that_gets_no_answer_in_time_fails() {
    let (mut watch, asks) = (EntitlementWatch::default(), Cell::new(0));
    let just_before = DEADLINE - Duration::from_millis(1);
    assert_eq!(
        step(
            &mut watch,
            just_before,
            OnNotEntitled::Quit,
            Answer::Checking,
            &asks
        ),
        None
    );
    // The next ask is one interval later, which is past the deadline
    let verdict = step(
        &mut watch,
        just_before + Duration::from_millis(100),
        OnNotEntitled::Quit,
        Answer::Checking,
        &asks,
    )
    .unwrap();
    assert_eq!(verdict.entitlement, Entitlement::NotEntitled);
    assert!(verdict.quit && verdict.timed_out);
}

#[test]
fn a_late_answer_at_the_deadline_is_the_answer_not_a_timeout() {
    let (mut watch, asks) = (EntitlementWatch::default(), Cell::new(0));
    let verdict = step(
        &mut watch,
        DEADLINE,
        OnNotEntitled::Quit,
        Answer::Entitled,
        &asks,
    )
    .unwrap();
    assert_eq!(verdict.entitlement, Entitlement::Entitled);
    assert!(!verdict.timed_out);
}

#[test]
fn a_settled_check_asks_nothing_more() {
    let (mut watch, asks) = (EntitlementWatch::default(), Cell::new(0));
    step(
        &mut watch,
        SECOND,
        OnNotEntitled::Quit,
        Answer::NotEntitled,
        &asks,
    )
    .unwrap();
    for turn in 2..50 {
        assert_eq!(
            step(
                &mut watch,
                turn * SECOND,
                OnNotEntitled::Quit,
                Answer::Entitled,
                &asks
            ),
            None
        );
    }
    assert_eq!(
        asks.get(),
        1,
        "the outcome is the first one and it is final"
    );
}

#[test]
fn the_choice_of_the_policy_is_read_when_the_answer_comes() {
    let (mut watch, asks) = (EntitlementWatch::default(), Cell::new(0));
    assert_eq!(
        step(
            &mut watch,
            Duration::ZERO,
            OnNotEntitled::Quit,
            Answer::Checking,
            &asks
        ),
        None
    );
    // The app chose to go on while the check ran
    let verdict = step(
        &mut watch,
        SECOND,
        OnNotEntitled::Continue,
        Answer::NotEntitled,
        &asks,
    )
    .unwrap();
    assert!(!verdict.quit);
}
