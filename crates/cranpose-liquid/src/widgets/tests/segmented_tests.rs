use super::*;

#[test]
fn pointer_position_is_the_clamped_lens_center() {
    let width = 100.0;
    assert_eq!(segment_lens_left(50.0, width, 3), 0.0);
    assert_eq!(segment_lens_left(150.0, width, 3), 100.0);
    assert_eq!(segment_lens_left(250.0, width, 3), 200.0);
    assert_eq!(segment_lens_left(-50.0, width, 3), 0.0);
    assert_eq!(segment_lens_left(400.0, width, 3), 200.0);
}

#[test]
fn raised_lens_lifts_in_depth_without_becoming_a_wide_worm() {
    let resting = segmented_lens_base_size(120.0, 0.0);
    let raised = segmented_lens_base_size(120.0, 1.0);
    assert_eq!(resting.width, 120.0 * MARKER_WIDTH_FACTOR);
    assert!(resting.height > SEGMENT_HEIGHT + TRACK_PADDING * 2.0);
    assert!(raised.width < resting.width * 1.10);
    assert!(raised.height > resting.height * 1.20);
    assert!(segmented_strain(crate::dynamics::STRETCH_MAX) < 1.20);
}

#[test]
fn a_scope_records_what_each_segment_announces() {
    let drawn = Rc::new(std::cell::Cell::new(0u32));
    let counted = Rc::clone(&drawn);
    let segments = collect_segments(|scope| {
        scope.segment("Sending");
        scope.segment_content("Received", move |selected| {
            counted.set(counted.get() + u32::from(selected));
        });
    });

    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].description, "Sending");
    assert_eq!(segments[1].description, "Received");
    assert_eq!(drawn.get(), 0);
    (segments[1].content)(true);
    assert_eq!(drawn.get(), 1);
}

#[test]
fn a_control_with_no_segments_declares_none() {
    assert!(collect_segments(|_| {}).is_empty());
}
