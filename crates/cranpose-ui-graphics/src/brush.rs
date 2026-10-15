//! Brush definitions for painting (solid colors, gradients, etc.)

use crate::{
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
        colors: Vec<Color>,
        /// Positions of `colors` along the line from `start` to `end`, from 0.0 to 1.0; `None`
        /// spaces the colors evenly.
        stops: Option<Vec<f32>>,
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
        colors: Vec<Color>,
        /// Positions of `colors` from 0.0 to 1.0; `None` spaces the colors evenly.
        stops: Option<Vec<f32>>,
        /// Gradient center in logical pixels, relative to the top-left corner of the painted area.
        center: Point,
        /// Distance in logical pixels from `center` at which the last color is reached.
        radius: f32,
        /// How the gradient continues beyond `radius`.
        tile_mode: TileMode,
    },
    /// Colors blend by angle around `center`. Compose's `Brush.sweepGradient`.
    SweepGradient {
        /// Colors to blend between, in order.
        colors: Vec<Color>,
        /// Positions of `colors` from 0.0 to 1.0; `None` spaces the colors evenly.
        stops: Option<Vec<f32>>,
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
        Brush::Solid(color)
    }
}

impl Brush {
    /// Creates a brush that paints one flat color.
    pub fn solid(color: Color) -> Self {
        Brush::Solid(color)
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
        Brush::LinearGradient {
            colors,
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
        Brush::LinearGradient {
            colors,
            stops: Some(stops),
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
        Brush::RadialGradient {
            colors,
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
        Brush::RadialGradient {
            colors,
            stops: Some(stops),
            center,
            radius,
            tile_mode,
        }
    }

    /// Creates a sweep gradient that spreads `colors` evenly around `center`.
    pub fn sweep_gradient(colors: Vec<Color>, center: Point) -> Self {
        Brush::SweepGradient {
            colors,
            stops: None,
            center,
        }
    }

    /// Creates a sweep gradient from `(position, color)` pairs, with positions from 0.0 to 1.0
    /// around `center`.
    pub fn sweep_gradient_stops(color_stops: Vec<(f32, Color)>, center: Point) -> Self {
        let (colors, stops) = split_color_stops(color_stops);
        Brush::SweepGradient {
            colors,
            stops: Some(stops),
            center,
        }
    }
}

impl Brush {
    /// The color this brush paints at `point` when it fills `rect`, before
    /// any dithering. Gradient coordinates are offsets from `rect`'s
    /// top-left corner, and an infinite one is `rect`'s far edge. Where a
    /// [`TileMode::Decal`] gradient paints nothing the color is transparent.
    pub fn color_at(&self, rect: Rect, point: Point) -> Color {
        let (x, y) = (point.x, point.y);
        let (colors, stops, t, tile_mode) = match self {
            Brush::Solid(color) => return *color,
            Brush::LinearGradient {
                colors,
                stops,
                start,
                end,
                tile_mode,
            } => {
                let sx = resolve_gradient_point(rect.x, rect.width, start.x);
                let sy = resolve_gradient_point(rect.y, rect.height, start.y);
                let ex = resolve_gradient_point(rect.x, rect.width, end.x);
                let ey = resolve_gradient_point(rect.y, rect.height, end.y);
                let (dx, dy) = (ex - sx, ey - sy);
                let denom = (dx * dx + dy * dy).max(f32::EPSILON);
                let t = ((x - sx) * dx + (y - sy) * dy) / denom;
                (colors, stops, t, *tile_mode)
            }
            Brush::RadialGradient {
                colors,
                stops,
                center,
                radius,
                tile_mode,
            } => {
                let dx = x - (rect.x + center.x);
                let dy = y - (rect.y + center.y);
                let t = (dx * dx + dy * dy).sqrt() / radius.max(f32::EPSILON);
                (colors, stops, t, *tile_mode)
            }
            Brush::SweepGradient {
                colors,
                stops,
                center,
            } => {
                let angle = (y - (rect.y + center.y)).atan2(x - (rect.x + center.x));
                let t = angle / std::f32::consts::TAU + 0.5;
                (colors, stops, t, TileMode::Clamp)
            }
        };
        match tiled_gradient_t(t, tile_mode) {
            Some(t) => interpolate_colors(colors, stops.as_deref(), t),
            None => TRANSPARENT,
        }
    }
}

const TRANSPARENT: Color = Color(0.0, 0.0, 0.0, 0.0);

fn resolve_gradient_point(origin: f32, extent: f32, value: f32) -> f32 {
    if value.is_finite() {
        origin + value
    } else if value.is_sign_positive() {
        origin + extent
    } else {
        origin
    }
}

/// Where `t` lands in the gradient's one span under `tile_mode`; `None`
/// past a decal gradient's ends.
fn tiled_gradient_t(t: f32, tile_mode: TileMode) -> Option<f32> {
    match tile_mode {
        TileMode::Clamp => Some(t.clamp(0.0, 1.0)),
        TileMode::Decal => (0.0..=1.0).contains(&t).then_some(t),
        TileMode::Repeated => Some(t.rem_euclid(1.0)),
        TileMode::Mirror => {
            let wrapped = t.rem_euclid(2.0);
            Some(if wrapped <= 1.0 {
                wrapped
            } else {
                2.0 - wrapped
            })
        }
    }
}

fn interpolate_colors(colors: &[Color], stops: Option<&[f32]>, t: f32) -> Color {
    let (Some(&first), Some(&last)) = (colors.first(), colors.last()) else {
        return TRANSPARENT;
    };
    if colors.len() == 1 {
        return first;
    }
    let clamped = t.clamp(0.0, 1.0);
    if let Some(stops) = stops.filter(|stops| stops.len() == colors.len()) {
        if stops.first().is_some_and(|&start| clamped <= start) {
            return first;
        }
        for (pair, span) in colors.windows(2).zip(stops.windows(2)) {
            if clamped <= span[1] {
                let width = (span[1] - span[0]).max(f32::EPSILON);
                return lerp_color(
                    pair[0],
                    pair[1],
                    ((clamped - span[0]) / width).clamp(0.0, 1.0),
                );
            }
        }
        return last;
    }
    let scaled = clamped * (colors.len() - 1) as f32;
    let index = scaled.floor() as usize;
    match colors.get(index..index + 2) {
        Some(&[from, to]) => lerp_color(from, to, scaled - index as f32),
        _ => last,
    }
}

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    let lerp = |start: f32, end: f32| start + (end - start) * t;
    Color(
        lerp(a.0, b.0),
        lerp(a.1, b.1),
        lerp(a.2, b.2),
        lerp(a.3, b.3),
    )
}

#[cfg(test)]
#[path = "tests/brush_tests.rs"]
mod tests;
