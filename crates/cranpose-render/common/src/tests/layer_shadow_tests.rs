use cranpose_ui_graphics::Color;

use super::*;

fn assert_close(actual: f32, expected: f32, what: &str) {
    assert!(
        (actual - expected).abs() < 1e-4,
        "{what}: {actual} is not {expected}"
    );
}

fn assert_rect(actual: Rect, expected: [f32; 4], what: &str) {
    assert_close(actual.x, expected[0], &format!("{what} x"));
    assert_close(actual.y, expected[1], &format!("{what} y"));
    assert_close(actual.width, expected[2], &format!("{what} width"));
    assert_close(actual.height, expected[3], &format!("{what} height"));
}

/// A light over a 200-unit-wide window, 500 units up, in pixel units.
const LIGHT: ShadowLight = ShadowLight {
    x: 100.0,
    y: 0.0,
    z: 500.0,
    radius: 800.0,
    pixels_per_unit: 1.0,
};

#[test]
fn a_layer_without_elevation_casts_no_shadow() {
    let geometry = layer_shadow_geometry(
        &GraphicsLayer::default(),
        Rect::from_size(Default::default()),
        LIGHT,
    );
    assert!(geometry.ambient.is_none());
    assert!(geometry.spot.is_none());
}

#[test]
fn the_light_stands_where_android_puts_it() {
    // A 360 × 748 dp phone at density 3, in pixels: the light is over the
    // middle of the screen, 500 dp up lowered for the 360 dp width.
    let light = ShadowLight::for_window(1080.0, 2244.0, 3.0, 1.0);
    assert_close(light.x, 540.0, "x");
    assert_close(light.y, 0.0, "y");
    assert_close(light.z, 1400.0, "z: 500 dp × (360 / 450 + 2) / 3 × 3");
    assert_close(light.radius, 2400.0, "radius");
}

#[test]
fn a_shadow_takes_skias_ambient_and_spot_geometry() {
    let layer = GraphicsLayer {
        shadow_elevation: 10.0,
        ambient_shadow_color: Color(0.2, 0.3, 0.4, 0.8),
        spot_shadow_color: Color(0.7, 0.6, 0.5, 0.9),
        ..GraphicsLayer::default()
    };
    let bounds = Rect {
        x: 20.0,
        y: 30.0,
        width: 40.0,
        height: 12.0,
    };
    let geometry = layer_shadow_geometry(&layer, bounds, LIGHT);

    // Ambient: reaches 5 past the caster over a 5.390625-wide penumbra
    // (5 × (1 + 10/128)), half strength 1.797969 out.
    let ambient = geometry.ambient.expect("ambient pass");
    assert_rect(
        ambient.rect,
        [18.202031, 28.202031, 43.595938, 15.595938],
        "ambient",
    );
    assert_close(ambient.corner_scale, 1.0, "ambient corner scale");
    assert_close(ambient.corner_outset, 1.797969, "ambient corner outset");
    assert_close(ambient.blur_radius, 2.458125, "ambient blur");
    assert_close(ambient.alpha, 0.8 * 0.039, "ambient alpha");

    // Spot: 10 / 490 of the way further from the light, scaled by 1 + 10/490,
    // over a penumbra of 800 × 10/490.
    let spot = geometry.spot.expect("spot pass");
    assert_rect(
        spot.rect,
        [19.902041, 32.146939, 37.746939, 9.175510],
        "spot",
    );
    assert_close(spot.corner_scale, 1.020408, "spot corner scale");
    assert_close(spot.corner_outset, -1.534694, "spot corner outset");
    assert_close(spot.blur_radius, 7.444898, "spot blur");
    assert_close(spot.alpha, 0.9 * 0.19, "spot alpha");
}

#[test]
fn a_translucent_caster_casts_a_fainter_shadow() {
    let layer = GraphicsLayer {
        shadow_elevation: 4.0,
        alpha: 0.5,
        ..GraphicsLayer::default()
    };
    let geometry = layer_shadow_geometry(&layer, Rect::from_size(Default::default()), LIGHT);
    assert_close(
        geometry.ambient.expect("ambient").alpha,
        0.5 * 0.039,
        "ambient",
    );
    assert_close(geometry.spot.expect("spot").alpha, 0.5 * 0.19, "spot");
}

#[test]
fn a_shadow_drops_away_from_the_light() {
    let layer = GraphicsLayer {
        shadow_elevation: 10.0,
        ..GraphicsLayer::default()
    };
    let square = |x: f32, y: f32| Rect {
        x,
        y,
        width: 10.0,
        height: 10.0,
    };
    let spot_of = |bounds: Rect| {
        let spot = layer_shadow_geometry(&layer, bounds, LIGHT)
            .spot
            .expect("spot");
        (spot.rect.x - bounds.x, spot.rect.y - bounds.y)
    };
    let (left, near) = spot_of(square(0.0, 10.0));
    let (right, far) = spot_of(square(190.0, 400.0));
    assert!(
        left < right,
        "a caster left of the light throws its shadow further left"
    );
    assert!(
        near < far,
        "a caster further below the light throws its shadow further down"
    );
}

#[test]
fn a_light_seen_from_a_moved_and_scaled_space_keeps_its_place() {
    let local = LIGHT.in_space(40.0, 20.0, 2.0);
    assert_close(local.x, 30.0, "x");
    assert_close(local.y, -10.0, "y");
    assert_close(local.z, 250.0, "z");
    assert_close(local.radius, 400.0, "radius");
    assert_close(local.pixels_per_unit, 2.0, "pixels per unit");
}
