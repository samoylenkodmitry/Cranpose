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
        None,
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

fn elevated(elevation: f32, alpha: f32) -> GraphicsLayer {
    GraphicsLayer {
        shadow_elevation: elevation,
        alpha,
        ambient_shadow_color: Color(0.2, 0.3, 0.4, 0.8),
        spot_shadow_color: Color(0.7, 0.6, 0.5, 0.8),
        ..GraphicsLayer::default()
    }
}

const CASTER: Rect = Rect {
    x: 20.0,
    y: 30.0,
    width: 40.0,
    height: 12.0,
};

#[test]
fn a_shadow_takes_skias_ambient_and_spot_geometry() {
    let geometry = layer_shadow_geometry(&elevated(10.0, 1.0), CASTER, None, LIGHT);

    // Ambient: reaches 5 past the caster, its corners rounded by the reach,
    // over a ramp 5 × (1 + 10/128) wide that ends at the umbra; the opaque
    // caster covers the middle from the umbra in. Android keeps 9 of the
    // alpha's 255 and truncates: 204 × 9 / 255.
    let (ambient, ambient_color) = geometry.ambient.expect("ambient pass");
    assert_rect(ambient.bounds, [15.0, 25.0, 50.0, 22.0], "ambient");
    assert_eq!(ambient.radii, [5.0; 4], "ambient corners");
    assert_close(ambient.umbra_inset, 5.390625, "ambient umbra");
    assert_close(ambient.distance_correction, 1.0, "ambient ramp end");
    let hole = ambient.hole.expect("an opaque caster's hole");
    assert_close(hole.inset, 5.390625, "ambient hole");
    assert_close(ambient_color.a(), 7.0 / 255.0, "ambient alpha");

    // Spot: 10 / 490 of the way further from the light, scaled by 1 + 10/490,
    // grown by a blur of 800 × 10/490 over a ramp twice that. Its inset
    // reaches past half its height, so Skia fills it.
    let (spot, spot_color) = geometry.spot.expect("spot pass");
    assert_rect(
        spot.bounds,
        [2.040816, 14.285714, 73.46939, 44.897957],
        "spot",
    );
    assert_close(spot.radii[0], 16.32653, "spot corners");
    assert_close(spot.umbra_inset, 22.448978, "spot umbra: half the height");
    assert_close(spot.distance_correction, 0.6875, "spot ramp end");
    assert!(spot.hole.is_none(), "a spot whose inset fills it");
    assert_close(spot_color.a(), 38.0 / 255.0, "spot alpha: 204 × 48 / 255");
}

#[test]
fn a_translucent_caster_casts_a_fainter_filled_shadow() {
    let geometry = layer_shadow_geometry(&elevated(10.0, 0.5), CASTER, None, LIGHT);
    let (ambient, color) = geometry.ambient.expect("ambient");
    assert_close(color.a(), 3.0 / 255.0, "ambient: 204 × 9 / 255 × 0.5");
    assert!(ambient.hole.is_none(), "nothing covers its middle");
    let (_, color) = geometry.spot.expect("spot");
    assert_close(color.a(), 19.0 / 255.0, "spot: 204 × 48 / 255 × 0.5");
}

