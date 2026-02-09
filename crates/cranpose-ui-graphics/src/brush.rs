//! Brush definitions for painting (solid colors, gradients, etc.)

use crate::color::Color;
use crate::geometry::Point;

#[derive(Clone, Debug, PartialEq)]
pub enum Brush {
    Solid(Color),
    LinearGradient(Vec<Color>),
    RadialGradient {
        colors: Vec<Color>,
        center: Point,
        radius: f32,
    },
    SweepGradient {
        colors: Vec<Color>,
        center: Point,
    },
}

impl Brush {
    pub fn solid(color: Color) -> Self {
        Brush::Solid(color)
    }

    pub fn linear_gradient(colors: Vec<Color>) -> Self {
        Brush::LinearGradient(colors)
    }

    pub fn radial_gradient(colors: Vec<Color>, center: Point, radius: f32) -> Self {
        Brush::RadialGradient {
            colors,
            center,
            radius,
        }
    }

    pub fn sweep_gradient(colors: Vec<Color>, center: Point) -> Self {
        Brush::SweepGradient { colors, center }
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
                center: p,
            } => {
                assert_eq!(c, colors);
                assert_eq!(p, center);
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
}
