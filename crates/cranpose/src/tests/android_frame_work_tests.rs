use super::frame_work_ns;

#[test]
fn a_frame_rendered_on_the_loop_counts_its_loop_and_render_parts() {
    assert_eq!(frame_work_ns(1_000, 4_000, 4_500, 7_500), Some(6_000));
}

#[test]
fn waits_for_a_buffer_and_for_the_frame_before_are_not_work() {
    let queued_behind_the_last_frame = frame_work_ns(1_000, 3_000, 20_000, 22_000);
    assert_eq!(queued_behind_the_last_frame, Some(4_000));
}

#[test]
fn a_frame_missing_a_timestamp_reports_nothing() {
    assert_eq!(frame_work_ns(0, 3_000, 4_000, 5_000), None);
    assert_eq!(frame_work_ns(1_000, 3_000, 0, 5_000), None);
    assert_eq!(frame_work_ns(1_000, 3_000, 4_000, 0), None);
    assert_eq!(frame_work_ns(1_000, 0, 4_000, 5_000), None);
}
