//! Brush definitions for painting (solid colors, gradients, etc.)

use crate::color::Color;
use crate::geometry::Point;
use crate::render_effect::TileMode;

#[derive(Clone, Debug, PartialEq)]
pub enum Brush {
    Solid(Color),
    LinearGradient {
        colors: Vec<Color>,
        stops: Option<Vec<f32>>,
        start: Point,
        end: Point,
        tile_mode: TileMode,
    },
    RadialGradient {
        colors: Vec<Color>,
        stops: Option<Vec<f32>>,
        center: Point,
        radius: f32,
        tile_mode: TileMode,
    },
    SweepGradient {
        colors: Vec<Color>,
        stops: Option<Vec<f32>>,
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

impl Brush {
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

    pub fn linear_gradient_range(colors: Vec<Color>, start: Point, end: Point) -> Self {
        Self::linear_gradient_with_tile_mode(colors, start, end, TileMode::Clamp)
    }

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

    pub fn vertical_gradient(colors: Vec<Color>, start_y: f32, end_y: f32) -> Self {
        Self::vertical_gradient_tiled(colors, start_y, end_y, TileMode::Clamp)
    }

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

    pub fn vertical_gradient_default(colors: Vec<Color>) -> Self {
        Self::vertical_gradient_tiled(colors, 0.0, f32::INFINITY, TileMode::Clamp)
    }

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

    pub fn horizontal_gradient(colors: Vec<Color>, start_x: f32, end_x: f32) -> Self {
        Self::horizontal_gradient_tiled(colors, start_x, end_x, TileMode::Clamp)
    }

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

    pub fn horizontal_gradient_default(colors: Vec<Color>) -> Self {
        Self::horizontal_gradient_tiled(colors, 0.0, f32::INFINITY, TileMode::Clamp)
    }

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

    pub fn radial_gradient(colors: Vec<Color>, center: Point, radius: f32) -> Self {
        Self::radial_gradient_tiled(colors, center, radius, TileMode::Clamp)
    }

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

    pub fn sweep_gradient(colors: Vec<Color>, center: Point) -> Self {
        Brush::SweepGradient {
            colors,
            stops: None,
            center,
        }
    }

    pub fn sweep_gradient_stops(color_stops: Vec<(f32, Color)>, center: Point) -> Self {
        let (colors, stops) = split_color_stops(color_stops);
        Brush::SweepGradient {
            colors,
            stops: Some(stops),
            center,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sweep_gradient_construction() {
        let colors = vec![Color(1.0, 0.0, 0.0, 1.0), Color(0.0, 0.0, 1.0, 1.0)];
        let center = Point { x: 50.0, y: 50.0 };
        let brush = Brush::sweep_gradient(colors.clone(), center);
        match brush {
            Brush::SweepGradient {
                colors: c,
                stops,
                center: p,
            } => {
                assert_eq!(c, colors);
                assert_eq!(p, center);
                assert!(stops.is_none());
            }
            _ => panic!("expected SweepGradient"),
        }
    }

    #[test]
    fn brush_clone_eq() {
        let a = Brush::solid(Color(1.0, 0.0, 0.0, 1.0));
        let b = a.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn vertical_gradient_construction() {
        let colors = vec![Color(0.0, 0.0, 0.0, 1.0), Color(0.0, 0.0, 0.0, 0.0)];
        let brush = Brush::vertical_gradient(colors.clone(), 24.0, 64.0);
        match brush {
            Brush::LinearGradient {
                colors: c,
                stops,
                start,
                end,
                tile_mode,
            } => {
                assert_eq!(c, colors);
                assert!(stops.is_none());
                assert_eq!(tile_mode, TileMode::Clamp);
                assert_eq!(start, Point { x: 0.0, y: 24.0 });
                assert_eq!(end, Point { x: 0.0, y: 64.0 });
            }
            _ => panic!("expected LinearGradient"),
        }
    }

    #[test]
    fn linear_gradient_defaults_to_infinite_end() {
        let brush = Brush::linear_gradient(vec![Color::BLACK, Color::WHITE]);
        match brush {
            Brush::LinearGradient { start, end, .. } => {
                assert_eq!(start, Point { x: 0.0, y: 0.0 });
                assert!(end.x.is_infinite());
                assert!(end.y.is_infinite());
            }
            _ => panic!("expected LinearGradient"),
        }
    }

    #[test]
    fn gradient_color_stops_are_stored() {
        let brush = Brush::linear_gradient_stops(
            vec![(0.0, Color::RED), (0.6, Color::GREEN), (1.0, Color::BLUE)],
            Point { x: 0.0, y: 0.0 },
            Point { x: 20.0, y: 10.0 },
            TileMode::Mirror,
        );

        match brush {
            Brush::LinearGradient {
                colors,
                stops,
                tile_mode,
                ..
            } => {
                assert_eq!(colors, vec![Color::RED, Color::GREEN, Color::BLUE]);
                assert_eq!(stops, Some(vec![0.0, 0.6, 1.0]));
                assert_eq!(tile_mode, TileMode::Mirror);
            }
            _ => panic!("expected LinearGradient"),
        }
    }
}