#[test]
fn a_shadow_drops_away_from_the_light() {
    let layer = elevated(10.0, 1.0);
    let square = |x: f32, y: f32| Rect {
        x,
        y,
        width: 10.0,
        height: 10.0,
    };
    let spot_of = |bounds: Rect| {
        let (spot, _) = layer_shadow_geometry(&layer, bounds, None, LIGHT)
            .spot
            .expect("spot");
        (spot.bounds.x - bounds.x, spot.bounds.y - bounds.y)
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
fn a_round_caster_casts_a_round_shadow() {
    let disc = Rect {
        x: 0.0,
        y: 0.0,
        width: 20.0,
        height: 20.0,
    };
    let caster = RoundedCornerShape::uniform(10.0);
    let geometry = layer_shadow_geometry(&elevated(4.0, 1.0), disc, Some(caster), LIGHT);
    let (ambient, _) = geometry.ambient.expect("ambient");
    assert_close(ambient.umbra_inset, 12.0, "the ramp runs to the centre");
    let hole = ambient.hole.expect("the caster covers a disc");
    assert_close(hole.radius, 10.0, "the caster's own disc");
    // Radially, a point 3 in from the outer edge takes the ramp 3 / blur in.
    let blur = 2.0 * (1.0 + 4.0 / 128.0);
    let at = 12.0 - 11.0 * std::f32::consts::FRAC_1_SQRT_2;
    assert_close(
        ambient.coverage(at - 2.0, at - 2.0),
        shadow_falloff(1.0 / blur),
        "one in from the edge on the diagonal",
    );
}

/// A shadow over (0, 0, 100, 60) with corners of `radius`, a ramp `blur`
/// wide and an inset of `inset`.
fn shadow(radius: f32, blur: f32, inset: f32) -> ShadowRRect {
    ShadowRRect::new(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 60.0,
        },
        [radius; 4],
        blur,
        inset,
    )
    .expect("a shadow")
}

#[test]
fn a_straight_edge_ramps_over_the_blur() {
    let shadow = shadow(5.0, 8.0, 100.0);
    for depth in [0.5, 2.0, 4.0, 7.5] {
        assert_close(
            shadow.coverage(50.0, depth),
            shadow_falloff(depth / 8.0),
            &format!("{depth} in from the top"),
        );
    }
    assert_close(
        shadow.coverage(50.0, 30.0),
        shadow_falloff(1.0),
        "the middle",
    );
}

#[test]
fn a_shadow_draws_nothing_outside_its_bounds_or_in_its_hole() {
    let shadow = shadow(5.0, 8.0, 6.0);
    let hole = shadow.hole.expect("a hole");
    assert_close(hole.inset, 8.0, "the hole starts at the umbra");
    assert_eq!(shadow.coverage(50.0, 30.0), 0.0, "in the hole");
    assert!(shadow.coverage(50.0, 7.5) > 0.0, "just above the hole");
    assert_eq!(shadow.coverage(-0.5, 30.0), 0.0, "left of the bounds");
    assert_eq!(shadow.coverage(50.0, 60.5), 0.0, "below the bounds");
}

#[test]
fn a_corner_as_deep_as_its_radius_is_a_quarter_circle() {
    let shadow = shadow(10.0, 4.0, 100.0);
    for (radius, angle) in [(2.0_f32, 0.3_f32), (6.0, 0.8), (9.5, 1.2)] {
        let x = 10.0 - radius * angle.cos();
        let y = 10.0 - radius * angle.sin();
        assert_close(
            shadow.coverage(x, y),
            shadow_falloff(2.5 * (1.0 - radius / 10.0)),
            &format!("{radius} from the corner's centre"),
        );
    }
}

#[test]
fn a_corner_deeper_than_its_radius_starts_its_ramp_at_skias_mesh_edge() {
    let (umbra, radius) = (12.0, 3.0);
    assert_close(fan_offset(umbra, 0.0, umbra, radius), 1.0, "on the edge");
    assert_close(
        fan_offset(umbra, umbra - radius, umbra, radius),
        1.0,
        "where the curve starts",
    );
    let at_corner = fan_offset(umbra, umbra, umbra, radius);
    assert!(
        at_corner > 1.0,
        "the outer corner lies beyond the curve: {at_corner}"
    );
    for (along, across) in [(7.0, 2.0), (5.0, 5.0)] {
        assert_close(
            fan_offset(along, across, umbra, radius),
            fan_offset(across, along, umbra, radius),
            "both edges of the corner",
        );
    }
}

#[test]
fn the_falloff_reads_skias_table() {
    assert!(
        shadow_falloff(0.0) < 0.5 / 255.0,
        "the outer edge, which Skia's byte table holds as 0"
    );
    assert_close(shadow_falloff(1.0), 0.982, "the umbra: 1 - 0.018");
    assert_close(
        shadow_falloff(0.5),
        (-4.0_f32 * (1.0 - 63.5 / 127.0_f32).powi(2)).exp() - 0.018,
        "half way, read half a texel in",
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
