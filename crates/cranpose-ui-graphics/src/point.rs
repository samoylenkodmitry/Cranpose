use std::ops::Sub;

/// A position in two dimensions, in logical pixels. Compose's `Offset`.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Point {
    /// Horizontal coordinate, in logical pixels, growing to the right.
    pub x: f32,
    /// Vertical coordinate, in logical pixels, growing downward.
    pub y: f32,
}

impl Point {
    /// Creates a point from its coordinates.
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// The origin point `(0.0, 0.0)`.
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    /// The distance from the origin to the point, in logical pixels, as Compose's
    /// `Offset.getDistance()`. The distance between two points is `(a - b).distance()`.
    #[inline]
    pub fn distance(self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }
}

impl Sub for Point {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self {
            x: self.x - other.x,
            y: self.y - other.y,
        }
    }
}
