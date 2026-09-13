use std::sync::OnceLock;

use cranpose_macros::composable;
use cranpose_ui::{
    Modifier,
    widgets::{Box, BoxSpec},
};
use cranpose_ui_graphics::{
    GraphicsLayer, RUNTIME_SHADER_PRELUDE_WGSL, RenderEffect, RuntimeShader, Size,
};

fn effect(size: Size, touch: (f32, f32), global: f32, local: f32) -> RenderEffect {
    static SHADER: OnceLock<RuntimeShader> = OnceLock::new();
    let mut shader = SHADER
        .get_or_init(|| {
            RuntimeShader::new(&format!(
                "{RUNTIME_SHADER_PRELUDE_WGSL}\n{}",
                include_str!("tab_lighting.wgsl")
            ))
        })
        .clone();
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
