use std::{ops::Range, rc::Rc};

use cranpose_foundation::PointerEvent;
use cranpose_ui::{Brush, DrawCommand, DrawCommandFn, LayoutNodeData, ModifierNodeSlices};
use cranpose_ui_graphics::{
    BlendMode, Color, ColorFilter, CommandRecording, CompositingStrategy, CornerRadii,
    DrawPrimitive, GraphicsLayer, Point, RoundedCornerShape, Size,
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
            render_effect: layer.render_effect,
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

/// The colour a primitive is actually painted in, once this layer has had its
/// say.
///
/// The colour is snapped to eight bits *first*, because that is where the
/// platform's own colour type already is by the time anything paints with it
/// (see [`Color::srgb_8bit`]). Only then does the layer's alpha multiply it.
/// The order is the whole point: an isolated layer's contents land in an 8-bit
/// buffer and the alpha multiplies whole channel values, so
/// `round(round(c * 255) * a)` and not `round(c * 255 * a)`.
pub fn apply_layer_to_color(color: Color, layer: &GraphicsLayer) -> Color {
    let color = color.srgb_8bit();
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
    if layer.alpha == 1.0
        && layer.color_filter.is_none()
        && layer_scale_x(layer) == 1.0
        && layer_scale_y(layer) == 1.0
    {
        return map_brush_colors(brush, Color::srgb_8bit);
    }
    map_brush_colors(scale_brush_geometry(brush, layer), |color| {
        apply_layer_to_color(color, layer)
    })
}

fn map_brush_colors(brush: Brush, paint: impl Fn(Color) -> Color) -> Brush {
    match brush {
        Brush::Solid(color) => Brush::solid(paint(color)),
        Brush::LinearGradient {
            colors,
            stops,
            start,
            end,
            tile_mode,
        } => Brush::LinearGradient {
            colors: colors.into_iter().map(paint).collect(),
            stops,
            start,
            end,
            tile_mode,
        },
        Brush::RadialGradient {
            colors,
            stops,
            center,
            radius,
            tile_mode,
        } => Brush::RadialGradient {
            colors: colors.into_iter().map(paint).collect(),
            stops,
            center,
            radius,
            tile_mode,
        },
        Brush::SweepGradient {
            colors,
            stops,
            center,
        } => Brush::SweepGradient {
            colors: colors.into_iter().map(paint).collect(),
            stops,
            center,
        },
    }
}

fn scale_brush_geometry(brush: Brush, layer: &GraphicsLayer) -> Brush {
    let scale_x = layer_scale_x(layer);
    let scale_y = layer_scale_y(layer);
    let uniform_scale = layer_uniform_scale(layer);

    match brush {
        Brush::Solid(color) => Brush::Solid(color),
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
                colors,
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
                colors,
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
                colors,
                stops,
                center,
            }
        }
    }
}

/// A layer-resolved brush, split at the solid/gradient boundary.
///
/// The per-frame shape emit produces one of these for every draw: the solid
/// case — effectively all of a heavy animated scene — carries its color
/// inline, so emitting a shape neither clones a `Brush` nor leaves an enum
/// with heap-carrying variants for frame teardown to walk. Only the rare
/// gradient still travels as a cloned [`Brush`].
#[derive(Clone, Debug, PartialEq)]
pub enum ResolvedBrush {
    Solid(Color),
    /// A non-solid brush (gradients), already layer-resolved.
    Other(Brush),
}

impl ResolvedBrush {
    pub fn from_brush(brush: Brush) -> Self {
        match brush {
            Brush::Solid(color) => Self::Solid(color),
            other => Self::Other(other),
        }
    }

    /// The plain `Brush` this resolved form stands for — same values,
    /// reassembled for consumers that keep speaking `Brush`.
    pub fn into_brush(self) -> Brush {
        match self {
            Self::Solid(color) => Brush::Solid(color),
            Self::Other(brush) => brush,
        }
    }
}

