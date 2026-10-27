//! Brush definitions for painting (solid colors, gradients, etc.)

use std::sync::Arc;

use crate::{
    brush_sampler::BrushSampler,
    color::Color,
    geometry::{Point, Rect},
    render_effect::TileMode,
};

/// Describes how a shape, path or text is filled: a flat color or a gradient. Compose's `Brush`.
#[derive(Clone, Debug, PartialEq)]
pub enum Brush {
    /// A single flat color. Compose's `SolidColor`.
    Solid(Color),
    /// Colors blend along the line from `start` to `end`. Compose's `Brush.linearGradient`.
    LinearGradient {
        /// Colors to blend between, in order, from `start` to `end`.
        colors: Arc<[Color]>,
        /// Positions of `colors` along the line from `start` to `end`, from 0.0 to 1.0; `None`
        /// spaces the colors evenly.
        stops: Option<Arc<[f32]>>,
        /// Where the first color is reached, in logical pixels relative to the top-left corner of
        /// the painted area. An infinite coordinate means the far edge of that area.
        start: Point,
        /// Where the last color is reached, in logical pixels relative to the top-left corner of
        /// the painted area. An infinite coordinate means the far edge of that area.
        end: Point,
        /// How the gradient continues beyond `start` and `end`.
        tile_mode: TileMode,
    },
    /// Colors blend with distance from `center`. Compose's `Brush.radialGradient`.
    RadialGradient {
        /// Colors to blend between, in order, from `center` outward.
        colors: Arc<[Color]>,
        /// Positions of `colors` from 0.0 to 1.0; `None` spaces the colors evenly.
        stops: Option<Arc<[f32]>>,
        /// Gradient center in logical pixels, relative to the top-left corner of the painted area.
        center: Point,
        /// Distance in logical pixels from `center` at which the last color is reached.
        radius: f32,
        /// How the gradient continues beyond `radius`.
        tile_mode: TileMode,
    },
    /// Colors blend by angle around `center`, the first at 3 o'clock and the
    /// rest clockwise from it. Compose's `Brush.sweepGradient`.
    SweepGradient {
        /// Colors to blend between, in order.
        colors: Arc<[Color]>,
        /// Positions of `colors` from 0.0 to 1.0; `None` spaces the colors evenly.
        stops: Option<Arc<[f32]>>,
        /// Gradient center in logical pixels, relative to the top-left corner of the painted area.
        center: Point,
    },
}

fn split_color_stops(color_stops: Vec<(f32, Color)>) -> (Vec<Color>, Vec<f32>) {
    let mut colors = Vec::with_capacity(color_stops.len());
    let mut stops = Vec::with_capacity(color_stops.len());
    for (stop, color) in color_stops {
        colors.push(color);
        stops.push(stop);
    }
    (colors, stops)
}

/// A colour paints as a solid brush wherever a brush is taken, as Compose
/// takes a `Color` beside each `Brush` overload.
impl From<Color> for Brush {
    fn from(color: Color) -> Self {
        Self::Solid(color)
    }
}

impl Brush {
    /// Creates a brush that paints one flat color.
    pub fn solid(color: Color) -> Self {
        Self::Solid(color)
    }

    /// The brush's colours: its one colour, or a gradient's stops in order.
    pub fn colors(&self) -> &[Color] {
        match self {
            Self::Solid(color) => std::slice::from_ref(color),
            Self::LinearGradient { colors, .. }
            | Self::RadialGradient { colors, .. }
            | Self::SweepGradient { colors, .. } => colors,
        }
    }

    /// The brush with each colour through `paint`. A gradient keeps its colour
    /// list, shared, when `paint` leaves every colour's bits as they are.
    pub fn map_colors(self, paint: impl Fn(Color) -> Color) -> Self {
        let map = |colors: Arc<[Color]>| {
            let bits = |color: Color| [color.0, color.1, color.2, color.3].map(f32::to_bits);
            if colors
                .iter()
                .all(|&color| bits(paint(color)) == bits(color))
            {
                colors
            } else {
                colors.iter().map(|&color| paint(color)).collect()
            }
        };
        match self {
            Self::Solid(color) => Self::Solid(paint(color)),
            Self::LinearGradient {
                colors,
                stops,
                start,
                end,
                tile_mode,
            } => Self::LinearGradient {
                colors: map(colors),
                stops,
                start,
                end,
                tile_mode,
            },
            Self::RadialGradient {
                colors,
                stops,
                center,
                radius,
                tile_mode,
            } => Self::RadialGradient {
                colors: map(colors),
                stops,
                center,
                radius,
                tile_mode,
            },
            Self::SweepGradient {
                colors,
                stops,
                center,
            } => Self::SweepGradient {
                colors: map(colors),
                stops,
                center,
            },
        }
    }

