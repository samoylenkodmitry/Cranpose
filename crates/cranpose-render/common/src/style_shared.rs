use std::rc::Rc;

use cranpose_foundation::PointerEvent;
use cranpose_ui::{Brush, DrawCommand, LayoutNodeData, ModifierNodeSlices};
use cranpose_ui_graphics::{
    BlendMode, Color, ColorFilter, CompositingStrategy, CornerRadii, DrawPrimitive, GraphicsLayer,
    Point, RoundedCornerShape, Size,
};

use crate::layer_transform::{layer_scale_x, layer_scale_y, layer_uniform_scale};

pub struct NodeStyle {
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

pub fn combine_layers(
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
            camera_distance: layer.camera_distance,
            transform_origin: layer.transform_origin,
            translation_x: current.translation_x + layer.translation_x,
            translation_y: current.translation_y + layer.translation_y,
            shadow_elevation: layer.shadow_elevation,
            ambient_shadow_color: layer.ambient_shadow_color,
            spot_shadow_color: layer.spot_shadow_color,
            shape: layer.shape,
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

pub use crate::graph::quad_bounds;

pub fn apply_layer_to_color(color: Color, layer: &GraphicsLayer) -> Color {
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
        Some(filter) => {
            let [r, g, b, a] = filter.apply_rgba([color.0, color.1, color.2, color.3]);
            Color(r, g, b, a)
        }
        None => color,
    }
}

pub fn compose_color_filters(
    base: Option<ColorFilter>,
    overlay: Option<ColorFilter>,
) -> Option<ColorFilter> {
    match (base, overlay) {
        (None, None) => None,
        (Some(filter), None) | (None, Some(filter)) => Some(filter),
        (Some(filter), Some(next)) => Some(filter.compose(next)),
    }
}

pub fn apply_layer_to_brush(brush: Brush, layer: &GraphicsLayer) -> Brush {
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

pub fn scale_corner_radii(radii: CornerRadii, scale: f32) -> CornerRadii {
    CornerRadii {
        top_left: radii.top_left * scale,
        top_right: radii.top_right * scale,
        bottom_right: radii.bottom_right * scale,
        bottom_left: radii.bottom_left * scale,
    }
}

#[derive(Clone, Copy)]
pub enum DrawPlacement {
    Behind,
    Overlay,
}

pub fn primitives_for_placement(
    command: &DrawCommand,
    placement: DrawPlacement,
    size: Size,
) -> Vec<DrawPrimitive> {
    // `filter(...).collect()` reports a zero lower-bound size hint, so it
    // grows the output through the whole doubling schedule; for an animated
    // scene these vectors hold thousands of primitives and are rebuilt every
    // frame. `Content` markers are rare, so the input length is the right
    // capacity.
    let filter_content = |primitives: Vec<DrawPrimitive>| {
        let mut out = Vec::with_capacity(primitives.len());
        out.extend(
            primitives
                .into_iter()
                .filter(|primitive| !matches!(primitive, DrawPrimitive::Content)),
        );
        out
    };

    let split_with_content = |primitives: Vec<DrawPrimitive>, placement| {
        let Some(last_content_idx) = primitives
            .iter()
            .rposition(|primitive| matches!(primitive, DrawPrimitive::Content))
        else {
            return if matches!(placement, DrawPlacement::Overlay) {
                filter_content(primitives)
            } else {
                Vec::new()
            };
        };

        let mut out = Vec::with_capacity(primitives.len());
        out.extend(
            primitives
                .into_iter()
                .enumerate()
                .filter_map(|(index, primitive)| {
                    if matches!(primitive, DrawPrimitive::Content) {
                        return None;
                    }
                    let is_before = index < last_content_idx;
                    match placement {
                        DrawPlacement::Behind if is_before => Some(primitive),
                        DrawPlacement::Overlay if !is_before => Some(primitive),
                        _ => None,
                    }
                }),
        );
        out
    };

    match (placement, command) {
        (DrawPlacement::Behind, DrawCommand::Behind(func)) => filter_content(func(size)),
        (DrawPlacement::Overlay, DrawCommand::Overlay(func)) => filter_content(func(size)),
        (_, DrawCommand::WithContent(func)) => split_with_content(func(size), placement),
        _ => Vec::new(),
    }
}
