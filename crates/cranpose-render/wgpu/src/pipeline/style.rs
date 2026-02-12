use std::rc::Rc;

use cranpose_foundation::PointerEvent;
use cranpose_ui::{Brush, DrawCommand, LayoutNodeData, ModifierNodeSlices};
use cranpose_ui_graphics::{
    BlendMode, Color, ColorFilter, CompositingStrategy, CornerRadii, DrawPrimitive, GraphicsLayer,
    LayerShape, Point, Rect, RoundedCornerShape, ShadowPrimitive, Size, TransformOrigin,
};

use crate::scene::{DrawShape, Scene, ShadowDraw};

pub(crate) struct NodeStyle {
    pub padding: cranpose_ui_graphics::EdgeInsets,
    pub background: Option<Color>,
    pub click_actions: Vec<Rc<dyn Fn(Point)>>,
    pub shape: Option<RoundedCornerShape>,
    pub pointer_inputs: Vec<Rc<dyn Fn(PointerEvent)>>,
    pub draw_commands: Vec<DrawCommand>,
    pub graphics_layer: Option<GraphicsLayer>,
    pub clip_to_bounds: bool,
}

impl NodeStyle {
    pub fn from_layout_node(data: &LayoutNodeData) -> Self {
        let resolved = data.resolved_modifiers;
        let slices: &ModifierNodeSlices = data.modifier_slices();
        let pointer_inputs = slices.pointer_inputs().to_vec();

        Self {
            padding: resolved.padding(),
            background: None,
            click_actions: slices.click_handlers().to_vec(),
            shape: None,
            pointer_inputs,
            draw_commands: slices.draw_commands().to_vec(),
            graphics_layer: slices.graphics_layer(),
            clip_to_bounds: slices.clip_to_bounds(),
        }
    }
}

pub(crate) fn combine_layers(
    current: GraphicsLayer,
    modifier_layer: Option<GraphicsLayer>,
) -> GraphicsLayer {
    if let Some(layer) = modifier_layer {
        GraphicsLayer {
            alpha: (current.alpha * layer.alpha).clamp(0.0, 1.0),
            scale: current.scale * layer.scale,
            scale_x: current.scale_x * layer.scale_x,
            scale_y: current.scale_y * layer.scale_y,
            rotation_x: current.rotation_x + layer.rotation_x,
            rotation_y: current.rotation_y + layer.rotation_y,
            rotation_z: current.rotation_z + layer.rotation_z,
            camera_distance: if (layer.camera_distance - 8.0).abs() > f32::EPSILON {
                layer.camera_distance
            } else {
                current.camera_distance
            },
            transform_origin: if layer.transform_origin != TransformOrigin::CENTER {
                layer.transform_origin
            } else {
                current.transform_origin
            },
            translation_x: current.translation_x + layer.translation_x,
            translation_y: current.translation_y + layer.translation_y,
            shadow_elevation: if layer.shadow_elevation > 0.0 {
                layer.shadow_elevation
            } else {
                current.shadow_elevation
            },
            ambient_shadow_color: if layer.ambient_shadow_color != Color::BLACK {
                layer.ambient_shadow_color
            } else {
                current.ambient_shadow_color
            },
            spot_shadow_color: if layer.spot_shadow_color != Color::BLACK {
                layer.spot_shadow_color
            } else {
                current.spot_shadow_color
            },
            shape: if layer.shape != LayerShape::Rectangle {
                layer.shape
            } else {
                current.shape
            },
            clip: current.clip || layer.clip,
            color_filter: compose_color_filters(current.color_filter, layer.color_filter),
            compositing_strategy: layer.compositing_strategy,
            blend_mode: layer.blend_mode,
            // render_effect is NOT inherited — it applies only to this layer's subtree
            render_effect: layer.render_effect,
            // backdrop_effect is NOT inherited — it applies only to this node's backdrop.
            backdrop_effect: layer.backdrop_effect,
        }
    } else {
        GraphicsLayer {
            compositing_strategy: CompositingStrategy::Auto,
            blend_mode: BlendMode::SrcOver,
            render_effect: None,
            backdrop_effect: None,
            ..current
        }
    }
}

fn layer_scale_x(layer: &GraphicsLayer) -> f32 {
    layer.scale * layer.scale_x
}

fn layer_scale_y(layer: &GraphicsLayer) -> f32 {
    layer.scale * layer.scale_y
}

pub(crate) fn layer_uniform_scale(layer: &GraphicsLayer) -> f32 {
    layer_scale_x(layer).min(layer_scale_y(layer))
}

pub(crate) fn apply_layer_affine_to_rect(
    rect: Rect,
    layer_bounds: Rect,
    layer: &GraphicsLayer,
) -> Rect {
    let offset_x = rect.x - layer_bounds.x;
    let offset_y = rect.y - layer_bounds.y;
    let scale_x = layer_scale_x(layer);
    let scale_y = layer_scale_y(layer);
    Rect {
        x: layer_bounds.x + offset_x * scale_x + layer.translation_x,
        y: layer_bounds.y + offset_y * scale_y + layer.translation_y,
        width: rect.width * scale_x,
        height: rect.height * scale_y,
    }
}

