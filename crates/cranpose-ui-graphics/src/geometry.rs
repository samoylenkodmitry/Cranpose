//! Geometric primitives: Point, Size, Rect, Insets, Path

use crate::{Brush, Color, ColorFilter, ImageBitmap};
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

    /// Returns the intersection of two rectangles, or `None` if they don't overlap.
    pub fn intersect(&self, other: Rect) -> Option<Rect> {
        let left = self.x.max(other.x);
        let top = self.y.max(other.y);
        let right = (self.x + self.width).min(other.x + other.width);
        let bottom = (self.y + self.height).min(other.y + other.height);
        let width = right - left;
        let height = bottom - top;
        if width <= 0.0 || height <= 0.0 {
            None
        } else {
            Some(Rect {
                x: left,
                y: top,
                width,
                height,
            })
        }
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
pub struct TransformOrigin {
    pub pivot_fraction_x: f32,
    pub pivot_fraction_y: f32,
}

impl TransformOrigin {
    pub const fn new(pivot_fraction_x: f32, pivot_fraction_y: f32) -> Self {
        Self {
            pivot_fraction_x,
            pivot_fraction_y,
        }
    }

    pub const CENTER: TransformOrigin = TransformOrigin::new(0.5, 0.5);
}

impl Default for TransformOrigin {
    fn default() -> Self {
        Self::CENTER
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum LayerShape {
    #[default]
    Rectangle,
    Rounded(RoundedCornerShape),
}

#[derive(Clone, Debug, PartialEq)]
pub struct GraphicsLayer {
    pub alpha: f32,
    pub scale: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub rotation_x: f32,
    pub rotation_y: f32,
    pub rotation_z: f32,
    pub camera_distance: f32,
    pub transform_origin: TransformOrigin,
    pub translation_x: f32,
    pub translation_y: f32,
    pub shadow_elevation: f32,
    pub ambient_shadow_color: Color,
    pub spot_shadow_color: Color,
    pub shape: LayerShape,
    pub clip: bool,
    pub compositing_strategy: CompositingStrategy,
    pub blend_mode: BlendMode,
    pub color_filter: Option<ColorFilter>,
    pub render_effect: Option<crate::render_effect::RenderEffect>,
    pub backdrop_effect: Option<crate::render_effect::RenderEffect>,
}

impl Default for GraphicsLayer {
    fn default() -> Self {
        Self {
            alpha: 1.0,
            scale: 1.0,
            scale_x: 1.0,
            scale_y: 1.0,
            rotation_x: 0.0,
            rotation_y: 0.0,
            rotation_z: 0.0,
            camera_distance: 8.0,
            transform_origin: TransformOrigin::CENTER,
            translation_x: 0.0,
            translation_y: 0.0,
            shadow_elevation: 0.0,
            ambient_shadow_color: Color::BLACK,
            spot_shadow_color: Color::BLACK,
            shape: LayerShape::Rectangle,
            clip: false,
            compositing_strategy: CompositingStrategy::Auto,
            blend_mode: BlendMode::SrcOver,
            color_filter: None,
            render_effect: None,
            backdrop_effect: None,
        }
    }
}

/// Blend mode used for draw primitives.
///
/// This mirrors Jetpack Compose's blend-mode vocabulary while the renderer
/// currently guarantees `SrcOver` and `DstOut` behavior.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BlendMode {
    Clear,
    Src,
    Dst,
    #[default]
    SrcOver,
    DstOver,
    SrcIn,
    DstIn,
    SrcOut,
    DstOut,
    SrcAtop,
    DstAtop,
    Xor,
    Plus,
    Modulate,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Multiply,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

/// Controls how a graphics layer is composited into its parent target.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CompositingStrategy {
    /// Use renderer heuristics (default).
    #[default]
    Auto,
    /// Render this layer to an offscreen target, then composite.
    Offscreen,
    /// Multiply alpha on source colors without allocating an offscreen layer.
    ModulateAlpha,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DrawPrimitive {
    /// Marker emitted by `draw_content()` inside `draw_with_content`.
    /// This is consumed by the modifier pipeline and never rendered directly.
    Content,
    /// Wrapper to associate a draw primitive with a non-default blend mode.
    Blend {
        primitive: Box<DrawPrimitive>,
        blend_mode: BlendMode,
    },
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
    /// Shadow that requires blur processing. The renderer decides technique
    /// (GPU blur, CPU approximation, etc.).
    Shadow(ShadowPrimitive),
}

/// Describes a shadow to be rendered. Each renderer chooses how to blur.
#[derive(Clone, Debug, PartialEq)]
pub enum ShadowPrimitive {
    /// Drop shadow: render shape behind content, blurred.
    Drop {
        shape: Box<DrawPrimitive>,
        blur_radius: f32,
        blend_mode: BlendMode,
    },
    /// Inner shadow: render fill + cutout to offscreen, blur, clip to bounds.
    Inner {
        fill: Box<DrawPrimitive>,
        cutout: Box<DrawPrimitive>,
        blur_radius: f32,
        blend_mode: BlendMode,
        /// Element bounds — blurred result must be clipped here.
        clip_rect: Rect,
    },
}

pub trait DrawScope {
    fn size(&self) -> Size;
    fn draw_content(&mut self);
    fn draw_rect(&mut self, brush: Brush);
    fn draw_rect_blend(&mut self, brush: Brush, blend_mode: BlendMode);
    /// Draws a rectangle at the specified position and size.
    fn draw_rect_at(&mut self, rect: Rect, brush: Brush);
    fn draw_rect_at_blend(&mut self, rect: Rect, brush: Brush, blend_mode: BlendMode);
    fn draw_round_rect(&mut self, brush: Brush, radii: CornerRadii);
    fn draw_round_rect_blend(&mut self, brush: Brush, radii: CornerRadii, blend_mode: BlendMode);
    fn draw_image(&mut self, image: ImageBitmap);
    fn draw_image_blend(&mut self, image: ImageBitmap, blend_mode: BlendMode);
    fn draw_image_at(
        &mut self,
        rect: Rect,
        image: ImageBitmap,
        alpha: f32,
        color_filter: Option<ColorFilter>,
    );
    fn draw_image_at_blend(
        &mut self,
        rect: Rect,
        image: ImageBitmap,
        alpha: f32,
        color_filter: Option<ColorFilter>,
        blend_mode: BlendMode,
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
    fn draw_image_src_blend(
        &mut self,
        image: ImageBitmap,
        src_rect: Rect,
        dst_rect: Rect,
        alpha: f32,
        color_filter: Option<ColorFilter>,
        blend_mode: BlendMode,
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

    fn push_blended_primitive(&mut self, primitive: DrawPrimitive, blend_mode: BlendMode) {
        if blend_mode == BlendMode::SrcOver {
            self.primitives.push(primitive);
        } else {
            self.primitives.push(DrawPrimitive::Blend {
                primitive: Box::new(primitive),
                blend_mode,
            });
        }
    }
}

impl DrawScope for DrawScopeDefault {
    fn size(&self) -> Size {
        self.size
    }

    fn draw_content(&mut self) {
        self.primitives.push(DrawPrimitive::Content);
    }

    fn draw_rect(&mut self, brush: Brush) {
        self.draw_rect_blend(brush, BlendMode::SrcOver);
    }

    fn draw_rect_blend(&mut self, brush: Brush, blend_mode: BlendMode) {
        self.push_blended_primitive(
            DrawPrimitive::Rect {
                rect: Rect::from_size(self.size),
                brush,
            },
            blend_mode,
        );
    }

    fn draw_rect_at(&mut self, rect: Rect, brush: Brush) {
        self.draw_rect_at_blend(rect, brush, BlendMode::SrcOver);
    }

    fn draw_rect_at_blend(&mut self, rect: Rect, brush: Brush, blend_mode: BlendMode) {
        self.push_blended_primitive(DrawPrimitive::Rect { rect, brush }, blend_mode);
    }

    fn draw_round_rect(&mut self, brush: Brush, radii: CornerRadii) {
        self.draw_round_rect_blend(brush, radii, BlendMode::SrcOver);
    }

    fn draw_round_rect_blend(&mut self, brush: Brush, radii: CornerRadii, blend_mode: BlendMode) {
        self.push_blended_primitive(
            DrawPrimitive::RoundRect {
                rect: Rect::from_size(self.size),
                brush,
                radii,
            },
            blend_mode,
        );
    }

    fn draw_image(&mut self, image: ImageBitmap) {
        self.draw_image_blend(image, BlendMode::SrcOver);
    }

    fn draw_image_blend(&mut self, image: ImageBitmap, blend_mode: BlendMode) {
        self.push_blended_primitive(
            DrawPrimitive::Image {
                rect: Rect::from_size(self.size),
                image,
                alpha: 1.0,
                color_filter: None,
                src_rect: None,
            },
            blend_mode,
        );
    }

    fn draw_image_at(
        &mut self,
        rect: Rect,
        image: ImageBitmap,
        alpha: f32,
        color_filter: Option<ColorFilter>,
    ) {
        self.draw_image_at_blend(rect, image, alpha, color_filter, BlendMode::SrcOver);
    }

    fn draw_image_at_blend(
        &mut self,
        rect: Rect,
        image: ImageBitmap,
        alpha: f32,
        color_filter: Option<ColorFilter>,
        blend_mode: BlendMode,
    ) {
        self.push_blended_primitive(
            DrawPrimitive::Image {
                rect,
                image,
                alpha: alpha.clamp(0.0, 1.0),
                color_filter,
                src_rect: None,
            },
            blend_mode,
        );
    }

    fn draw_image_src(
        &mut self,
        image: ImageBitmap,
        src_rect: Rect,
        dst_rect: Rect,
        alpha: f32,
        color_filter: Option<ColorFilter>,
    ) {
        self.draw_image_src_blend(
            image,
            src_rect,
            dst_rect,
            alpha,
            color_filter,
            BlendMode::SrcOver,
        );
    }

    fn draw_image_src_blend(
        &mut self,
        image: ImageBitmap,
        src_rect: Rect,
        dst_rect: Rect,
        alpha: f32,
        color_filter: Option<ColorFilter>,
        blend_mode: BlendMode,
    ) {
        self.push_blended_primitive(
            DrawPrimitive::Image {
                rect: dst_rect,
                image,
                alpha: alpha.clamp(0.0, 1.0),
                color_filter,
                src_rect: Some(src_rect),
            },
            blend_mode,
        );
    }

    fn into_primitives(self) -> Vec<DrawPrimitive> {
        self.primitives
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, ImageBitmap, RenderEffect};

    fn assert_image_alpha(primitive: &DrawPrimitive, expected: f32) {
        match primitive {
            DrawPrimitive::Image { alpha, .. } => assert!((alpha - expected).abs() < 1e-5),
            DrawPrimitive::Blend { primitive, .. } => assert_image_alpha(primitive, expected),
            other => panic!("expected image primitive, got {other:?}"),
        }
    }

    fn unwrap_image(primitive: &DrawPrimitive) -> &DrawPrimitive {
        match primitive {
            DrawPrimitive::Image { .. } => primitive,
            DrawPrimitive::Blend { primitive, .. } => unwrap_image(primitive),
            other => panic!("expected image primitive, got {other:?}"),
        }
    }

    #[test]
    fn draw_content_inserts_content_marker() {
        let mut scope = DrawScopeDefault::new(Size::new(8.0, 8.0));
        scope.draw_rect(Brush::solid(Color::WHITE));
        scope.draw_content();
        scope.draw_rect_blend(Brush::solid(Color::BLACK), BlendMode::DstOut);

        let primitives = scope.into_primitives();
        assert!(matches!(primitives[1], DrawPrimitive::Content));
        assert!(matches!(
            primitives[2],
            DrawPrimitive::Blend {
                blend_mode: BlendMode::DstOut,
                ..
            }
        ));
    }

    #[test]
    fn draw_rect_blend_wraps_non_default_modes() {
        let mut scope = DrawScopeDefault::new(Size::new(10.0, 10.0));
        scope.draw_rect_blend(Brush::solid(Color::RED), BlendMode::DstOut);

        let primitives = scope.into_primitives();
        assert_eq!(primitives.len(), 1);
        match &primitives[0] {
            DrawPrimitive::Blend {
                primitive,
                blend_mode,
            } => {
                assert_eq!(*blend_mode, BlendMode::DstOut);
                assert!(matches!(**primitive, DrawPrimitive::Rect { .. }));
            }
            other => panic!("expected blended primitive, got {other:?}"),
        }
    }

    #[test]
    fn draw_image_uses_scope_size_as_default_rect() {
        let mut scope = DrawScopeDefault::new(Size::new(40.0, 24.0));
        let image = ImageBitmap::from_rgba8(2, 2, vec![255; 16]).expect("image");
        scope.draw_image(image.clone());
        let primitives = scope.into_primitives();
        assert_eq!(primitives.len(), 1);
        match unwrap_image(&primitives[0]) {
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
        match unwrap_image(&primitives[0]) {
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
        assert_image_alpha(&scope.into_primitives()[0], 1.0);
    }

    #[test]
    fn graphics_layer_clone_with_render_effect() {
        let layer = GraphicsLayer {
            render_effect: Some(RenderEffect::blur(10.0)),
            backdrop_effect: Some(RenderEffect::blur(6.0)),
            color_filter: Some(ColorFilter::tint(Color::from_rgba_u8(128, 200, 255, 255))),
            alpha: 0.5,
            rotation_z: 12.0,
            shadow_elevation: 4.0,
            shape: LayerShape::Rounded(RoundedCornerShape::uniform(6.0)),
            clip: true,
            compositing_strategy: CompositingStrategy::Offscreen,
            blend_mode: BlendMode::SrcOver,
            ..Default::default()
        };
        let cloned = layer.clone();
        assert_eq!(cloned.alpha, 0.5);
        assert!(cloned.render_effect.is_some());
        assert!(cloned.backdrop_effect.is_some());
        assert_eq!(layer.color_filter, cloned.color_filter);
        assert_eq!(layer.render_effect, cloned.render_effect);
        assert_eq!(layer.backdrop_effect, cloned.backdrop_effect);
        assert!((cloned.rotation_z - 12.0).abs() < 1e-6);
        assert!((cloned.shadow_elevation - 4.0).abs() < 1e-6);
        assert_eq!(
            cloned.shape,
            LayerShape::Rounded(RoundedCornerShape::uniform(6.0))
        );
        assert!(cloned.clip);
        assert_eq!(cloned.compositing_strategy, CompositingStrategy::Offscreen);
        assert_eq!(cloned.blend_mode, BlendMode::SrcOver);
    }

    #[test]
    fn graphics_layer_default_has_no_effect() {
        let layer = GraphicsLayer::default();
        assert!(layer.color_filter.is_none());
        assert!(layer.render_effect.is_none());
        assert!(layer.backdrop_effect.is_none());
        assert_eq!(layer.compositing_strategy, CompositingStrategy::Auto);
        assert_eq!(layer.blend_mode, BlendMode::SrcOver);
        assert_eq!(layer.alpha, 1.0);
        assert_eq!(layer.transform_origin, TransformOrigin::CENTER);
        assert!((layer.camera_distance - 8.0).abs() < 1e-6);
        assert_eq!(layer.shape, LayerShape::Rectangle);
        assert!(!layer.clip);
        assert_eq!(layer.ambient_shadow_color, Color::BLACK);
        assert_eq!(layer.spot_shadow_color, Color::BLACK);
    }

    #[test]
    fn transform_origin_construction() {
        let origin = TransformOrigin::new(0.25, 0.75);
        assert!((origin.pivot_fraction_x - 0.25).abs() < 1e-6);
        assert!((origin.pivot_fraction_y - 0.75).abs() < 1e-6);
    }

    #[test]
    fn layer_shape_default_is_rectangle() {
        assert_eq!(LayerShape::default(), LayerShape::Rectangle);
    }
}
