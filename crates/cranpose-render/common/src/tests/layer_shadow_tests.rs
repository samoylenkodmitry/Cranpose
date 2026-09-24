use cranpose_ui_graphics::Color;

use super::*;

#[test]
fn layer_shadow_geometry_returns_none_for_zero_elevation() {
    let geometry = layer_shadow_geometry(
        &GraphicsLayer::default(),
        Rect::from_size(Default::default()),
    );
    assert!(geometry.ambient.is_none());
    assert!(geometry.spot.is_none());
}

#[test]
fn layer_shadow_geometry_matches_shadow_model() {
    let layer = GraphicsLayer {
        shadow_elevation: 10.0,
        ambient_shadow_color: Color(0.2, 0.3, 0.4, 0.8),
        spot_shadow_color: Color(0.7, 0.6, 0.5, 0.9),
        ..GraphicsLayer::default()
    };
    let transformed_bounds = Rect {
        x: 20.0,
        y: 30.0,
        width: 40.0,
        height: 12.0,
    };
    let geometry = layer_shadow_geometry(&layer, transformed_bounds);

    let ambient = geometry.ambient.expect("ambient shadow pass");
    assert_eq!(
        ambient.rect,
        Rect {
            x: 17.6,
            y: 27.6,
            width: 44.8,
            height: 16.8,
        }
    );
    assert!((ambient.blur_radius - 9.5).abs() < 1e-6);
    assert!((ambient.alpha - 0.576).abs() < 1e-6);

    let spot = geometry.spot.expect("spot shadow pass");
    assert_eq!(
        spot.rect,
        Rect {
            x: 20.071999,
            y: 34.472,
            width: 43.456,
            height: 15.455999,
        }
    );
    assert!((spot.blur_radius - 7.2).abs() < 1e-6);
    assert!((spot.alpha - 0.864).abs() < 1e-6);
}
