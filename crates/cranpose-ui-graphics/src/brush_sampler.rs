//! A brush laid over the rect it fills, for the colors of many points.

use crate::{
    brush::Brush,
    color::Color,
    geometry::{Point, Rect},
    render_effect::TileMode,
    whole,
};

/// A [`Brush`] laid over the rect it fills: the color of each point is the
/// one [`Brush::color_at`] gives, with the work that depends only on the
/// brush and the rect done once, as a renderer that colors every pixel of a
/// shape needs it.
#[derive(Clone, Copy, Debug)]
pub struct BrushSampler<'a> {
    paint: Paint<'a>,
}

#[derive(Clone, Copy, Debug)]
enum Paint<'a> {
    Solid(Color),
    // The gradient most brushes are, which takes its own short steps.
    LinearPair(LinearPair),
    Gradient {
        colors: &'a [Color],
        stops: Option<&'a [f32]>,
        tile_mode: TileMode,
        shape: GradientShape,
    },
}

// A linear gradient of two colors, clamped past its ends, with no stops of
// its own: `first` at `start`, `last` at `(dx, dy)` from it.
#[derive(Clone, Copy, Debug)]
struct LinearPair {
    start: Point,
    dx: f32,
    dy: f32,
    length_squared: f32,
    first: Color,
    last: Color,
    // `last - first`, a channel at a time, as the interpolation takes it.
    step: Color,
}

impl LinearPair {
    // The color the general steps give: `t` clamped, `last` at the end and
    // `first` moved toward `last` by `t` elsewhere, in the same operations.
    fn color_at(&self, point: Point) -> Color {
        let t = ((point.x - self.start.x) * self.dx + (point.y - self.start.y) * self.dy)
            / self.length_squared;
        let t = t.clamp(0.0, 1.0);
        if t >= 1.0 {
            return self.last;
        }
        let (first, step) = (self.first, self.step);
        Color(
            first.0 + step.0 * t,
            first.1 + step.1 * t,
            first.2 + step.2 * t,
            first.3 + step.3 * t,
        )
    }
}

// Where a point falls along a gradient.
#[derive(Clone, Copy, Debug)]
enum GradientShape {
    // Along `(dx, dy)` from `start`; `length_squared` is that step's
    // squared length.
    Linear {
        start: Point,
        dx: f32,
        dy: f32,
        length_squared: f32,
    },
    Radial {
        center: Point,
        radius: f32,
    },
    Sweep {
        center: Point,
    },
}

impl<'a> BrushSampler<'a> {
    /// `brush` filling `rect`.
    pub fn new(brush: &'a Brush, rect: Rect) -> Self {
        let paint = match brush {
            Brush::Solid(color) => Paint::Solid(*color),
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
                let start = Point::new(sx, sy);
                let length_squared = (dx * dx + dy * dy).max(f32::EPSILON);
                // Stops of another count than the colors' go unread.
                let own_stops = stops
                    .as_ref()
                    .is_some_and(|stops| stops.len() == colors.len());
                match **colors {
                    [first, last] if *tile_mode == TileMode::Clamp && !own_stops => {
                        Paint::LinearPair(LinearPair {
                            start,
                            dx,
                            dy,
                            length_squared,
                            first,
                            last,
                            step: Color(
                                last.0 - first.0,
                                last.1 - first.1,
                                last.2 - first.2,
                                last.3 - first.3,
                            ),
                        })
                    }
                    _ => Paint::Gradient {
                        colors,
                        stops: stops.as_deref(),
                        tile_mode: *tile_mode,
                        shape: GradientShape::Linear {
                            start,
                            dx,
                            dy,
                            length_squared,
                        },
                    },
                }
            }
            Brush::RadialGradient {
                colors,
                stops,
                center,
                radius,
                tile_mode,
            } => Paint::Gradient {
                colors,
                stops: stops.as_deref(),
                tile_mode: *tile_mode,
                shape: GradientShape::Radial {
                    center: Point::new(rect.x + center.x, rect.y + center.y),
                    radius: radius.max(f32::EPSILON),
                },
            },
            Brush::SweepGradient {
                colors,
                stops,
                center,
            } => Paint::Gradient {
                colors,
                stops: stops.as_deref(),
                tile_mode: TileMode::Clamp,
                shape: GradientShape::Sweep {
                    center: Point::new(rect.x + center.x, rect.y + center.y),
                },
            },
        };
        Self { paint }
    }

    /// The color at `point`, before any dithering.
    pub fn color_at(&self, point: Point) -> Color {
        let (colors, stops, tile_mode, shape) = match self.paint {
            Paint::Solid(color) => return color,
            Paint::LinearPair(pair) => return pair.color_at(point),
            Paint::Gradient {
                colors,
                stops,
                tile_mode,
                shape,
            } => (colors, stops, tile_mode, shape),
        };
        let (x, y) = (point.x, point.y);
        let t = match shape {
            GradientShape::Linear {
                start,
                dx,
                dy,
                length_squared,
            } => ((x - start.x) * dx + (y - start.y) * dy) / length_squared,
            GradientShape::Radial { center, radius } => {
                let dx = x - center.x;
                let dy = y - center.y;
                (dx * dx + dy * dy).sqrt() / radius
            }
            GradientShape::Sweep { center } => {
                let angle = (y - center.y).atan2(x - center.x);
                // From 3 o'clock, clockwise, as Compose's sweep.
                (angle / std::f32::consts::TAU).rem_euclid(1.0)
            }
        };
        match tiled_gradient_t(t, tile_mode) {
            Some(t) => interpolate_colors(colors, stops, t),
            None => TRANSPARENT,
        }
    }

    /// Whether the brush is one flat color.
    pub fn is_solid(&self) -> bool {
        matches!(self.paint, Paint::Solid(_))
    }

    /// Whether every point of a row has the same color: a solid brush, or a
    /// linear gradient that runs straight down, whose step across is zero.
    pub fn has_uniform_rows(&self) -> bool {
        match self.paint {
            Paint::Solid(_) => true,
            Paint::LinearPair(pair) => pair.dx == 0.0,
            Paint::Gradient { shape, .. } => {
                matches!(shape, GradientShape::Linear { dx, .. } if dx == 0.0)
            }
        }
    }

    /// Whether every point of a column has the same color: a solid brush, or
    /// a linear gradient that runs straight across, whose step down is zero.
    pub fn has_uniform_columns(&self) -> bool {
        match self.paint {
            Paint::Solid(_) => true,
            Paint::LinearPair(pair) => pair.dy == 0.0,
            Paint::Gradient { shape, .. } => {
                matches!(shape, GradientShape::Linear { dy, .. } if dy == 0.0)
            }
        }
    }

    /// Whether every color the brush gives is opaque: its colors are, and a
    /// gradient's ends repeat or stretch rather than turn transparent.
    pub fn is_opaque(&self) -> bool {
        match self.paint {
            Paint::Solid(color) => color.3 >= 1.0,
            Paint::LinearPair(pair) => pair.first.3 >= 1.0 && pair.last.3 >= 1.0,
            Paint::Gradient {
                colors, tile_mode, ..
            } => {
                tile_mode != TileMode::Decal
                    && !colors.is_empty()
                    && colors.iter().all(|color| color.3 >= 1.0)
            }
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

// Where `t` lands in the gradient's one span under `tile_mode`; `None`
// past a decal gradient's ends.
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
    let index = whole::floor(scaled) as usize;
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