fn layer_rotation_pivot(layer_bounds: Rect, layer: &GraphicsLayer) -> (f32, f32) {
    (
        layer_bounds.x + layer_bounds.width * layer.transform_origin.pivot_fraction_x,
        layer_bounds.y + layer_bounds.height * layer.transform_origin.pivot_fraction_y,
    )
}

fn layer_has_rotation(layer: &GraphicsLayer) -> bool {
    layer.rotation_x.abs() > f32::EPSILON
        || layer.rotation_y.abs() > f32::EPSILON
        || layer.rotation_z.abs() > f32::EPSILON
}

fn apply_rotation_and_perspective(
    point: [f32; 2],
    pivot: (f32, f32),
    layer: &GraphicsLayer,
) -> [f32; 2] {
    if !layer_has_rotation(layer) {
        return point;
    }

    let mut x = point[0] - pivot.0;
    let mut y = point[1] - pivot.1;
    let mut z = 0.0f32;

    let (sin_x, cos_x) = layer.rotation_x.to_radians().sin_cos();
    let (sin_y, cos_y) = layer.rotation_y.to_radians().sin_cos();
    let (sin_z, cos_z) = layer.rotation_z.to_radians().sin_cos();

    let y_rot_x = y * cos_x - z * sin_x;
    let z_rot_x = y * sin_x + z * cos_x;
    y = y_rot_x;
    z = z_rot_x;

    let x_rot_y = x * cos_y + z * sin_y;
    let z_rot_y = -x * sin_y + z * cos_y;
    x = x_rot_y;
    z = z_rot_y;

    let x_rot_z = x * cos_z - y * sin_z;
    let y_rot_z = x * sin_z + y * cos_z;
    x = x_rot_z;
    y = y_rot_z;

    // Compose cameraDistance is effectively scaled by display density when mapped to
    // backend transforms; raw values like 8.0 are not literal "near camera plane"
    // distances in local layer units.
    const CAMERA_DISTANCE_SCALE: f32 = 72.0;
    let camera_distance = (layer.camera_distance * CAMERA_DISTANCE_SCALE).max(1.0);
    let denom = (camera_distance - z).max(1.0);
    let perspective = camera_distance / denom;

    [pivot.0 + x * perspective, pivot.1 + y * perspective]
}

pub(crate) fn apply_layer_to_quad(
    rect: Rect,
    layer_bounds: Rect,
    layer: &GraphicsLayer,
) -> [[f32; 2]; 4] {
    let affine_rect = apply_layer_affine_to_rect(rect, layer_bounds, layer);
    let affine_layer_bounds = apply_layer_affine_to_rect(layer_bounds, layer_bounds, layer);
    let pivot = layer_rotation_pivot(affine_layer_bounds, layer);
    let quad = [
        [affine_rect.x, affine_rect.y],
        [affine_rect.x + affine_rect.width, affine_rect.y],
        [affine_rect.x, affine_rect.y + affine_rect.height],
        [
            affine_rect.x + affine_rect.width,
            affine_rect.y + affine_rect.height,
        ],
    ];

    quad.map(|point| apply_rotation_and_perspective(point, pivot, layer))
}

pub(crate) fn quad_bounds(quad: [[f32; 2]; 4]) -> Rect {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;

    for [x, y] in quad {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }

    Rect {
        x: min_x,
        y: min_y,
        width: (max_x - min_x).max(0.0),
        height: (max_y - min_y).max(0.0),
    }
}

pub(crate) fn apply_layer_to_rect(rect: Rect, layer_bounds: Rect, layer: &GraphicsLayer) -> Rect {
    quad_bounds(apply_layer_to_quad(rect, layer_bounds, layer))
}

fn intersect_rects(a: Rect, b: Rect) -> Option<Rect> {
    let left = a.x.max(b.x);
    let top = a.y.max(b.y);
    let right = (a.x + a.width).min(b.x + b.width);
    let bottom = (a.y + a.height).min(b.y + b.height);
    let width = right - left;
    let height = bottom - top;
    if width > 0.0 && height > 0.0 {
        Some(Rect {
            x: left,
            y: top,
            width,
            height,
        })
    } else {
        None
    }
}

pub(crate) fn apply_layer_to_color(color: Color, layer: &GraphicsLayer) -> Color {
    apply_color_filter_to_color(
        Color(
            color.0,
            color.1,
            color.2,
            (color.3 * layer.alpha).clamp(0.0, 1.0),
        ),
        layer.color_filter,
    )
}

