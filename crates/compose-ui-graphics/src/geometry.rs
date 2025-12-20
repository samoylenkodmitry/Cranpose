//! Geometric primitives: Point, Size, Rect, Insets, Path

use crate::Brush;
use std::ops::AddAssign;

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub const ZERO: Point = Point { x: 0.0, y: 0.0 };
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }

    pub const ZERO: Size = Size {
        width: 0.0,
        height: 0.0,
    };
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn from_origin_size(origin: Point, size: Size) -> Self {
        Self {
            x: origin.x,
            y: origin.y,
            width: size.width,
            height: size.height,
        }
    }

    pub fn from_size(size: Size) -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: size.width,
            height: size.height,
        }
    }

    pub fn translate(&self, dx: f32, dy: f32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
            width: self.width,
            height: self.height,
        }
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x <= self.x + self.width && y <= self.y + self.height
    }
}

/// Padding values for each edge of a rectangle.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EdgeInsets {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl EdgeInsets {
    pub fn uniform(all: f32) -> Self {
        Self {
            left: all,
            top: all,
            right: all,
            bottom: all,
        }
    }

    pub fn horizontal(horizontal: f32) -> Self {
        Self {
            left: horizontal,
            right: horizontal,
            ..Self::default()
        }
    }

    pub fn vertical(vertical: f32) -> Self {
        Self {
            top: vertical,
            bottom: vertical,
            ..Self::default()
        }
    }

    pub fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self {
            left: horizontal,
            right: horizontal,
            top: vertical,
            bottom: vertical,
        }
    }

    pub fn from_components(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    pub fn is_zero(&self) -> bool {
        self.left == 0.0 && self.top == 0.0 && self.right == 0.0 && self.bottom == 0.0
    }

    pub fn horizontal_sum(&self) -> f32 {
        self.left + self.right
    }

    pub fn vertical_sum(&self) -> f32 {
        self.top + self.bottom
    }
}

impl AddAssign for EdgeInsets {
    fn add_assign(&mut self, rhs: Self) {
        self.left += rhs.left;
        self.top += rhs.top;
        self.right += rhs.right;
        self.bottom += rhs.bottom;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CornerRadii {
    pub top_left: f32,
    pub top_right: f32,
    pub bottom_right: f32,
    pub bottom_left: f32,
}

impl CornerRadii {
    pub fn uniform(radius: f32) -> Self {
        Self {
            top_left: radius,
            top_right: radius,
            bottom_right: radius,
            bottom_left: radius,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoundedCornerShape {
    radii: CornerRadii,
}

impl RoundedCornerShape {
    pub fn new(top_left: f32, top_right: f32, bottom_right: f32, bottom_left: f32) -> Self {
        Self {
            radii: CornerRadii {
                top_left,
                top_right,
                bottom_right,
                bottom_left,
            },
        }
    }

    pub fn uniform(radius: f32) -> Self {
        Self {
            radii: CornerRadii::uniform(radius),
        }
    }

    pub fn with_radii(radii: CornerRadii) -> Self {
        Self { radii }
    }

    pub fn resolve(&self, width: f32, height: f32) -> CornerRadii {
        let mut resolved = self.radii;
        let max_width = (width / 2.0).max(0.0);
        let max_height = (height / 2.0).max(0.0);
        resolved.top_left = resolved.top_left.clamp(0.0, max_width).min(max_height);
        resolved.top_right = resolved.top_right.clamp(0.0, max_width).min(max_height);
        resolved.bottom_right = resolved.bottom_right.clamp(0.0, max_width).min(max_height);
        resolved.bottom_left = resolved.bottom_left.clamp(0.0, max_width).min(max_height);
        resolved
    }

    pub fn radii(&self) -> CornerRadii {
        self.radii
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GraphicsLayer {
    pub alpha: f32,
    pub scale: f32,
    pub translation_x: f32,
    pub translation_y: f32,
}

impl Default for GraphicsLayer {
    fn default() -> Self {
        Self {
            alpha: 1.0,
            scale: 1.0,
            translation_x: 0.0,
            translation_y: 0.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum DrawPrimitive {
    Rect {
        rect: Rect,
        brush: Brush,
    },
    RoundRect {
        rect: Rect,
        brush: Brush,
        radii: CornerRadii,
    },
}

pub trait DrawScope {
    fn size(&self) -> Size;
    fn draw_content(&self);
    fn draw_rect(&mut self, brush: Brush);
    /// Draws a rectangle at the specified position and size.
    fn draw_rect_at(&mut self, rect: Rect, brush: Brush);
    fn draw_round_rect(&mut self, brush: Brush, radii: CornerRadii);
    fn into_primitives(self) -> Vec<DrawPrimitive>;
}

#[derive(Default)]
pub struct DrawScopeDefault {
    size: Size,
    primitives: Vec<DrawPrimitive>,
}

impl DrawScopeDefault {
    pub fn new(size: Size) -> Self {
        Self {
            size,
            primitives: Vec::new(),
        }
    }
}

impl DrawScope for DrawScopeDefault {
    fn size(&self) -> Size {
        self.size
    }

    fn draw_content(&self) {}

    fn draw_rect(&mut self, brush: Brush) {
        self.primitives.push(DrawPrimitive::Rect {
            rect: Rect::from_size(self.size),
            brush,
        });
    }

    fn draw_rect_at(&mut self, rect: Rect, brush: Brush) {
        self.primitives.push(DrawPrimitive::Rect { rect, brush });
    }

    fn draw_round_rect(&mut self, brush: Brush, radii: CornerRadii) {
        self.primitives.push(DrawPrimitive::RoundRect {
            rect: Rect::from_size(self.size),
            brush,
            radii,
        });
    }

    fn into_primitives(self) -> Vec<DrawPrimitive> {
        self.primitives
    }
}
