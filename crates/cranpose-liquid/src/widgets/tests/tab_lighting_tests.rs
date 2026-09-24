use super::*;

#[test]
fn the_warm_up_names_the_pipeline_the_effect_draws() {
    let [warm_up] = shader_warm_ups();
    let size = Size {
        width: 10.0,
        height: 10.0,
    };
    let Some(RenderEffect::Shader { shader }) = effect(size, (1.0, 2.0), 0.5, 0.25) else {
        panic!("tab lighting is a runtime shader effect");
    };
    assert_eq!(warm_up.shader.source_hash(), shader.source_hash());
    assert_eq!(warm_up.shader.overrides_hash(), shader.overrides_hash());
    assert_eq!(warm_up.target, ShaderTarget::Page);
}

#[test]
fn an_unlit_bar_draws_no_lighting() {
    let size = Size {
        width: 10.0,
        height: 10.0,
    };
    assert!(effect(size, (1.0, 2.0), 0.0, 0.0).is_none());
    assert!(effect(size, (1.0, 2.0), -0.5, 0.0).is_none());
    assert!(effect(size, (1.0, 2.0), 0.01, 0.0).is_some());
    assert!(effect(size, (1.0, 2.0), 0.0, 0.01).is_some());
}
