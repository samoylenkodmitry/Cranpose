use std::{cell::RefCell, rc::Rc, sync::OnceLock};

use cranpose_macros::composable;
use cranpose_ui::{
    Modifier,
    widgets::{Box, BoxSpec},
};
use cranpose_ui_graphics::{
    BlendMode, Color, CompositingStrategy, GraphicsLayer, RUNTIME_SHADER_PRELUDE_WGSL, Rect,
    RenderEffect, RuntimeShader, ShaderTarget, ShaderWarmUp, Size, SubstrateSpec,
};

#[derive(Clone, Copy, PartialEq)]
pub(super) struct InkSelection {
    pub bounds: Rect,
    pub color: Color,
    pub content_zoom: f32,
    pub activity: f32,
    pub optical_scale: f32,
    pub projection: Size,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct InkGrid {
    pub first_center: (f32, f32),
    pub pitch: f32,
    pub count: usize,
}

enum InkPass {
    Color,
    Mask,
}

fn shader(pass: InkPass) -> RuntimeShader {
    static SHADER: OnceLock<RuntimeShader> = OnceLock::new();
    let mut shader = SHADER
        .get_or_init(|| {
            let mut shader = RuntimeShader::new(&format!(
                "{RUNTIME_SHADER_PRELUDE_WGSL}\n{}\n{}",
                cranpose_ui_graphics::LIQUID_GLASS_GEOMETRY_WGSL,
                include_str!("vibrancy.wgsl")
            ));
            shader.set_float(0, 0.9);
            shader
        })
        .clone();
    shader.set_override("VIBRANCY_MASK", f64::from(matches!(pass, InkPass::Mask)));
    let backdrop = matches!(pass, InkPass::Color);
    shader.set_batched_source(backdrop);
    shader.set_substrates(if backdrop {
        &[SubstrateSpec::Mean]
    } else {
        &[]
    });
    shader
}

/// The ink shaders: the colour pass composited over the content's backdrop
/// and the mask pass rendered into its `DstOut` layer.
pub(super) fn shader_warm_ups() -> [ShaderWarmUp; 2] {
    [
        ShaderWarmUp {
            shader: shader(InkPass::Color),
            target: ShaderTarget::Page,
        },
        ShaderWarmUp {
            shader: shader(InkPass::Mask),
            target: ShaderTarget::Layer,
        },
    ]
}

fn effect(
    pass: InkPass,
    size: Size,
    selection: InkSelection,
    grid: InkGrid,
    dark: bool,
) -> RenderEffect {
    let mut shader = shader(pass);
    shader.set_float(12, f32::from(dark));
    shader.set_float2(13, selection.activity, selection.optical_scale);
    shader.set_float2(16, selection.projection.width, selection.projection.height);
    shader.set_float4(
        20,
        grid.first_center.0,
        grid.first_center.1,
        grid.pitch,
        grid.count as f32,
    );
    shader.set_float4(0, 0.9, size.width, size.height, selection.content_zoom);
    shader.set_float4(
        4,
        selection.bounds.x + selection.bounds.width * 0.5,
        selection.bounds.y + selection.bounds.height * 0.5,
        selection.bounds.width,
        selection.bounds.height,
    );
    shader.set_float4(
        8,
        selection.color.r(),
        selection.color.g(),
        selection.color.b(),
        selection.color.a(),
    );
    RenderEffect::runtime_shader(shader)
}

#[composable]
pub(super) fn VibrantContent(
    modifier: Modifier,
    size: Size,
    foreground: Color,
    ink: impl Fn() -> (InkSelection, InkGrid) + 'static,
    content: impl FnMut() + 'static,
) {
    let ink: Rc<dyn Fn() -> (InkSelection, InkGrid)> = Rc::new(ink);
    let modifier = modifier.size(size);
    let dark = foreground.r() + foreground.g() + foreground.b() > 1.5;
    let content = Rc::new(RefCell::new(content));
    Box(
        modifier.graphics_layer(|| GraphicsLayer {
            compositing_strategy: CompositingStrategy::Offscreen,
            ..Default::default()
        }),
        BoxSpec::default(),
        move || {
            let color_ink = Rc::clone(&ink);
            let mask_ink = Rc::clone(&ink);
            Box(
                Modifier::empty().fill_max_size().graphics_layer(move || {
                    let (selection, grid) = color_ink();
                    GraphicsLayer {
                        backdrop_effect: Some(effect(InkPass::Color, size, selection, grid, dark)),
                        ..Default::default()
                    }
                }),
                BoxSpec::default(),
                || {},
            );
            let content = Rc::clone(&content);
            Box(
                Modifier::empty().fill_max_size().graphics_layer(move || {
                    let (selection, grid) = mask_ink();
                    GraphicsLayer {
                        render_effect: Some(effect(InkPass::Mask, size, selection, grid, dark)),
                        blend_mode: BlendMode::DstOut,
                        ..Default::default()
                    }
                }),
                BoxSpec::default(),
                move || (content.borrow_mut())(),
            );
        },
    );
}

#[cfg(test)]
#[path = "tests/vibrancy_tests.rs"]
mod tests;
