use std::sync::Arc;

use super::*;

#[test]
fn smooth_rect_blur_effect_chains_blur_and_mask() {
    let effect = smooth_rect_blur_effect(140.0, 100.0, 20.0, 12.0);
    let RenderEffect::Chain { first, second } = effect else {
        panic!("expected chained effect");
    };
    assert!(matches!(
        *first,
        RenderEffect::Blur {
            radius_x: 12.0,
            radius_y: 12.0,
            edge_treatment: TileMode::Clamp
        }
    ));
    assert!(matches!(*second, RenderEffect::Shader { .. }));
}

#[test]
fn half_cut_mask_effect_uses_half_progress_and_ltr_direction() {
    let effect = half_cut_mask_effect(140.0, 100.0);
    let RenderEffect::Shader { shader } = effect else {
        panic!("expected shader effect");
    };
    let u = shader.uniforms();

    assert_eq!(u[0], 140.0);
    assert_eq!(u[1], 100.0);
    assert_eq!(u[2], 0.5);
    assert_eq!(u[5], 0.0);
}

#[test]
fn vertical_gradient_brush_encodes_requested_range() {
    let brush = Brush::vertical_gradient(
        vec![Color::BLACK, Color::from_rgba_u8(0, 0, 0, 0)],
        24.0,
        60.0,
    );
    match brush {
        Brush::LinearGradient { start, end, .. } => {
            assert_eq!(start, Point::new(0.0, 24.0));
            assert_eq!(end, Point::new(0.0, 60.0));
        }
        other => panic!("expected LinearGradient, got {other:?}"),
    }
}

#[test]
fn liquid_glass_local_effect_uses_local_layer_space() {
    let effect = liquid_glass_local_effect(140.0, 100.0, 20.0);
    let RenderEffect::Shader { shader } = effect else {
        panic!("expected shader effect");
    };
    let u = shader.uniforms();

    assert_eq!(u[0], 140.0);
    assert_eq!(u[1], 100.0);
    assert_eq!(u[2], 70.0);
    assert_eq!(u[3], 50.0);
    assert_eq!(u[4], 140.0);
    assert_eq!(u[5], 100.0);
    assert_eq!(u[6], 20.0);
}

#[test]
fn clamped_drag_position_stays_within_bounds() {
    let next = clamped_drag_position(
        Point::new(999.0, -10.0),
        (10.0, 15.0),
        320.0,
        240.0,
        140.0,
        100.0,
    );
    assert_eq!(next, Point::new(180.0, 0.0));
}

#[test]
fn apply_drag_position_updates_state() {
    let runtime =
        cranpose_core::runtime::Runtime::new(Arc::new(cranpose_core::runtime::DefaultScheduler));
    let pos = cranpose_core::MutableState::with_runtime(Point::ZERO, runtime.handle());

    apply_drag_position(&pos, Point::new(42.0, 24.0));

    assert_eq!(pos.get(), Point::new(42.0, 24.0));
}

#[test]
fn slider_fraction_clamps_and_handles_degenerate_range() {
    assert_eq!(slider_fraction(15.0, 10.0, 20.0), 0.5);
    assert_eq!(slider_fraction(30.0, 10.0, 20.0), 1.0);
    assert_eq!(slider_fraction(5.0, 10.0, 20.0), 0.0);
    assert_eq!(slider_fraction(10.0, 10.0, 10.0), 0.0);
}

#[test]
fn slider_value_maps_fraction_and_clamps() {
    assert_eq!(slider_value(0.0, 2.0, 10.0), 2.0);
    assert_eq!(slider_value(0.5, 2.0, 10.0), 6.0);
    assert_eq!(slider_value(1.0, 2.0, 10.0), 10.0);
    assert_eq!(slider_value(2.0, 2.0, 10.0), 10.0);
    assert_eq!(slider_value(-1.0, 2.0, 10.0), 2.0);
}

#[test]
fn shader_section_parser_accepts_aliases() {
    assert_eq!(
        ShaderSection::from_startup_name("interactive-effects"),
        Some(ShaderSection::InteractiveEffects)
    );
    assert_eq!(
        ShaderSection::from_startup_name("Effect Semantics"),
        Some(ShaderSection::EffectSemantics)
    );
    assert_eq!(
        ShaderSection::from_startup_name("graphics_layer_fields"),
        Some(ShaderSection::GraphicsLayerFields)
    );
    assert_eq!(
        ShaderSection::from_startup_name("mask"),
        Some(ShaderSection::MaskApi)
    );
}

#[test]
fn shader_section_parser_rejects_unknown_names() {
    assert_eq!(ShaderSection::from_startup_name(""), None);
    assert_eq!(
        ShaderSection::from_startup_name("definitely-not-a-section"),
        None
    );
}
