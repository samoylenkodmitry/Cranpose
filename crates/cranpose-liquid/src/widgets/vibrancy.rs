use std::{cell::RefCell, rc::Rc, sync::OnceLock};

use cranpose_macros::composable;
use cranpose_ui::{
    Modifier,
    widgets::{Box, BoxSpec},
};
use cranpose_ui_graphics::{
    BlendMode, Color, CompositingStrategy, GraphicsLayer, RUNTIME_SHADER_PRELUDE_WGSL, Rect,
    RenderEffect, RuntimeShader, ShaderTarget, ShaderWarmUp, Size,
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

fn mask_shader() -> RuntimeShader {
    static SHADER: OnceLock<RuntimeShader> = OnceLock::new();
    SHADER
        .get_or_init(|| {
            let mut shader = RuntimeShader::new(&format!(
                "{RUNTIME_SHADER_PRELUDE_WGSL}\n{}\n{}",
                cranpose_ui_graphics::LIQUID_GLASS_GEOMETRY_WGSL,
                include_str!("vibrancy.wgsl")
            ));
            shader.set_float(0, 0.9);
            shader.set_override("VIBRANCY_MASK", 1.0);
            shader
        })
        .clone()
}

/// The ink mask shader rendered into its `DstOut` layer.
pub(super) fn shader_warm_ups() -> [ShaderWarmUp; 1] {
    [ShaderWarmUp {
        shader: mask_shader(),
        target: ShaderTarget::Layer,
    }]
}

fn mask_effect(size: Size, selection: InkSelection, grid: InkGrid) -> RenderEffect {
    let mut shader = mask_shader();
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
    let content = Rc::new(RefCell::new(content));
    Box(
        modifier.graphics_layer(|| GraphicsLayer {
            compositing_strategy: CompositingStrategy::Offscreen,
            ..Default::default()
        }),
        BoxSpec::default(),
        move || {
            Box(
                Modifier::empty().fill_max_size().background(foreground),
                BoxSpec::default(),
                || {},
            );
            Box(
                Modifier::empty().fill_max_size().clip_to_bounds(),
                BoxSpec::default(),
                move || {
                    Box(
                        Modifier::empty()
                            .offset(selection.bounds.x, selection.bounds.y)
                            .size(Size::new(selection.bounds.width, selection.bounds.height))
                            .rounded_corners(
                                selection.bounds.width.min(selection.bounds.height) * 0.5,
                            )
                            .background(selection.color),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
            let content = Rc::clone(&content);
            Box(
                Modifier::empty()
                    .fill_max_size()
                    .graphics_layer(move || GraphicsLayer {
                        render_effect: Some(mask_effect(size, selection, grid)),
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
    fn the_warm_up_names_the_mask_pipeline() {
        let [warm_up] = shader_warm_ups();
        let size = Size {
            width: 10.0,
            height: 10.0,
        };
        let RenderEffect::Shader { shader } = mask_effect(size, selection(), grid()) else {
            panic!("the ink mask is a runtime shader effect");
        };
        assert_eq!(warm_up.shader.source_hash(), shader.source_hash());
        assert_eq!(warm_up.shader.overrides_hash(), shader.overrides_hash());
        assert_eq!(warm_up.target, ShaderTarget::Layer);
        assert!(
            !shader.batched_source(),
            "the mask reads its own layer, never the page"
        );
    }
}