    /// Whether the brush paints anything: some colour of it is not
    /// transparent.
    pub fn paints(&self) -> bool {
        self.colors().iter().any(|color| color.3 > 0.0)
    }

    /// Creates a linear gradient that defaults to Compose semantics:
    /// start at `(0,0)`, end at `(+inf,+inf)`, and `TileMode::Clamp`.
    pub fn linear_gradient(colors: Vec<Color>) -> Self {
        Self::linear_gradient_with_tile_mode(
            colors,
            Point { x: 0.0, y: 0.0 },
            Point {
                x: f32::INFINITY,
                y: f32::INFINITY,
            },
            TileMode::Clamp,
        )
    }

    /// Creates a linear gradient that spreads `colors` evenly from `start` to `end`, clamping
    /// outside that line.
    pub fn linear_gradient_range(colors: Vec<Color>, start: Point, end: Point) -> Self {
        Self::linear_gradient_with_tile_mode(colors, start, end, TileMode::Clamp)
    }

    /// Creates a linear gradient that spreads `colors` evenly from `start` to `end`, repeating or
    /// clamping outside that line as `tile_mode` says.
    pub fn linear_gradient_with_tile_mode(
        colors: Vec<Color>,
        start: Point,
        end: Point,
        tile_mode: TileMode,
    ) -> Self {
        Self::LinearGradient {
            colors: colors.into(),
            stops: None,
            start,
            end,
            tile_mode,
        }
    }

    /// Creates a linear gradient from `(position, color)` pairs, with positions from 0.0 at `start`
    /// to 1.0 at `end`.
    pub fn linear_gradient_stops(
        color_stops: Vec<(f32, Color)>,
        start: Point,
        end: Point,
        tile_mode: TileMode,
    ) -> Self {
        let (colors, stops) = split_color_stops(color_stops);
        Self::LinearGradient {
            colors: colors.into(),
            stops: Some(stops.into()),
            start,
            end,
            tile_mode,
        }
    }

    /// Creates a vertical gradient that spreads `colors` evenly from `start_y` to `end_y`, clamping
    /// outside that range.
    pub fn vertical_gradient(colors: Vec<Color>, start_y: f32, end_y: f32) -> Self {
        Self::vertical_gradient_tiled(colors, start_y, end_y, TileMode::Clamp)
    }

    /// Creates a vertical gradient that spreads `colors` evenly from `start_y` to `end_y`,
    /// repeating or clamping outside that range as `tile_mode` says.
    pub fn vertical_gradient_tiled(
        colors: Vec<Color>,
        start_y: f32,
        end_y: f32,
        tile_mode: TileMode,
    ) -> Self {
        Self::linear_gradient_with_tile_mode(
            colors,
            Point { x: 0.0, y: start_y },
            Point { x: 0.0, y: end_y },
            tile_mode,
        )
    }

    /// Creates a vertical gradient across the full height of the painted area, clamping at both
    /// ends.
    pub fn vertical_gradient_default(colors: Vec<Color>) -> Self {
        Self::vertical_gradient_tiled(colors, 0.0, f32::INFINITY, TileMode::Clamp)
    }

    /// Creates a vertical gradient from `(position, color)` pairs, with positions from 0.0 at
    /// `start_y` to 1.0 at `end_y`.
    pub fn vertical_gradient_stops(
        color_stops: Vec<(f32, Color)>,
        start_y: f32,
        end_y: f32,
        tile_mode: TileMode,
    ) -> Self {
        Self::linear_gradient_stops(
            color_stops,
            Point { x: 0.0, y: start_y },
            Point { x: 0.0, y: end_y },
            tile_mode,
        )
    }

