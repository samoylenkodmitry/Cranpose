use super::*;

#[test]
fn the_limit_is_what_the_runtime_reports() {
    assert_eq!(LayerLimit::reported(32).layers(), 32);
}

#[test]
fn a_runtime_that_reports_none_gets_the_guaranteed_number() {
    assert_eq!(LayerLimit::reported(0).layers(), 16);
}

#[test]
fn a_refused_frame_drops_the_limit_to_the_guaranteed_number_once() {
    let mut limit = LayerLimit::reported(32);
    assert!(limit.refused(18));
    assert_eq!(limit.layers(), 16);
    assert!(
        !limit.refused(18),
        "the limit cannot fall below the guarantee"
    );
}

#[test]
fn a_refused_frame_of_the_guaranteed_number_or_fewer_changes_nothing() {
    let mut limit = LayerLimit::reported(32);
    assert!(!limit.refused(16));
    assert_eq!(limit.layers(), 32);
}
