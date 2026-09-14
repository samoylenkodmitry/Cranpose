use std::sync::OnceLock;

use cranpose_macros::composable;
use cranpose_ui::{
    Modifier,
    widgets::{Box, BoxSpec},
};
use cranpose_ui_graphics::{
    GraphicsLayer, RUNTIME_SHADER_PRELUDE_WGSL, RenderEffect, RuntimeShader, ShaderTarget,
    ShaderWarmUp, Size,
};

fn shader() -> RuntimeShader {
    static SHADER: OnceLock<RuntimeShader> = OnceLock::new();
    SHADER
        .get_or_init(|| {
            RuntimeShader::new(&format!(
                "{RUNTIME_SHADER_PRELUDE_WGSL}\n{}",
                include_str!("tab_lighting.wgsl")
            ))
        })
        .clone()
}

/// The lighting shader, composited over a tab bar's backdrop.
pub(super) fn shader_warm_ups() -> [ShaderWarmUp; 1] {
    [ShaderWarmUp {
        shader: shader(),
        target: ShaderTarget::Page,
    }]
}

fn effect(size: Size, touch: (f32, f32), global: f32, local: f32) -> RenderEffect {
    let mut shader = shader();
    shader.set_float4(0, size.width, size.height, touch.0, touch.1);
    shader.set_float4(
        4,
        0.8448276 * global.clamp(0.0, 1.0),
        0.28448275 * local.clamp(0.0, 1.0),
        46.5,
        0.0,
    );
    RenderEffect::runtime_shader(shader)
}

#[composable]
pub(super) fn TabLighting(
    size: Size,
    transform: GraphicsLayer,
    touch: cranpose_core::MutableState<(f32, f32)>,
    glow: cranpose_core::State<f32>,
    local_glow_factor: cranpose_core::State<f32>,
) {
    Box(
        Modifier::empty()
            .size(size)
            .graphics_layer(move || GraphicsLayer {
                backdrop_effect: Some(effect(
                    size,
                    touch.get(),
                    glow.get(),
                    glow.get() * local_glow_factor.get(),
                )),
                ..transform.clone()
            }),
        BoxSpec::default(),
        || {},
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_warm_up_names_the_pipeline_the_effect_draws() {
        let [warm_up] = shader_warm_ups();
        let size = Size {
            width: 10.0,
            height: 10.0,
        };
        let RenderEffect::Shader { shader } = effect(size, (1.0, 2.0), 0.5, 0.25) else {
            panic!("tab lighting is a runtime shader effect");
        };
        assert_eq!(warm_up.shader.source_hash(), shader.source_hash());
        assert_eq!(warm_up.shader.overrides_hash(), shader.overrides_hash());
        assert_eq!(warm_up.target, ShaderTarget::Page);
    }
}