    /// Creates a horizontal gradient that spreads `colors` evenly from `start_x` to `end_x`,
    /// clamping outside that range.
    pub fn horizontal_gradient(colors: Vec<Color>, start_x: f32, end_x: f32) -> Self {
        Self::horizontal_gradient_tiled(colors, start_x, end_x, TileMode::Clamp)
    }

    /// Creates a horizontal gradient that spreads `colors` evenly from `start_x` to `end_x`,
    /// repeating or clamping outside that range as `tile_mode` says.
    pub fn horizontal_gradient_tiled(
        colors: Vec<Color>,
        start_x: f32,
        end_x: f32,
        tile_mode: TileMode,
    ) -> Self {
        Self::linear_gradient_with_tile_mode(
            colors,
            Point { x: start_x, y: 0.0 },
            Point { x: end_x, y: 0.0 },
            tile_mode,
        )
    }

    /// Creates a horizontal gradient across the full width of the painted area, clamping at both
    /// ends.
    pub fn horizontal_gradient_default(colors: Vec<Color>) -> Self {
        Self::horizontal_gradient_tiled(colors, 0.0, f32::INFINITY, TileMode::Clamp)
    }

    /// Creates a horizontal gradient from `(position, color)` pairs, with positions from 0.0 at
    /// `start_x` to 1.0 at `end_x`.
    pub fn horizontal_gradient_stops(
        color_stops: Vec<(f32, Color)>,
        start_x: f32,
        end_x: f32,
        tile_mode: TileMode,
    ) -> Self {
        Self::linear_gradient_stops(
            color_stops,
            Point { x: start_x, y: 0.0 },
            Point { x: end_x, y: 0.0 },
            tile_mode,
        )
    }

    /// Creates a radial gradient that spreads `colors` evenly from `center` out to `radius`,
    /// clamping beyond it.
    pub fn radial_gradient(colors: Vec<Color>, center: Point, radius: f32) -> Self {
        Self::radial_gradient_tiled(colors, center, radius, TileMode::Clamp)
    }

    /// Creates a radial gradient that spreads `colors` evenly from `center` out to `radius`,
    /// repeating or clamping beyond it as `tile_mode` says.
    pub fn radial_gradient_tiled(
        colors: Vec<Color>,
        center: Point,
        radius: f32,
        tile_mode: TileMode,
    ) -> Self {
        Self::RadialGradient {
            colors: colors.into(),
            stops: None,
            center,
            radius,
            tile_mode,
        }
    }

    /// Creates a radial gradient from `(position, color)` pairs, with positions from 0.0 at the
    /// center to 1.0 at `radius`.
    pub fn radial_gradient_stops(
        color_stops: Vec<(f32, Color)>,
        center: Point,
        radius: f32,
        tile_mode: TileMode,
    ) -> Self {
        let (colors, stops) = split_color_stops(color_stops);
        Self::RadialGradient {
            colors: colors.into(),
            stops: Some(stops.into()),
            center,
            radius,
            tile_mode,
        }
    }

    /// Creates a sweep gradient that spreads `colors` evenly around `center`.
    pub fn sweep_gradient(colors: Vec<Color>, center: Point) -> Self {
        Self::SweepGradient {
            colors: colors.into(),
            stops: None,
            center,
        }
    }

    /// Creates a sweep gradient from `(position, color)` pairs, with positions from 0.0 to 1.0
    /// around `center`.
    pub fn sweep_gradient_stops(color_stops: Vec<(f32, Color)>, center: Point) -> Self {
        let (colors, stops) = split_color_stops(color_stops);
        Self::SweepGradient {
            colors: colors.into(),
            stops: Some(stops.into()),
            center,
        }
    }

    /// The color this brush paints at `point` when it fills `rect`, before
    /// any dithering. Gradient coordinates are offsets from `rect`'s
    /// top-left corner, and an infinite one is `rect`'s far edge. Where a
    /// [`TileMode::Decal`] gradient paints nothing the color is transparent.
    /// [`BrushSampler`] gives the same colors for many points of one rect.
    pub fn color_at(&self, rect: Rect, point: Point) -> Color {
        BrushSampler::new(self, rect).color_at(point)
    }
}

#[cfg(test)]
#[path = "tests/brush_tests.rs"]
mod tests;
