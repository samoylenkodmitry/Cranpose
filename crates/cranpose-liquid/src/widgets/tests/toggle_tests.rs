use super::*;

#[test]
fn toggle_lens_stays_centered_on_the_thumb_at_both_endpoints() {
    let node_width = 130.0;
    for (progress, expected) in [(0.0, 20.5), (1.0, 42.5)] {
        let thumb_x = lens_ride_x(Some(progress), 0.0);
        let center = lens_translation_x(thumb_x, node_width) + node_width * 0.5;
        assert_eq!(center, expected);
    }
    assert_eq!(lens_ride_x(None, 12.5), 12.5);
}

#[test]
fn switch_semantics_exposes_the_committed_value() {
    for checked in [false, true] {
        let mut config = cranpose_ui::SemanticsConfiguration::default();
        switch_semantics(checked)(&mut config);
        assert_eq!(config.toggled, Some(checked));
        assert_eq!(config.role, Some(cranpose_ui::SemanticsWidgetRole::Switch));
        assert!(config.is_clickable);
    }
}
