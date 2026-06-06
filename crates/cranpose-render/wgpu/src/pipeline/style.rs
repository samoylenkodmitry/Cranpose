#[cfg(test)]
pub(crate) use cranpose_render_common::graph::quad_bounds;
#[cfg(test)]
pub(crate) use cranpose_render_common::layer_transform::{
    apply_layer_affine_to_rect, apply_layer_to_quad,
};
#[cfg(test)]
pub(crate) use cranpose_render_common::layer_transform::{
    apply_layer_to_rect, layer_uniform_scale,
};
pub(crate) use cranpose_render_common::style_shared::{
    apply_layer_to_brush, apply_layer_to_color, scale_corner_radii,
};
#[cfg(test)]
pub(crate) use cranpose_render_common::style_shared::{
    compose_color_filters, primitives_for_placement, DrawPlacement,
};
#[cfg(test)]
use cranpose_ui::DrawCommand;
#[cfg(test)]
use cranpose_ui_graphics::{
    BlendMode, CornerRadii, DrawPrimitive, GraphicsLayer, Rect, RoundedCornerShape, Size,
};

#[cfg(test)]
use crate::scene::CompositorScene;

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
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
                    false,
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
                    false,
                );
            }
            DrawPrimitive::Shadow(shadow_prim) => {
                super::push_shadow_primitive(shadow_prim, layer_bounds, layer, clip, scene);
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
mod tests {
    use super::*;
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
        let mut scene = CompositorScene::new();
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
