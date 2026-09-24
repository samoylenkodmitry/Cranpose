use super::*;

#[test]
fn gradient_cut_spec_defaults() {
    let spec = GradientCutMaskSpec::default();
    assert_eq!(spec.progress, 0.5);
    assert_eq!(spec.feather, 24.0);
    assert_eq!(spec.corner_radius, 16.0);
    assert_eq!(spec.direction, CutDirection::LeftToRight);
}

#[test]
fn gradient_fade_spec_defaults() {
    let spec = GradientFadeMaskSpec::default();
    assert_eq!(spec.start, 0.0);
    assert_eq!(spec.end, 64.0);
    assert_eq!(spec.direction, CutDirection::TopToBottom);
}

#[test]
fn gradient_cut_effect_sets_uniforms() {
    let spec = GradientCutMaskSpec {
        progress: 0.33,
        feather: 18.0,
        corner_radius: 20.0,
        direction: CutDirection::BottomToTop,
    };
    let effect = gradient_cut_mask_effect(&spec, 320.0, 180.0);
    let RenderEffect::Shader { shader } = effect else {
        panic!("expected shader render effect");
    };

    let u = shader.uniforms();
    assert_eq!(u[0], 320.0);
    assert_eq!(u[1], 180.0);
    assert_eq!(u[2], 0.33);
    assert_eq!(u[3], 18.0);
    assert_eq!(u[4], 20.0);
    assert_eq!(u[5], 3.0);
}

#[test]
fn gradient_cut_effect_clamps_values() {
    let spec = GradientCutMaskSpec {
        progress: 2.4,
        feather: -3.0,
        corner_radius: -8.0,
        direction: CutDirection::RightToLeft,
    };
    let effect = gradient_cut_mask_effect(&spec, 0.0, 0.0);
    let RenderEffect::Shader { shader } = effect else {
        panic!("expected shader render effect");
    };

    let u = shader.uniforms();
    assert_eq!(u[0], 1.0);
    assert_eq!(u[1], 1.0);
    assert_eq!(u[2], 1.0);
    assert_eq!(u[3], 0.0);
    assert_eq!(u[4], 0.0);
    assert_eq!(u[5], 1.0);
}

#[test]
fn rounded_alpha_mask_uses_dedicated_shader_uniforms() {
    let effect = rounded_alpha_mask_effect(240.0, 120.0, 14.0, 6.0);
    let RenderEffect::Shader { shader } = effect else {
        panic!("expected shader render effect");
    };

    assert_eq!(shader.source(), ROUNDED_ALPHA_MASK_WGSL);
    let u = shader.uniforms();
    assert_eq!(u[0], 240.0);
    assert_eq!(u[1], 120.0);
    assert_eq!(u[2], 6.0);
    assert_eq!(u[3], 14.0);
    assert_eq!(u[4], 14.0);
    assert_eq!(u[5], 14.0);
    assert_eq!(u[6], 14.0);
}

#[test]
fn rounded_corner_alpha_mask_sets_per_corner_uniforms() {
    let effect = rounded_corner_alpha_mask_effect(
        240.0,
        120.0,
        CornerRadii {
            top_left: 4.0,
            top_right: 8.0,
            bottom_right: 12.0,
            bottom_left: 16.0,
        },
        2.0,
    );
    let RenderEffect::Shader { shader } = effect else {
        panic!("expected shader render effect");
    };

    assert_eq!(shader.source(), ROUNDED_ALPHA_MASK_WGSL);
    let u = shader.uniforms();
    assert_eq!(u[0], 240.0);
    assert_eq!(u[1], 120.0);
    assert_eq!(u[2], 2.0);
    assert_eq!(u[3], 4.0);
    assert_eq!(u[4], 8.0);
    assert_eq!(u[5], 12.0);
    assert_eq!(u[6], 16.0);
}

#[test]
fn gradient_fade_dst_out_effect_sets_uniforms() {
    let spec = GradientFadeMaskSpec {
        start: 24.0,
        end: 52.0,
        direction: CutDirection::BottomToTop,
    };
    let effect = gradient_fade_dst_out_effect(&spec, 300.0, 180.0);
    let RenderEffect::Shader { shader } = effect else {
        panic!("expected shader render effect");
    };

    assert_eq!(shader.source(), GRADIENT_FADE_DST_OUT_WGSL);
    let u = shader.uniforms();
    assert_eq!(u[0], 300.0);
    assert_eq!(u[1], 180.0);
    assert_eq!(u[2], 24.0);
    assert_eq!(u[3], 52.0);
    assert_eq!(u[4], 3.0);
}
