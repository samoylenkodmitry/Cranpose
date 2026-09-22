use super::*;

#[test]
fn reveal_uses_minimal_movement_on_either_axis() {
    let range = Some(ScrollAxisRange::new(100.0, 1000.0, false));
    assert_eq!(axis_delta(20.0, 40.0, 0.0, 120.0, range), 0.0);
    assert_eq!(axis_delta(110.0, 40.0, 0.0, 120.0, range), 30.0);
    assert_eq!(axis_delta(-10.0, 40.0, 0.0, 120.0, range), -10.0);
    assert_eq!(axis_delta(-20.0, 200.0, 0.0, 120.0, range), 0.0);
}

#[test]
fn reveal_respects_reverse_scrolling_and_unavailable_directions() {
    let reversed = Some(ScrollAxisRange::new(100.0, 1000.0, true));
    assert_eq!(axis_delta(110.0, 40.0, 0.0, 120.0, reversed), -30.0);
    let start = Some(ScrollAxisRange::new(0.0, 1000.0, false));
    assert_eq!(axis_delta(-10.0, 40.0, 0.0, 120.0, start), 0.0);
    let end = Some(ScrollAxisRange::new(1000.0, 1000.0, false));
    assert_eq!(axis_delta(110.0, 40.0, 0.0, 120.0, end), 0.0);
}

#[test]
fn reveal_rejects_missing_ranges_and_invalid_geometry() {
    assert_eq!(axis_delta(110.0, 40.0, 0.0, 120.0, None), 0.0);
    let range = Some(ScrollAxisRange::new(0.0, 1000.0, false));
    for size in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert_eq!(axis_delta(110.0, size, 0.0, 120.0, range), 0.0);
    }
}
