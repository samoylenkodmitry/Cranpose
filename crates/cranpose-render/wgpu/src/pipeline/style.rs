pub(crate) use cranpose_render_common::style_shared::{
    apply_layer_affine_to_rect, apply_layer_to_brush, apply_layer_to_color, apply_layer_to_quad,
    apply_layer_to_rect, layer_uniform_scale, quad_bounds, scale_corner_radii,
};
#[cfg(test)]
pub(crate) use cranpose_render_common::style_shared::{
    compose_color_filters, primitives_for_placement, DrawPlacement,
};
#[cfg(test)]
use cranpose_ui::DrawCommand;
#[cfg(test)]
use cranpose_ui_graphics::{BlendMode, DrawPrimitive, GraphicsLayer, ShadowPrimitive, Size};
use cranpose_ui_graphics::{CornerRadii, Rect, RoundedCornerShape};

#[cfg(test)]
use crate::scene::{DrawShape, Scene, ShadowDraw};

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_draw_commands(
    commands: &[DrawCommand],
    placement: DrawPlacement,
    rect: Rect,
    size: Size,
    layer: &GraphicsLayer,
    clip: Option<Rect>,
    scene: &mut Scene,
) {
    fn emit_primitive(
        primitive: DrawPrimitive,
        layer_bounds: Rect,
        layer: &GraphicsLayer,
        clip: Option<Rect>,
        scene: &mut Scene,
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
            DrawPrimitive::Rect {
                rect: local_rect,
                brush,
            } => {
                let draw_rect = local_rect.translate(layer_bounds.x, layer_bounds.y);
                let local_rect = apply_layer_affine_to_rect(draw_rect, layer_bounds, layer);
                let quad = apply_layer_to_quad(draw_rect, layer_bounds, layer);
                let transformed = quad_bounds(quad);
                let brush = apply_layer_to_brush(brush, layer);
                scene.push_shape_with_geometry(
                    transformed,
                    local_rect,
                    quad,
                    brush,
                    None,
                    clip,
                    blend_mode.unwrap_or(BlendMode::SrcOver),
                );
            }
            DrawPrimitive::RoundRect {
                rect: local_rect,
                brush,
                radii,
            } => {
                let draw_rect = local_rect.translate(layer_bounds.x, layer_bounds.y);
                let local_rect = apply_layer_affine_to_rect(draw_rect, layer_bounds, layer);
                let quad = apply_layer_to_quad(draw_rect, layer_bounds, layer);
                let transformed = quad_bounds(quad);
                let scaled_radii = scale_corner_radii(radii, layer_uniform_scale(layer));
                let shape = RoundedCornerShape::with_radii(scaled_radii);
                let brush = apply_layer_to_brush(brush, layer);
                scene.push_shape_with_geometry(
                    transformed,
                    local_rect,
                    quad,
                    brush,
                    Some(shape),
                    clip,
                    blend_mode.unwrap_or(BlendMode::SrcOver),
                );
            }
            DrawPrimitive::Image {
                rect: local_rect,
                image,
                alpha,
                color_filter,
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
                    clip,
                    src_rect,
                    blend_mode.unwrap_or(BlendMode::SrcOver),
                );
            }
            DrawPrimitive::Shadow(shadow_prim) => {
                emit_shadow(shadow_prim, layer_bounds, layer, clip, scene);
            }
        }
    }

    fn emit_shadow(
        shadow_prim: ShadowPrimitive,
        layer_bounds: Rect,
        layer: &GraphicsLayer,
        clip: Option<Rect>,
        scene: &mut Scene,
    ) {
        fn prim_to_draw_shape(
            prim: DrawPrimitive,
            layer_bounds: Rect,
            layer: &GraphicsLayer,
            blend_mode: BlendMode,
        ) -> Option<(DrawShape, BlendMode)> {
            match prim {
                DrawPrimitive::Rect {
                    rect: local_rect,
                    brush,
                } => {
                    let draw_rect = local_rect.translate(layer_bounds.x, layer_bounds.y);
                    let local_rect = apply_layer_affine_to_rect(draw_rect, layer_bounds, layer);
                    let quad = apply_layer_to_quad(draw_rect, layer_bounds, layer);
                    let transformed = quad_bounds(quad);
                    let brush = apply_layer_to_brush(brush, layer);
                    Some((
                        DrawShape {
                            rect: transformed,
                            local_rect,
                            quad,
                            brush,
                            shape: None,
                            z_index: 0, // set by push_shadow_draw
                            clip: None,
                            blend_mode,
                        },
                        blend_mode,
                    ))
                }
                DrawPrimitive::RoundRect {
                    rect: local_rect,
                    brush,
                    radii,
                } => {
                    let draw_rect = local_rect.translate(layer_bounds.x, layer_bounds.y);
                    let local_rect = apply_layer_affine_to_rect(draw_rect, layer_bounds, layer);
                    let quad = apply_layer_to_quad(draw_rect, layer_bounds, layer);
                    let transformed = quad_bounds(quad);
                    let scaled_radii = scale_corner_radii(radii, layer_uniform_scale(layer));
                    let shape = RoundedCornerShape::with_radii(scaled_radii);
                    let brush = apply_layer_to_brush(brush, layer);
                    Some((
                        DrawShape {
                            rect: transformed,
                            local_rect,
                            quad,
                            brush,
                            shape: Some(shape),
                            z_index: 0,
                            clip: None,
                            blend_mode,
                        },
                        blend_mode,
                    ))
                }
                _ => None,
            }
        }

        match shadow_prim {
            ShadowPrimitive::Drop {
                shape,
                blur_radius,
                blend_mode,
            } => {
                let Some(shape_pair) = prim_to_draw_shape(*shape, layer_bounds, layer, blend_mode)
                else {
                    return;
                };
                scene.push_shadow_draw(ShadowDraw {
                    shapes: vec![shape_pair],
                    texts: vec![],
                    blur_radius,
                    clip,
                    z_index: 0,
                });
            }
            ShadowPrimitive::Inner {
                fill,
                cutout,
                blur_radius,
                blend_mode,
                clip_rect,
            } => {
                let Some(fill_pair) = prim_to_draw_shape(*fill, layer_bounds, layer, blend_mode)
                else {
                    return;
                };
                let Some(cutout_pair) =
                    prim_to_draw_shape(*cutout, layer_bounds, layer, BlendMode::DstOut)
                else {
                    return;
                };
                // Transform the clip rect to screen coordinates
                let abs_clip = Rect {
                    x: clip_rect.x + layer_bounds.x,
                    y: clip_rect.y + layer_bounds.y,
                    width: clip_rect.width,
                    height: clip_rect.height,
                };
                let transformed_clip = apply_layer_to_rect(abs_clip, layer_bounds, layer);
                scene.push_shadow_draw(ShadowDraw {
                    shapes: vec![fill_pair, cutout_pair],
                    texts: vec![],
                    blur_radius,
                    clip: clip.map_or(Some(transformed_clip), |parent_clip| {
                        parent_clip.intersect(transformed_clip)
                    }),
                    z_index: 0,
                });
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

#[allow(dead_code)]
pub(crate) fn point_in_rounded_rect(x: f32, y: f32, rect: Rect, shape: RoundedCornerShape) -> bool {
    let radii = shape.resolve(rect.width, rect.height);
    point_in_resolved_rounded_rect(x, y, rect, &radii)
}

#[allow(dead_code)]
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
mod tests {
    use super::*;
    use crate::scene::Scene;
    use cranpose_ui::Brush;
    use cranpose_ui_graphics::{Color, TransformOrigin};
    use std::rc::Rc;

    #[test]
    fn apply_layer_to_rect_rotates_around_center() {
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 40.0,
        };
        let layer = GraphicsLayer {
            rotation_z: 90.0,
            ..Default::default()
        };

        let transformed = apply_layer_to_rect(rect, rect, &layer);
        assert!((transformed.width - 40.0).abs() < 0.01);
        assert!((transformed.height - 100.0).abs() < 0.01);
    }

    #[test]
    fn apply_layer_to_rect_honors_transform_origin() {
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 40.0,
        };
        let layer = GraphicsLayer {
            rotation_z: 90.0,
            transform_origin: TransformOrigin::new(0.0, 0.0),
            ..Default::default()
        };

        let transformed = apply_layer_to_rect(rect, rect, &layer);
        assert!((transformed.x + 40.0).abs() < 0.01);
        assert!((transformed.y - 0.0).abs() < 0.01);
    }

    #[test]
    fn apply_layer_to_rect_camera_distance_changes_projection() {
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 60.0,
        };
        let near_camera = GraphicsLayer {
            rotation_y: 25.0,
            camera_distance: 8.0,
            ..Default::default()
        };
        let far_camera = GraphicsLayer {
            rotation_y: 25.0,
            camera_distance: 24.0,
            ..Default::default()
        };

        let near = apply_layer_to_rect(rect, rect, &near_camera);
        let far = apply_layer_to_rect(rect, rect, &far_camera);
        let delta = (near.x - far.x).abs()
            + (near.y - far.y).abs()
            + (near.width - far.width).abs()
            + (near.height - far.height).abs();
        assert!(delta > 0.05);
    }

    #[test]
    fn apply_draw_commands_scales_round_rect_radii_with_uniform_axis_scale() {
        let command = DrawCommand::Behind(Rc::new(|_size| {
            vec![DrawPrimitive::RoundRect {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 80.0,
                    height: 40.0,
                },
                brush: Brush::solid(Color::BLACK),
                radii: CornerRadii::uniform(10.0),
            }]
        }));

        let layer = GraphicsLayer {
            scale: 1.0,
            scale_x: 2.0,
            scale_y: 0.5,
            ..Default::default()
        };
        let mut scene = Scene::new();
        let bounds = Rect {
            x: 0.0,
            y: 0.0,
            width: 80.0,
            height: 40.0,
        };
        apply_draw_commands(
            &[command],
            DrawPlacement::Behind,
            bounds,
            Size {
                width: 80.0,
                height: 40.0,
            },
            &layer,
            None,
            &mut scene,
        );

        let shape = scene.shapes[0].shape.expect("rounded shape");
        let radii = shape.radii();
        assert!((radii.top_left - 5.0).abs() < 1e-6);
        assert!((radii.top_right - 5.0).abs() < 1e-6);
        assert!((radii.bottom_right - 5.0).abs() < 1e-6);
        assert!((radii.bottom_left - 5.0).abs() < 1e-6);
    }
}