fn apply_color_filter_to_color(color: Color, filter: Option<ColorFilter>) -> Color {
    match filter {
        Some(ColorFilter::Tint(tint)) => Color(
            (color.0 * tint.r()).clamp(0.0, 1.0),
            (color.1 * tint.g()).clamp(0.0, 1.0),
            (color.2 * tint.b()).clamp(0.0, 1.0),
            (color.3 * tint.a()).clamp(0.0, 1.0),
        ),
        None => color,
    }
}

fn compose_color_filters(
    base: Option<ColorFilter>,
    overlay: Option<ColorFilter>,
) -> Option<ColorFilter> {
    match (base, overlay) {
        (None, None) => None,
        (Some(filter), None) | (None, Some(filter)) => Some(filter),
        (Some(ColorFilter::Tint(a)), Some(ColorFilter::Tint(b))) => Some(ColorFilter::Tint(Color(
            (a.r() * b.r()).clamp(0.0, 1.0),
            (a.g() * b.g()).clamp(0.0, 1.0),
            (a.b() * b.b()).clamp(0.0, 1.0),
            (a.a() * b.a()).clamp(0.0, 1.0),
        ))),
    }
}

pub(crate) fn apply_layer_to_brush(brush: Brush, layer: &GraphicsLayer) -> Brush {
    let scale_x = layer_scale_x(layer);
    let scale_y = layer_scale_y(layer);
    let uniform_scale = layer_uniform_scale(layer);

    match brush {
        Brush::Solid(color) => Brush::solid(apply_layer_to_color(color, layer)),
        Brush::LinearGradient {
            colors,
            stops,
            mut start,
            mut end,
            tile_mode,
        } => {
            start.x *= scale_x;
            start.y *= scale_y;
            end.x *= scale_x;
            end.y *= scale_y;
            Brush::LinearGradient {
                colors: colors
                    .into_iter()
                    .map(|c| apply_layer_to_color(c, layer))
                    .collect(),
                stops,
                start,
                end,
                tile_mode,
            }
        }
        Brush::RadialGradient {
            colors,
            stops,
            mut center,
            mut radius,
            tile_mode,
        } => {
            center.x *= scale_x;
            center.y *= scale_y;
            radius *= uniform_scale;
            Brush::RadialGradient {
                colors: colors
                    .into_iter()
                    .map(|c| apply_layer_to_color(c, layer))
                    .collect(),
                stops,
                center,
                radius,
                tile_mode,
            }
        }
        Brush::SweepGradient {
            colors,
            stops,
            mut center,
        } => {
            center.x *= scale_x;
            center.y *= scale_y;
            Brush::SweepGradient {
                colors: colors
                    .into_iter()
                    .map(|c| apply_layer_to_color(c, layer))
                    .collect(),
                stops,
                center,
            }
        }
    }
}

pub(crate) fn scale_corner_radii(radii: CornerRadii, scale: f32) -> CornerRadii {
    CornerRadii {
        top_left: radii.top_left * scale,
        top_right: radii.top_right * scale,
        bottom_right: radii.bottom_right * scale,
        bottom_left: radii.bottom_left * scale,
    }
}

#[derive(Clone, Copy)]
pub(crate) enum DrawPlacement {
    Behind,
    Overlay,
}

fn primitives_for_placement(
    command: &DrawCommand,
    placement: DrawPlacement,
    size: Size,
) -> Vec<DrawPrimitive> {
    let split_with_content = |primitives: Vec<DrawPrimitive>, placement| {
        let Some(content_idx) = primitives
            .iter()
            .position(|primitive| matches!(primitive, DrawPrimitive::Content))
        else {
            return if matches!(placement, DrawPlacement::Overlay) {
                primitives
                    .into_iter()
                    .filter(|primitive| !matches!(primitive, DrawPrimitive::Content))
                    .collect()
            } else {
                Vec::new()
            };
        };

        primitives
            .into_iter()
            .enumerate()
            .filter_map(|(index, primitive)| {
                if matches!(primitive, DrawPrimitive::Content) {
                    return None;
                }
                let is_before = index < content_idx;
                match placement {
                    DrawPlacement::Behind if is_before => Some(primitive),
                    DrawPlacement::Overlay if !is_before => Some(primitive),
                    _ => None,
                }
            })
            .collect()
    };

    match (placement, command) {
        (DrawPlacement::Behind, DrawCommand::Behind(func)) => func(size)
            .into_iter()
            .filter(|primitive| !matches!(primitive, DrawPrimitive::Content))
            .collect(),
        (DrawPlacement::Overlay, DrawCommand::Overlay(func)) => func(size)
            .into_iter()
            .filter(|primitive| !matches!(primitive, DrawPrimitive::Content))
            .collect(),
        (_, DrawCommand::WithContent(func)) => split_with_content(func(size), placement),
        _ => Vec::new(),
    }
}

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
                let scaled_radii = scale_corner_radii(radii, layer.scale);
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
                    let scaled_radii = scale_corner_radii(radii, layer.scale);
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
                    blur_radius,
                    clip: clip.map_or(Some(transformed_clip), |parent_clip| {
                        intersect_rects(parent_clip, transformed_clip)
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
}
