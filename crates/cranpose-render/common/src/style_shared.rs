use std::ops::Range;

use cranpose_ui::{Brush, DrawCommand, DrawCommandFn};
use cranpose_ui_graphics::{
    Color, ColorFilter, CommandRecording, CornerRadii, DrawPrimitive, GraphicsLayer, Size,
};

pub use crate::graph::quad_bounds;
use crate::layer_transform::{layer_scale_x, layer_scale_y, layer_uniform_scale};

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
    apply_paint_to_color(color, layer.alpha, layer.color_filter.as_ref())
}

/// The colour a primitive is painted in under a layer's `alpha` and
/// `color_filter`, the only parts of a layer a colour takes; see
/// [`apply_layer_to_color`].
pub fn apply_paint_to_color(color: Color, alpha: f32, color_filter: Option<&ColorFilter>) -> Color {
    let color = color.srgb_8bit();
    let color = Color(color.0, color.1, color.2, (color.3 * alpha).clamp(0.0, 1.0));
    match color_filter {
        Some(filter) => {
            let [r, g, b, a] = filter.apply_rgba([color.0, color.1, color.2, color.3]);
            Color(r, g, b, a)
        }
        None => color,
    }
}

/// Combines two optional color filters into one. With only one present, that filter is returned.
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

/// Resolves `brush` against `layer`: its colors take the layer's alpha and color filter, and a
/// gradient's geometry takes the layer's scale.
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
    /// A solid color, already layer-resolved.
    Solid(Color),
    /// A non-solid brush (gradients), already layer-resolved.
    Other(Brush),
}

impl ResolvedBrush {
    /// Wraps `brush` as `Solid` when it is a solid color and as `Other` otherwise.
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

/// Multiplies every corner radius by `scale`.
pub fn scale_corner_radii(radii: CornerRadii, scale: f32) -> CornerRadii {
    CornerRadii {
        top_left: radii.top_left * scale,
        top_right: radii.top_right * scale,
        bottom_right: radii.bottom_right * scale,
        bottom_left: radii.bottom_left * scale,
    }
}

/// Which half of a draw command is drawn, relative to the content of the node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DrawPlacement {
    /// Drawn behind the node's content.
    Behind,
    /// Drawn over the node's content.
    Overlay,
}

/// Records `command` at `size` and returns the primitives that `placement` draws, or an empty
/// list when the command has no part for that placement.
pub fn primitives_for_placement(
    command: &DrawCommand,
    placement: DrawPlacement,
    size: Size,
) -> Vec<DrawPrimitive> {
    recording_for_placement_reusing(command, placement, size, CommandRecording::default)
        .map(|(recording, segments)| recording.primitives(segments).collect())
        .unwrap_or_default()
}

/// Records `command` into `storage`, a recording the caller kept from an
/// earlier frame so its buffers keep the capacity they earned, and names
/// the segments `placement` draws: everything for a behind or overlay
/// command, the part before or after the last content marker for a
/// with-content command. `None` when the command has no `placement` half,
/// in which case storage is not acquired and nothing is recorded.
pub fn recording_for_placement_reusing(
    command: &DrawCommand,
    placement: DrawPlacement,
    size: Size,
    storage: impl FnOnce() -> CommandRecording,
) -> Option<(CommandRecording, Range<u32>)> {
    let record = move |func: &DrawCommandFn| {
        let mut scope = cranpose_ui::command_draw_scope_reusing(size, storage());
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
#[path = "tests/style_shared_tests.rs"]
mod tests;
