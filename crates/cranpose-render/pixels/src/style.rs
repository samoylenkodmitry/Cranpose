#[cfg(test)]
pub(crate) use cranpose_render_common::graph::quad_bounds;
#[cfg(test)]
pub(crate) use cranpose_render_common::layer_transform::apply_layer_to_rect;
#[cfg(test)]
pub(crate) use cranpose_render_common::layer_transform::{
    apply_layer_affine_to_rect, apply_layer_to_quad,
};
#[cfg(test)]
pub(crate) use cranpose_render_common::style_shared::{
    DrawPlacement, compose_color_filters, primitives_for_placement,
};
pub(crate) use cranpose_render_common::style_shared::{
    apply_layer_to_brush, apply_layer_to_color, combine_layers, scale_corner_radii,
};
#[cfg(test)]
use cranpose_ui::DrawCommand;
#[cfg(test)]
use cranpose_ui_graphics::{BlendMode, DrawPrimitive, GraphicsLayer, ShadowPrimitive, Size};
use cranpose_ui_graphics::{CornerRadii, Rect};

#[cfg(test)]
use crate::scene::RasterScene;

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_draw_commands(
    commands: &[DrawCommand],
    placement: DrawPlacement,
    rect: Rect,
    size: Size,
    layer: &GraphicsLayer,
    clip: Option<Rect>,
    scene: &mut RasterScene,
) {
    fn emit_primitive(
        primitive: DrawPrimitive,
        layer_bounds: Rect,
        layer: &GraphicsLayer,
        clip: Option<Rect>,
        scene: &mut RasterScene,
        blend_mode: Option<BlendMode>,
    ) {
        match primitive {
            DrawPrimitive::Content => {}
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
            | DrawPrimitive::Text(_)) => {
                crate::pipeline::push_draw_primitive(
                    &shape_primitive,
                    layer_bounds,
                    layer,
                    clip,
                    scene,
                    blend_mode,
                    false,
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
                let local_rect = apply_layer_affine_to_rect(draw_rect, layer_bounds, layer);
                let quad = apply_layer_to_quad(draw_rect, layer_bounds, layer);
                let transformed = quad_bounds(quad);
                let combined_alpha = (alpha * layer.alpha).clamp(0.0, 1.0);
                let combined_filter = compose_color_filters(color_filter, layer.color_filter);
                scene.push_image_with_geometry(
                    transformed,
                    local_rect,
                    quad,
                    image,
                    combined_alpha,
                    combined_filter,
                    sampling,
                    clip,
                    src_rect,
                    blend_mode.unwrap_or(BlendMode::SrcOver),
                );
            }
            DrawPrimitive::Shadow(shadow_primitive) => match shadow_primitive {
                ShadowPrimitive::Drop {
                    shape,
                    cutout,
                    blur_radius: _,
                    blend_mode: shadow_blend_mode,
                } => {
                    emit_primitive(
                        *shape,
                        layer_bounds,
                        layer,
                        clip,
                        scene,
                        blend_mode.or(Some(shadow_blend_mode)),
                    );
                    if let Some(cutout) = cutout {
                        emit_primitive(
                            *cutout,
                            layer_bounds,
                            layer,
                            clip,
                            scene,
                            Some(BlendMode::DstOut),
                        );
                    }
                }
                ShadowPrimitive::Inner {
                    fill,
                    cutout,
                    blur_radius: _,
                    blend_mode: shadow_blend_mode,
                    clip_rect,
                } => {
                    let abs_clip = Rect {
                        x: clip_rect.x + layer_bounds.x,
                        y: clip_rect.y + layer_bounds.y,
                        width: clip_rect.width,
                        height: clip_rect.height,
                    };
                    let transformed_clip = apply_layer_to_rect(abs_clip, layer_bounds, layer);
                    let shadow_clip = clip.map_or(Some(transformed_clip), |parent_clip| {
                        parent_clip.intersect(transformed_clip)
                    });
                    emit_primitive(
                        *fill,
                        layer_bounds,
                        layer,
                        shadow_clip,
                        scene,
                        blend_mode.or(Some(shadow_blend_mode)),
                    );
                    emit_primitive(
                        *cutout,
                        layer_bounds,
                        layer,
                        shadow_clip,
                        scene,
                        blend_mode.or(Some(BlendMode::DstOut)),
                    );
                }
            },
        }
    }

    for command in commands {
        let primitives = primitives_for_placement(command, placement, size);
        for primitive in primitives {
            emit_primitive(primitive, rect, layer, clip, scene, None);
        }
    }
}

pub(crate) fn point_in_resolved_rounded_rect(
    x: f32,
    y: f32,
    rect: Rect,
    radii: &CornerRadii,
) -> bool {
    if !rect.contains(x, y) {
        return false;
    }
    let left = rect.x;
    let right = rect.x + rect.width;
    let top = rect.y;
    let bottom = rect.y + rect.height;

    if radii.top_left > 0.0 && x < left + radii.top_left && y < top + radii.top_left {
        let cx = left + radii.top_left;
        let cy = top + radii.top_left;
        if (x - cx).powi(2) + (y - cy).powi(2) > radii.top_left.powi(2) {
            return false;
        }
    }
    if radii.top_right > 0.0 && x > right - radii.top_right && y < top + radii.top_right {
        let cx = right - radii.top_right;
        let cy = top + radii.top_right;
        if (x - cx).powi(2) + (y - cy).powi(2) > radii.top_right.powi(2) {
            return false;
        }
    }
    if radii.bottom_right > 0.0 && x > right - radii.bottom_right && y > bottom - radii.bottom_right
    {
        let cx = right - radii.bottom_right;
        let cy = bottom - radii.bottom_right;
        if (x - cx).powi(2) + (y - cy).powi(2) > radii.bottom_right.powi(2) {
            return false;
        }
    }
    if radii.bottom_left > 0.0 && x < left + radii.bottom_left && y > bottom - radii.bottom_left {
        let cx = left + radii.bottom_left;
        let cy = bottom - radii.bottom_left;
        if (x - cx).powi(2) + (y - cy).powi(2) > radii.bottom_left.powi(2) {
            return false;
        }
    }
    true
}

#[cfg(test)]
#[path = "tests/style_tests.rs"]
mod tests;
