use super::outer_size_for_inner;

#[test]
fn a_floating_window_grows_by_its_own_frame_so_the_canvas_gets_the_requested_size() {
    assert_eq!(
        outer_size_for_inner((275.0, 493.0), (291.0, 541.0), (275.0, 493.0)),
        (291, 541)
    );
    assert_eq!(
        outer_size_for_inner((960.0, 720.0), (291.0, 541.0), (275.0, 493.0)),
        (976, 768),
        "the frame measured at one size is the frame at every size"
    );
}

#[test]
fn a_window_that_reports_no_frame_gets_the_requested_size_itself() {
    assert_eq!(
        outer_size_for_inner((275.5, 492.6), (400.0, 300.0), (400.0, 300.0)),
        (276, 493)
    );
}

#[test]
fn a_window_reporting_a_smaller_outside_than_inside_is_given_no_negative_frame() {
    assert_eq!(
        outer_size_for_inner((275.0, 493.0), (300.0, 300.0), (310.0, 320.0)),
        (275, 493)
    );
}
