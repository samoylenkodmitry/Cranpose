use super::*;

#[test]
fn a_frame_holds_the_guaranteed_number_until_the_runtime_takes_more() {
    let limit = LayerLimit::reported(32);
    assert_eq!(limit.layers(), 16);
    assert_eq!(limit.trial(), Some(32));
}

#[test]
fn a_runtime_that_takes_the_trial_gets_the_number_that_it_reports() {
    let mut limit = LayerLimit::reported(32);
    limit.tried(true);
    assert_eq!(limit.layers(), 32);
    assert_eq!(limit.trial(), None);
}

#[test]
fn a_runtime_that_refuses_the_trial_keeps_the_guaranteed_number() {
    let mut limit = LayerLimit::reported(32);
    limit.tried(false);
    assert_eq!(limit.layers(), 16);
    assert_eq!(limit.trial(), None, "one frame tries");
}

#[test]
fn a_runtime_that_reports_the_guaranteed_number_or_none_has_no_trial() {
    for reported in [0, 16] {
        let limit = LayerLimit::reported(reported);
        assert_eq!(limit.layers(), 16);
        assert_eq!(limit.trial(), None);
    }
}

#[test]
fn a_refused_frame_after_the_trial_drops_the_limit_to_the_guaranteed_number_once() {
    let mut limit = LayerLimit::reported(32);
    limit.tried(true);
    assert!(!limit.refused(16), "nothing to drop");
    assert!(limit.refused(18));
    assert_eq!(limit.layers(), 16);
    assert!(
        !limit.refused(18),
        "the limit cannot fall below the guarantee"
    );
}
