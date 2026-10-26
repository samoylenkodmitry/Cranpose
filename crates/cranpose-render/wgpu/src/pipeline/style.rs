#[cfg(test)]
pub(crate) use cranpose_render_common::graph::quad_bounds;
#[cfg(test)]
pub(crate) use cranpose_render_common::layer_transform::apply_layer_to_quad;
#[cfg(test)]
pub(crate) use cranpose_render_common::layer_transform::apply_layer_to_rect;
pub(crate) use cranpose_render_common::style_shared::{
    apply_layer_to_brush, apply_layer_to_color, scale_corner_radii,
};
#[cfg(test)]
pub(crate) use cranpose_render_common::style_shared::{
    compose_color_filters, primitives_for_placement,
};
#[cfg(test)]
use cranpose_ui::DrawCommand;
#[cfg(test)]
use cranpose_ui_graphics::{
    BlendMode, CornerRadii, DrawPlacement, DrawPrimitive, GraphicsLayer, Rect, Size,
};

#[cfg(test)]
use crate::scene::{CompositorScene, ImageDraw};

#[cfg(test)]
pub(crate) fn apply_draw_commands(
    commands: &[DrawCommand],
    placement: DrawPlacement,
    rect: Rect,
    size: Size,
    layer: &GraphicsLayer,
    clip: Option<Rect>,
    scene: &mut CompositorScene,
) {
    fn emit_primitive(
        primitive: DrawPrimitive,
        layer_bounds: Rect,
        layer: &GraphicsLayer,
        clip: Option<Rect>,
        scene: &mut CompositorScene,
        blend_mode: Option<BlendMode>,
    ) {
        match primitive {
            DrawPrimitive::Content | DrawPrimitive::Group(_) => {}
            DrawPrimitive::Blend {
                primitive,
                blend_mode: nested,
            } => emit_primitive(
                *primitive,
                layer_bounds,
                layer,
                clip,
                scene,
                blend_mode.or(Some(nested)),
            ),
            shape_primitive @ (DrawPrimitive::Rect { .. }
            | DrawPrimitive::RoundRect { .. }
            | DrawPrimitive::Arc { .. }
            | DrawPrimitive::Line { .. }
            | DrawPrimitive::Trapezoid { .. }
            | DrawPrimitive::Text(_)) => {
                super::push_draw_primitive(
                    &shape_primitive,
                    &crate::scene::ContentContext {
                        layer_bounds,
                        local_layer: layer,
                        visual_clip: clip,
                        rounding: None,
                        anchor: None,
                        motion_context_animated: false,
                    },
                    scene,
                    blend_mode,
                );
            }
            DrawPrimitive::Image {
                rect: local_rect,
                image,
                alpha,
                color_filter,
                sampling,
                src_rect,
            } => {
                let draw_rect = local_rect.translate(layer_bounds.x, layer_bounds.y);
                let quad = apply_layer_to_quad(draw_rect, layer_bounds, layer);
                let transformed = quad_bounds(quad);
                let combined_alpha = (alpha * layer.alpha).clamp(0.0, 1.0);
                let combined_filter =
                    compose_color_filters(color_filter.as_ref(), layer.color_filter.as_ref());
                scene.push_image(ImageDraw {
                    rect: transformed,
                    quad,
                    snap_anchor: None,
                    image,
                    alpha: combined_alpha,
                    color_filter: combined_filter,
                    sampling,
                    clip,
                    rounding: None,
                    blend_mode: blend_mode.unwrap_or(BlendMode::SrcOver),
                    src_rect,
                    motion_context_animated: false,
                });
            }
            DrawPrimitive::Shadow(shadow_prim) => {
                super::push_shadow_primitive(&shadow_prim, layer_bounds, layer, clip, None, scene);
            }
        }
    }

    for command in commands {
        let primitives = primitives_for_placement(command, placement, size);
        for primitive in primitives {
            emit_primitive(primitive, rect, layer, clip, scene, None);
        }
    }
}

#[cfg(test)]
#[path = "tests/style_tests.rs"]
mod tests;
