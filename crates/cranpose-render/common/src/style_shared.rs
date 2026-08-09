use std::rc::Rc;

use cranpose_foundation::PointerEvent;
use cranpose_ui::{Brush, DrawCommand, LayoutNodeData, ModifierNodeSlices};
use cranpose_ui_graphics::{
    clear_recorded_content_markers, recorded_content_markers, BlendMode, Color, ColorFilter,
    CompositingStrategy, CornerRadii, DrawPrimitive, GraphicsLayer, Point, RoundedCornerShape,
    Size,
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
    // The overwhelmingly common case — full-alpha layer, no filter, unit
    // scale — leaves every brush untouched; a scene of thousands of shape
    // draws per frame should not rebuild its colors to discover that.
    if layer.alpha == 1.0
        && layer.color_filter.is_none()
        && layer_scale_x(layer) == 1.0
        && layer_scale_y(layer) == 1.0
    {
        return brush;
    }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
    //
    // `markers` is the recording scope's own marker count, trusted only when
    // its identity guard matches the vector the command actually returned
    // (see [`recorded_content_markers`]). `Some(0)` skips the marker scans
    // below outright — on a watch-class core, re-streaming a fresh
    // multi-thousand-primitive recording just to learn "no markers" is
    // measurable frame time. `None` scans exactly as before.
    let filter_content = |primitives: Vec<DrawPrimitive>, markers: Option<u32>| {
        if markers == Some(0) {
            return primitives;
        }
        // The marker scan is a cheap discriminant read; a canvas that never
        // calls `draw_content()` — every game scene — keeps its vector as-is
        // instead of moving thousands of primitives through a second one.
        if !primitives
            .iter()
            .any(|primitive| matches!(primitive, DrawPrimitive::Content))
        {
            return primitives;
        }
        let mut out = Vec::with_capacity(primitives.len());
        out.extend(
            primitives
                .into_iter()
                .filter(|primitive| !matches!(primitive, DrawPrimitive::Content)),
        );
        out
    };

    let split_with_content = |primitives: Vec<DrawPrimitive>, placement, markers: Option<u32>| {
        let last_content_idx = if markers == Some(0) {
            None
        } else {
            primitives
                .iter()
                .rposition(|primitive| matches!(primitive, DrawPrimitive::Content))
        };
        let Some(last_content_idx) = last_content_idx else {
            return if matches!(placement, DrawPlacement::Overlay) {
                filter_content(primitives, markers)
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

    // A note left dangling by some unrelated recording (one whose vector
    // never came through here) must not survive into the reads below, however
    // the allocator reuses addresses.
    clear_recorded_content_markers();
    match (placement, command) {
        (DrawPlacement::Behind, DrawCommand::Behind(func)) => {
            let primitives = func(size);
            let markers = recorded_content_markers(&primitives);
            filter_content(primitives, markers)
        }
        (DrawPlacement::Overlay, DrawCommand::Overlay(func)) => {
            let primitives = func(size);
            let markers = recorded_content_markers(&primitives);
            filter_content(primitives, markers)
        }
        (_, DrawCommand::WithContent(func)) => {
            let primitives = func(size);
            let markers = recorded_content_markers(&primitives);
            split_with_content(primitives, placement, markers)
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranpose_ui_graphics::{DrawScope, DrawScopeDefault, Rect};

    fn recorded_command(
        record: impl Fn(&mut dyn DrawScope) + 'static,
    ) -> Rc<dyn Fn(Size) -> Vec<DrawPrimitive>> {
        Rc::new(move |size| {
            let mut scope = DrawScopeDefault::new(size);
            record(&mut scope);
            scope.into_primitives()
        })
    }

    fn rect_at(x: f32) -> Rect {
        Rect {
            x,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        }
    }

    fn rect_xs(primitives: &[DrawPrimitive]) -> Vec<f32> {
        primitives
            .iter()
            .map(|primitive| match primitive {
                DrawPrimitive::Rect { rect, .. } => rect.x,
                other => panic!("unexpected primitive {other:?}"),
            })
            .collect()
    }

    #[test]
    fn marker_free_recording_passes_through() {
        let command = DrawCommand::Behind(recorded_command(|scope| {
            scope.draw_rect_at(rect_at(1.0), Brush::solid(Color::WHITE));
            scope.draw_rect_at(rect_at(2.0), Brush::solid(Color::WHITE));
        }));
        let out = primitives_for_placement(&command, DrawPlacement::Behind, Size::new(10.0, 10.0));
        assert_eq!(rect_xs(&out), [1.0, 2.0]);
    }

    #[test]
    fn recorded_markers_still_split_content_placements() {
        let with_content = recorded_command(|scope| {
            scope.draw_rect_at(rect_at(1.0), Brush::solid(Color::WHITE));
            scope.draw_content();
            scope.draw_rect_at(rect_at(2.0), Brush::solid(Color::WHITE));
        });
        let command = DrawCommand::WithContent(with_content);
        let size = Size::new(10.0, 10.0);
        let behind = primitives_for_placement(&command, DrawPlacement::Behind, size);
        assert_eq!(rect_xs(&behind), [1.0]);
        let overlay = primitives_for_placement(&command, DrawPlacement::Overlay, size);
        assert_eq!(rect_xs(&overlay), [2.0]);
    }

    #[test]
    fn hand_built_vectors_fall_back_to_scanning() {
        // No recording scope, so no note exists: the marker must still be
        // found and stripped by the scan path.
        let command = DrawCommand::Behind(Rc::new(|_size| {
            vec![
                DrawPrimitive::Rect {
                    rect: rect_at(1.0),
                    brush: Brush::solid(Color::WHITE),
                    stroke: None,
                },
                DrawPrimitive::Content,
                DrawPrimitive::Rect {
                    rect: rect_at(2.0),
                    brush: Brush::solid(Color::WHITE),
                    stroke: None,
                },
            ]
        }));
        let out = primitives_for_placement(&command, DrawPlacement::Behind, Size::new(10.0, 10.0));
        assert_eq!(rect_xs(&out), [1.0, 2.0]);
    }
}
