//! The layer-affine, brush-resolution, raster-bucket and image-compare rules.
//!
//! Each of these sits under the renderer rather than beside it, so a mistake
//! shows up as a picture that is subtly wrong rather than a test that fails:
//! a point mapped about the wrong pivot, a solid colour that ignores the
//! layer's alpha, or two scales sharing one raster-cache bucket. They are
//! cheap to state exactly, so they are stated exactly.

use cranpose_render_common::{
    image_compare::pixel_difference,
    layer_transform::apply_layer_affine_to_point,
    raster_cache::ScaleBucket,
    scene_builder::{set_verify_executor, verify_executor},
    style_shared::{resolve_layer_brush, ResolvedBrush},
};
use cranpose_ui_graphics::{Brush, Color, GraphicsLayer, Point, Rect};

fn bounds() -> Rect {
    Rect {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 100.0,
    }
}

#[test]
fn an_identity_layer_leaves_a_point_exactly_where_it_was() {
    let point = Point::new(12.5, 37.25);
    let mapped = apply_layer_affine_to_point(point, bounds(), &GraphicsLayer::default());
    assert_eq!(mapped, point, "an identity layer moved the point");
}

#[test]
fn a_translated_layer_moves_a_point_by_the_translation() {
    let layer = GraphicsLayer {
        translation_x: 10.0,
        translation_y: -4.0,
        ..GraphicsLayer::default()
    };
    let mapped = apply_layer_affine_to_point(Point::new(1.0, 2.0), bounds(), &layer);
    assert_eq!(mapped, Point::new(11.0, -2.0));
}

#[test]
fn a_scaled_layer_scales_about_its_pivot_and_leaves_the_pivot_alone() {
    let layer = GraphicsLayer {
        scale_x: 2.0,
        scale_y: 2.0,
        ..GraphicsLayer::default()
    };
    // The default pivot is the centre of the bounds.
    let pivot = Point::new(50.0, 50.0);
    assert_eq!(
        apply_layer_affine_to_point(pivot, bounds(), &layer),
        pivot,
        "the pivot must be the one point a scale does not move"
    );
    assert_eq!(
        apply_layer_affine_to_point(Point::new(60.0, 50.0), bounds(), &layer),
        Point::new(70.0, 50.0),
        "a point ten past the pivot must end twenty past it at 2x"
    );
}

#[test]
fn a_solid_brush_passes_through_an_untouched_layer_unchanged() {
    let brush = Brush::Solid(Color::rgba(1.0, 0.0, 0.0, 1.0));
    match resolve_layer_brush(&brush, &GraphicsLayer::default()) {
        ResolvedBrush::Solid(color) => assert_eq!(color, Color::rgba(1.0, 0.0, 0.0, 1.0)),
        other => panic!("an identity layer changed a solid brush: {other:?}"),
    }
}

#[test]
fn a_solid_brush_takes_the_layers_alpha() {
    let layer = GraphicsLayer {
        alpha: 0.5,
        ..GraphicsLayer::default()
    };
    let brush = Brush::Solid(Color::rgba(1.0, 0.0, 0.0, 1.0));
    match resolve_layer_brush(&brush, &layer) {
        ResolvedBrush::Solid(color) => assert!(
            color.a() < 1.0,
            "the layer's alpha never reached the colour: {color:?}"
        ),
        other => panic!("a solid brush stopped being solid: {other:?}"),
    }
}

#[test]
fn identical_pixels_differ_by_nothing_and_opposites_differ_by_everything() {
    assert_eq!(pixel_difference([1, 2, 3, 4], [1, 2, 3, 4]), 0);
    assert_eq!(
        pixel_difference([0, 0, 0, 0], [255, 255, 255, 255]),
        255 * 4,
        "the difference must sum every channel"
    );
    // The measure is symmetric, or a comparison would depend on argument order.
    assert_eq!(
        pixel_difference([10, 20, 30, 40], [40, 30, 20, 10]),
        pixel_difference([40, 30, 20, 10], [10, 20, 30, 40])
    );
}

#[test]
fn a_scale_bucket_normalises_a_scale_that_cannot_be_rastered_at() {
    let unit = ScaleBucket::from_scale(1.0);
    for impossible in [0.0, -3.0, f32::NAN] {
        assert_eq!(
            ScaleBucket::from_scale(impossible).raw(),
            unit.raw(),
            "a scale of {impossible} was given a bucket of its own"
        );
    }
}

#[test]
fn two_different_scales_do_not_share_one_raster_bucket() {
    assert_ne!(
        ScaleBucket::from_scale(1.0).raw(),
        ScaleBucket::from_scale(2.0).raw(),
        "1x and 2x sharing a bucket would serve a blurry raster to one of them"
    );
}

#[test]
fn verification_is_serial_until_an_executor_is_lent() {
    assert!(
        verify_executor().is_none(),
        "a verify executor was installed on a thread nobody lent one to"
    );
    // The setter takes `None` back, which is what a host does on teardown.
    set_verify_executor(None);
    assert!(verify_executor().is_none());
}

#[test]
fn a_font_registry_rejects_fallback_bytes_that_are_not_a_font() {
    use cranpose_render_common::font_source::SoftwareTextFontRegistry;

    let mut registry = SoftwareTextFontRegistry::default();
    // A fallback is what every unnamed request lands on, so accepting rubbish
    // here would make every style resolve to a face that cannot be shaped.
    assert!(
        registry
            .register_fallback_bytes(b"not a font".to_vec())
            .is_err(),
        "the registry accepted ten bytes of text as a typeface"
    );
}

#[test]
fn a_scene_counts_the_live_modifier_slice_lookups_that_missed() {
    use cranpose_render_common::graph_scene::RenderDiagnostics;

    let diagnostics = RenderDiagnostics::new();
    assert_eq!(diagnostics.live_modifier_slice_lookup_miss_count(), 0);

    diagnostics.record_live_modifier_slice_lookup_miss();
    diagnostics.record_live_modifier_slice_lookup_miss();
    assert_eq!(
        diagnostics.live_modifier_slice_lookup_miss_count(),
        2,
        "a miss counter that does not count is a diagnostic that lies"
    );
}
