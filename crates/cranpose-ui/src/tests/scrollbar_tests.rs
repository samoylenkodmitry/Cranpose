use super::*;

#[test]
fn content_that_fits_shows_no_thumb() {
    assert_eq!(thumb_geometry(100.0, 100.0, 0.0, ThumbBounds::FULL), None);
    assert_eq!(thumb_geometry(80.0, 100.0, 0.0, ThumbBounds::FULL), None);
    assert_eq!(thumb_geometry(500.0, 0.0, 0.0, ThumbBounds::FULL), None);
}

#[test]
fn a_measurement_that_is_not_a_number_shows_no_thumb() {
    assert_eq!(
        thumb_geometry(f32::NAN, 100.0, 0.0, ThumbBounds::FULL),
        None
    );
    assert_eq!(
        thumb_geometry(400.0, f32::INFINITY, 0.0, ThumbBounds::FULL),
        None
    );
    assert_eq!(
        thumb_geometry(400.0, 100.0, f32::NAN, ThumbBounds::FULL),
        None
    );
}

#[test]
fn the_thumb_is_the_share_of_content_on_screen_and_travels_with_it() {
    let top = thumb_geometry(400.0, 100.0, 0.0, ThumbBounds::FULL).expect("scrollable");
    assert_eq!(top.length, 0.25);
    assert_eq!(top.offset, 0.0);
    assert_eq!(top.progress(), 0.0);

    let bottom = thumb_geometry(400.0, 100.0, 300.0, ThumbBounds::FULL).expect("scrollable");
    assert_eq!(bottom.offset, 0.75);
    assert_eq!(bottom.end(), 1.0);
    assert_eq!(bottom.progress(), 1.0);

    let middle = thumb_geometry(400.0, 100.0, 150.0, ThumbBounds::FULL).expect("scrollable");
    assert_eq!(middle.offset, 0.375);
    assert_eq!(middle.progress(), 0.5);
}

#[test]
fn scrolling_past_either_end_stays_on_the_track() {
    let before = thumb_geometry(400.0, 100.0, -50.0, ThumbBounds::FULL).expect("scrollable");
    assert_eq!(before.offset, 0.0);
    let after = thumb_geometry(400.0, 100.0, 900.0, ThumbBounds::FULL).expect("scrollable");
    assert_eq!(after.end(), 1.0);
}

#[test]
fn a_very_long_list_still_shows_a_grabbable_thumb() {
    let bounds = ThumbBounds::at_least(24.0, 240.0);
    let geometry = thumb_geometry(100_000.0, 240.0, 0.0, bounds).expect("scrollable");
    assert_eq!(geometry.length, 0.1);

    let roomy = thumb_geometry(480.0, 240.0, 0.0, bounds).expect("scrollable");
    assert_eq!(roomy.length, 0.5);
}

#[test]
fn bounds_cannot_describe_an_empty_or_inverted_range() {
    let inverted = ThumbBounds::new(0.8, 0.2);
    assert_eq!(inverted.minimum(), 0.2);
    assert_eq!(inverted.maximum(), 0.8);

    let unmeasurable = ThumbBounds::new(f32::NAN, f32::NAN);
    assert_eq!(unmeasurable, ThumbBounds::FULL);

    assert_eq!(ThumbBounds::at_least(24.0, 0.0), ThumbBounds::FULL);
    assert_eq!(ThumbBounds::at_least(f32::NAN, 240.0), ThumbBounds::FULL);
}

#[test]
fn dragging_the_thumb_across_its_travel_scrolls_the_whole_content() {
    let geometry = thumb_geometry(400.0, 100.0, 0.0, ThumbBounds::FULL).expect("scrollable");
    assert_eq!(
        content_delta_for_thumb_drag(150.0, 200.0, geometry, 300.0),
        300.0
    );
    assert_eq!(
        content_delta_for_thumb_drag(-75.0, 200.0, geometry, 300.0),
        -150.0
    );
}

#[test]
fn a_thumb_with_nowhere_to_go_does_not_scroll() {
    let full = ThumbGeometry {
        length: 1.0,
        offset: 0.0,
    };
    assert_eq!(content_delta_for_thumb_drag(40.0, 200.0, full, 300.0), 0.0);

    let geometry = thumb_geometry(400.0, 100.0, 0.0, ThumbBounds::FULL).expect("scrollable");
    assert_eq!(
        content_delta_for_thumb_drag(40.0, 200.0, geometry, 0.0),
        0.0
    );
    assert_eq!(
        content_delta_for_thumb_drag(f32::NAN, 200.0, geometry, 300.0),
        0.0
    );
}
