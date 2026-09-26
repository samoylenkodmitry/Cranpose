use std::sync::{Once, OnceLock};

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

/// The lighting over a bar's backdrop, or nothing while neither glow is
/// lit: the shader then returns its source, so the page stays byte for
/// byte as it was (`tab_lighting_rest_identity.rs`) and the bar owes no
/// capture, pass or stage for it.
fn effect(size: Size, touch: (f32, f32), global: f32, local: f32) -> Option<RenderEffect> {
    let global = 0.8448276 * global.clamp(0.0, 1.0);
    let local = 0.28448275 * local.clamp(0.0, 1.0);
    if global <= 0.0 && local <= 0.0 {
        return None;
    }
    let mut shader = shader();
    shader.set_float4(0, size.width, size.height, touch.0, touch.1);
    shader.set_float4(4, global, local, 46.5, 0.0);
    Some(RenderEffect::runtime_shader(shader))
}

#[composable]
pub(super) fn TabLighting(
    size: Size,
    transform: impl Fn() -> GraphicsLayer + 'static,
    touch: cranpose_core::MutableState<(f32, f32)>,
    glow: cranpose_core::State<f32>,
    local_glow_factor: cranpose_core::State<f32>,
) {
    static WARM_UP: Once = Once::new();
    WARM_UP.call_once(|| cranpose_ui_graphics::request_shader_warm_ups(shader_warm_ups()));
    Box(
        Modifier::empty()
            .size(size)
            .graphics_layer(move || GraphicsLayer {
                backdrop_effect: effect(
                    size,
                    touch.get(),
                    glow.get(),
                    glow.get() * local_glow_factor.get(),
                ),
                ..transform()
            }),
        BoxSpec::default(),
        || {},
    );
}

#[cfg(test)]
#[path = "tests/tab_lighting_tests.rs"]
mod tests;
