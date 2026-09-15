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
    selection: InkSelection,
    grid: InkGrid,
    content: impl FnMut() + 'static,
) {
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
            Box(
                Modifier::empty()
                    .fill_max_size()
                    .graphics_layer(move || GraphicsLayer {
                        backdrop_effect: Some(effect(InkPass::Color, size, selection, grid, dark)),
                        ..Default::default()
                    }),
                BoxSpec::default(),
                || {},
            );
            let content = Rc::clone(&content);
            Box(
                Modifier::empty()
                    .fill_max_size()
                    .graphics_layer(move || GraphicsLayer {
                        render_effect: Some(effect(InkPass::Mask, size, selection, grid, dark)),
                        blend_mode: BlendMode::DstOut,
                        ..Default::default()
                    }),
                BoxSpec::default(),
                move || (content.borrow_mut())(),
            );
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selection() -> InkSelection {
        InkSelection {
            bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            color: Color::WHITE,
            content_zoom: 1.0,
            activity: 0.0,
            optical_scale: 1.0,
            projection: Size {
                width: 10.0,
                height: 10.0,
            },
        }
    }

    fn grid() -> InkGrid {
        InkGrid {
            first_center: (5.0, 5.0),
            pitch: 10.0,
            count: 1,
        }
    }

    #[test]
    fn each_warm_up_names_the_pipeline_its_pass_draws() {
        let warm_ups = shader_warm_ups();
        let size = Size {
            width: 10.0,
            height: 10.0,
        };
        for (warm_up, pass, target) in [
            (&warm_ups[0], InkPass::Color, ShaderTarget::Page),
            (&warm_ups[1], InkPass::Mask, ShaderTarget::Layer),
        ] {
            let RenderEffect::Shader { shader } = effect(pass, size, selection(), grid(), false)
            else {
                panic!("vibrancy is a runtime shader effect");
            };
            assert_eq!(warm_up.shader.source_hash(), shader.source_hash());
            assert_eq!(warm_up.shader.overrides_hash(), shader.overrides_hash());
            assert_eq!(warm_up.target, target);
        }
        assert_ne!(
            warm_ups[0].shader.overrides_hash(),
            warm_ups[1].shader.overrides_hash(),
            "the mask override is what tells the two pipelines apart"
        );
    }
}
