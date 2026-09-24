use cranpose_ui_graphics::ShaderTarget;

use super::RuntimeShaderPipelineMode;

#[test]
fn a_layer_replaces_and_a_page_composites() {
    assert_eq!(
        RuntimeShaderPipelineMode::for_target(ShaderTarget::Layer),
        RuntimeShaderPipelineMode::Replace
    );
    assert_eq!(
        RuntimeShaderPipelineMode::for_target(ShaderTarget::Page),
        RuntimeShaderPipelineMode::PremultipliedSrcOver
    );
}