/// [`apply_layer_to_brush`] without the solid-brush clone: the borrowed
/// brush's color is copied (or layer-adjusted) inline, and only gradients
/// are cloned. Produces exactly the values `apply_layer_to_brush` would —
/// both branches below mirror its fast path and its `Solid` arm verbatim.
pub fn resolve_layer_brush(brush: &Brush, layer: &GraphicsLayer) -> ResolvedBrush {
    match brush {
        Brush::Solid(color) => {
            if layer.alpha == 1.0
                && layer.color_filter.is_none()
                && layer_scale_x(layer) == 1.0
                && layer_scale_y(layer) == 1.0
            {
                ResolvedBrush::Solid(*color)
            } else {
                ResolvedBrush::Solid(apply_layer_to_color(*color, layer))
            }
        }
        other => ResolvedBrush::Other(apply_layer_to_brush(other.clone(), layer)),
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
    recording_for_placement_reusing(command, placement, size, CommandRecording::default())
        .map(|(recording, segments)| recording.primitives(segments).collect())
        .unwrap_or_default()
}

/// Records `command` into `storage`, a recording the caller kept from an
/// earlier frame so its buffers keep the capacity they earned, and names
/// the segments `placement` draws: everything for a behind or overlay
/// command, the part before or after the last content marker for a
/// with-content command. `None` when the command has no `placement` half,
/// in which case nothing was recorded.
pub fn recording_for_placement_reusing(
    command: &DrawCommand,
    placement: DrawPlacement,
    size: Size,
    storage: CommandRecording,
) -> Option<(CommandRecording, Range<u32>)> {
    let record = move |func: &DrawCommandFn| {
        let mut scope = cranpose_ui::command_draw_scope_reusing(size, storage);
        func(&mut scope);
        scope.finish()
    };
    match (placement, command) {
        (DrawPlacement::Behind, DrawCommand::Behind(func))
        | (DrawPlacement::Overlay, DrawCommand::Overlay(func)) => {
            let recording = record(func);
            let segments = recording.all_segments();
            Some((recording, segments))
        }
        (_, DrawCommand::WithContent(func)) => {
            let recording = record(func);
            let segments = recording.content_split(placement == DrawPlacement::Behind);
            Some((recording, segments))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use cranpose_ui_graphics::{DrawScope, DrawScopeDefault, Rect};

    use super::*;

    fn recorded_command(record: impl Fn(&mut dyn DrawScope) + 'static) -> DrawCommandFn {
        Rc::new(move |scope: &mut DrawScopeDefault| record(scope))
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
    fn reused_storage_records_identically_to_fresh() {
        let command = DrawCommand::WithContent(recorded_command(|scope| {
            scope.draw_rect_at(rect_at(1.0), Brush::solid(Color::WHITE));
            scope.draw_content();
            scope.draw_rect_at(rect_at(2.0), Brush::solid(Color::WHITE));
        }));
        let size = Size::new(10.0, 10.0);
        for placement in [DrawPlacement::Behind, DrawPlacement::Overlay] {
            let fresh = primitives_for_placement(&command, placement, size);
            let dirty = CommandRecording::from_primitives(vec![DrawPrimitive::Content; 8]);
            let (recording, segments) =
                recording_for_placement_reusing(&command, placement, size, dirty)
                    .expect("a with-content command records for both placements");
            let reused: Vec<DrawPrimitive> = recording.primitives(segments).collect();
            assert_eq!(fresh, reused);
        }
    }

    #[test]
    fn pushed_batches_keep_marker_count_authoritative() {
        let command = DrawCommand::Behind(Rc::new(|scope: &mut DrawScopeDefault| {
            scope.push_recorded(vec![
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
            ]);
        }));
        let out = primitives_for_placement(&command, DrawPlacement::Behind, Size::new(10.0, 10.0));
        assert_eq!(rect_xs(&out), [1.0, 2.0]);
    }
}
