//! Geometric primitives: Point, Size, Rect, Insets, Path

use crate::{Brush, ColorFilter, ImageBitmap};
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

#[derive(Clone, Debug, PartialEq)]
pub struct GraphicsLayer {
    pub alpha: f32,
    pub scale: f32,
    pub translation_x: f32,
    pub translation_y: f32,
    pub color_filter: Option<ColorFilter>,
    pub render_effect: Option<crate::render_effect::RenderEffect>,
    pub backdrop_effect: Option<crate::render_effect::RenderEffect>,
}

impl Default for GraphicsLayer {
    fn default() -> Self {
        Self {
            alpha: 1.0,
            scale: 1.0,
            translation_x: 0.0,
            translation_y: 0.0,
            color_filter: None,
            render_effect: None,
            backdrop_effect: None,
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
    Image {
        rect: Rect,
        image: ImageBitmap,
        alpha: f32,
        color_filter: Option<ColorFilter>,
        /// Optional source rectangle in image-pixel coordinates.
        /// When `None`, the entire image is drawn. When `Some`, only the
        /// specified sub-region of the source image is sampled.
        src_rect: Option<Rect>,
    },
}

pub trait DrawScope {
    fn size(&self) -> Size;
    fn draw_content(&self);
    fn draw_rect(&mut self, brush: Brush);
    /// Draws a rectangle at the specified position and size.
    fn draw_rect_at(&mut self, rect: Rect, brush: Brush);
    fn draw_round_rect(&mut self, brush: Brush, radii: CornerRadii);
    fn draw_image(&mut self, image: ImageBitmap);
    fn draw_image_at(
        &mut self,
        rect: Rect,
        image: ImageBitmap,
        alpha: f32,
        color_filter: Option<ColorFilter>,
    );
    /// Draws a sub-region of an image. `src_rect` is in image-pixel
    /// coordinates; `dst_rect` is in scope coordinates.
    fn draw_image_src(
        &mut self,
        image: ImageBitmap,
        src_rect: Rect,
        dst_rect: Rect,
        alpha: f32,
        color_filter: Option<ColorFilter>,
    );
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

    fn draw_image(&mut self, image: ImageBitmap) {
        self.primitives.push(DrawPrimitive::Image {
            rect: Rect::from_size(self.size),
            image,
            alpha: 1.0,
            color_filter: None,
            src_rect: None,
        });
    }

    fn draw_image_at(
        &mut self,
        rect: Rect,
        image: ImageBitmap,
        alpha: f32,
        color_filter: Option<ColorFilter>,
    ) {
        self.primitives.push(DrawPrimitive::Image {
            rect,
            image,
            alpha: alpha.clamp(0.0, 1.0),
            color_filter,
            src_rect: None,
        });
    }

    fn draw_image_src(
        &mut self,
        image: ImageBitmap,
        src_rect: Rect,
        dst_rect: Rect,
        alpha: f32,
        color_filter: Option<ColorFilter>,
    ) {
        self.primitives.push(DrawPrimitive::Image {
            rect: dst_rect,
            image,
            alpha: alpha.clamp(0.0, 1.0),
            color_filter,
            src_rect: Some(src_rect),
        });
    }

    fn into_primitives(self) -> Vec<DrawPrimitive> {
        self.primitives
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, ImageBitmap};

    #[test]
    fn draw_image_uses_scope_size_as_default_rect() {
        let mut scope = DrawScopeDefault::new(Size::new(40.0, 24.0));
        let image = ImageBitmap::from_rgba8(2, 2, vec![255; 16]).expect("image");
        scope.draw_image(image.clone());
        let primitives = scope.into_primitives();
        assert_eq!(primitives.len(), 1);
        match &primitives[0] {
            DrawPrimitive::Image {
                rect,
                image: actual,
                alpha,
                color_filter,
                src_rect,
            } => {
                assert_eq!(*rect, Rect::from_size(Size::new(40.0, 24.0)));
                assert_eq!(*actual, image);
                assert_eq!(*alpha, 1.0);
                assert!(color_filter.is_none());
                assert!(src_rect.is_none());
            }
            other => panic!("expected image primitive, got {other:?}"),
        }
    }

    #[test]
    fn draw_image_src_stores_src_rect() {
        let mut scope = DrawScopeDefault::new(Size::new(100.0, 100.0));
        let image = ImageBitmap::from_rgba8(64, 64, vec![255; 64 * 64 * 4]).expect("image");
        let src = Rect {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0,
        };
        let dst = Rect {
            x: 0.0,
            y: 0.0,
            width: 60.0,
            height: 80.0,
        };
        scope.draw_image_src(image.clone(), src, dst, 0.8, None);
        let primitives = scope.into_primitives();
        assert_eq!(primitives.len(), 1);
        match &primitives[0] {
            DrawPrimitive::Image {
                rect,
                image: actual,
                alpha,
                src_rect,
                ..
            } => {
                assert_eq!(*rect, dst);
                assert_eq!(*actual, image);
                assert!((alpha - 0.8).abs() < 1e-5);
                assert_eq!(*src_rect, Some(src));
            }
            other => panic!("expected image primitive, got {other:?}"),
        }
    }

    #[test]
    fn draw_image_at_clamps_alpha() {
        let mut scope = DrawScopeDefault::new(Size::new(10.0, 10.0));
        let image = ImageBitmap::from_rgba8(1, 1, vec![255, 255, 255, 255]).expect("image");
        scope.draw_image_at(
            Rect::from_origin_size(Point::new(2.0, 3.0), Size::new(5.0, 6.0)),
            image,
            3.0,
            Some(ColorFilter::Tint(Color::from_rgba_u8(128, 128, 255, 255))),
        );
        match &scope.into_primitives()[0] {
            DrawPrimitive::Image { alpha, .. } => assert_eq!(*alpha, 1.0),
            other => panic!("expected image primitive, got {other:?}"),
        }
    }

    #[test]
    fn graphics_layer_clone_with_render_effect() {
        use crate::RenderEffect;

        let layer = GraphicsLayer {
            render_effect: Some(RenderEffect::blur(10.0)),
            backdrop_effect: Some(RenderEffect::blur(6.0)),
            color_filter: Some(ColorFilter::tint(Color::from_rgba_u8(128, 200, 255, 255))),
            alpha: 0.5,
            ..Default::default()
        };
        let cloned = layer.clone();
        assert_eq!(cloned.alpha, 0.5);
        assert!(cloned.render_effect.is_some());
        assert!(cloned.backdrop_effect.is_some());
        assert_eq!(layer.color_filter, cloned.color_filter);
        assert_eq!(layer.render_effect, cloned.render_effect);
        assert_eq!(layer.backdrop_effect, cloned.backdrop_effect);
    }

    #[test]
    fn graphics_layer_default_has_no_effect() {
        let layer = GraphicsLayer::default();
        assert!(layer.color_filter.is_none());
        assert!(layer.render_effect.is_none());
        assert!(layer.backdrop_effect.is_none());
        assert_eq!(layer.alpha, 1.0);
    }
}
