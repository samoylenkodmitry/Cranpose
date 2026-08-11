use cranpose_ui::round_scroll_indicator::{
    indicator_arc, indicator_geometry, indicator_segments, IndicatorPart,
};

#[test]
fn public_api_builds_three_finite_segments_for_a_scrollable_round_screen() {
    let arc = indicator_arc(113.5);
    let geometry = indicator_geometry(400.0, 100.0, 150.0).expect("scrollable content");
    let segments = indicator_segments(arc, geometry, 1.0);

    assert_eq!(segments[0].0, IndicatorPart::Track);
    assert_eq!(segments[1].0, IndicatorPart::Thumb);
    assert_eq!(segments[2].0, IndicatorPart::Track);
    assert!((arc.sweep().to_degrees() - 30.54).abs() < 0.05);
}
