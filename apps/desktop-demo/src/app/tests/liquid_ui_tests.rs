use super::*;

#[test]
fn shader_preview_uses_a_bounded_live_wcksrd_lens() {
    assert_eq!(OPTICAL_PREVIEW_HEIGHT, 256.0);
    assert_eq!(OPTICAL_PREVIEW_LENS_SIZE, Size::new(150.0, 82.0));
    let stage = Size::new(440.0, OPTICAL_PREVIEW_HEIGHT);
    let initial = initial_optical_preview_center(stage);
    assert_eq!(initial.x, 220.0);
    assert!((initial.y - 71.68).abs() < 1.0e-4);
    assert_eq!(
        clamp_optical_preview_center(Point::new(-20.0, -20.0), stage),
        Point::new(75.0, 41.0)
    );
    assert_eq!(
        clamp_optical_preview_center(Point::new(900.0, 900.0), stage),
        Point::new(365.0, 215.0)
    );
}

#[test]
fn shader_playground_maps_normalized_controls_to_wcksrd() {
    let glass = optical_preview_glass(0.50, 0.75, 0.25, 0.30, 0.34);
    assert_eq!(glass.refraction_depth, 0.50);
    assert_eq!(glass.refraction_curve, 0.75);
    assert_eq!(glass.blur_radius, Some(2.0));
    assert_eq!(glass.dispersion, 0.30);
    assert_eq!(glass.highlight, 0.34);
}

#[test]
fn toggle_press_reference_stage_uses_target_pixels_at_three_x() {
    let layout = toggle_press_reference_layout();
    assert_eq!(layout.size, Size::new(340.0 / 3.0, 190.0 / 3.0));
    assert_eq!(layout.track_origin, Point::new(37.0 / 3.0, 52.0 / 3.0));
    assert_eq!(
        layout.backdrop_cap_center,
        Point::new(203.0 / 3.0, 94.0 / 3.0)
    );
    assert_eq!(layout.backdrop_cap_radius, 77.0 / 3.0);
    assert_eq!(layout.background, Color::from_rgb_u8(238, 237, 245));
}

#[test]
fn tab_swipe_reference_backdrop_uses_target_geometry() {
    let layout = tab_swipe_reference_layout();
    assert_eq!(layout.size, Size::new(440.0, 132.0));
    assert_eq!(
        layout.title,
        Rect {
            x: 40.0,
            y: 0.0,
            width: 360.0,
            height: 29.0,
        }
    );
    assert_eq!(
        layout.enroll,
        Rect {
            x: 20.0,
            y: 33.0,
            width: 400.0,
            height: 51.0,
        }
    );
    assert_eq!(
        layout.xcode,
        Rect {
            x: 32.0,
            y: 69.0,
            width: 376.0,
            height: 50.0,
        }
    );
}

#[test]
fn tab_swipe_reference_row_is_physical_glass() {
    let glass = tab_swipe_reference_glass();
    assert_eq!(glass.variant, GlassVariant::Regular);
    assert!(glass.shadow);
    assert_eq!(glass.refraction_depth, 0.0);
    assert!(glass.blur_radius.is_none());
    assert!(glass.tint.is_none());
}

#[test]
fn floating_tab_bar_matches_the_reference_bottom_inset() {
    assert_eq!(LIQUID_TAB_BAR_BOTTOM_PADDING, 19.0);
}

#[test]
fn tab_swipe_sources_match_the_reference_symbol_footprints() {
    let tabs = tab_swipe_reference_tabs();
    assert_eq!(tabs.len(), 4);
    assert_eq!(tabs[2].icon_style, LiquidTabIconStyle::AppBadge);
    assert_eq!(tabs[2].icon_scale, 0.94);
    assert_eq!(tabs[3].icon_scale, 0.95);
}
