//! Geometric primitives: Point, Size, Rect, Insets, Path

use std::{ops::AddAssign, rc::Rc};

use crate::{
    ArcRecordArgs, Brush, Color, ColorFilter, CommandRecorder, CommandRecording, DrawGroup,
    DrawTransform, GroupMarker, ImageBitmap, ImageSampling, PathFillRule, Trapezoid,
    normalized_band,
    path::{DrawStyle, Path},
    stroke::{LineGeometry, Stroke},
    typography::{
        DrawTextMeasurer, DrawTextStyle, TextAlign, TextMeasurement, TextVerticalAlign,
        estimate_text_measurement,
    },
};

const VECTOR_PATH_MASK_CACHE_ENTRIES: usize = 96;
const VECTOR_PATH_MASK_CACHE_BYTES: usize = 8 * 1024 * 1024;

pub(crate) struct VectorPathMaskCache {
    entries: Vec<(u64, ImageBitmap)>,
    bytes: usize,
}

impl VectorPathMaskCache {
    pub(crate) const fn new() -> Self {
        Self {
            entries: Vec::new(),
            bytes: 0,
        }
    }

    fn get(&mut self, key: u64) -> Option<ImageBitmap> {
        let index = self.entries.iter().position(|(seen, _)| *seen == key)?;
        let entry = self.entries.remove(index);
        let image = entry.1.clone();
        self.entries.push(entry);
        Some(image)
    }

    fn put(&mut self, key: u64, image: ImageBitmap) {
        let bytes = image.pixels().len();
        if bytes > VECTOR_PATH_MASK_CACHE_BYTES {
            return;
        }
        self.bytes += bytes;
        self.entries.push((key, image));
        while self.entries.len() > VECTOR_PATH_MASK_CACHE_ENTRIES
            || self.bytes > VECTOR_PATH_MASK_CACHE_BYTES
        {
            let (_, dropped) = self.entries.remove(0);
            self.bytes = self.bytes.saturating_sub(dropped.pixels().len());
        }
    }
}

/// The key of `path`'s raster: a gradient's colors depend on the rect it
/// resolves against, a solid color's do not.
/// Which side of a path's edge a coverage mask covers: its fill, or the
/// rest of the mask's rect, as a clip's mask erases it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PathSide {
    Inside,
    Outside,
}

fn vector_path_mask_key(
    path: &crate::VectorPath,
    origin: Point,
    mask_size: (usize, usize),
    brush: &Brush,
    brush_rect: Rect,
    side: PathSide,
) -> u64 {
    use std::hash::Hasher;
    let mut hasher = crate::fx_hash::FxHasher::default();
    hasher.write_u8(path.fill_rule() as u8);
    hasher.write_u8(side as u8);
    hasher.write_u32(origin.x.to_bits());
    hasher.write_u32(origin.y.to_bits());
    hasher.write_usize(mask_size.0);
    hasher.write_usize(mask_size.1);
    crate::render_hash::hash_brush(brush, &mut hasher);
    if !matches!(brush, Brush::Solid(_)) {
        for value in [
            brush_rect.x,
            brush_rect.y,
            brush_rect.width,
            brush_rect.height,
        ] {
            hasher.write_u32(value.to_bits());
        }
    }
    for subpath in path.subpaths() {
        hasher.write_usize(subpath.len());
        for point in subpath {
            hasher.write_u32(point.x.to_bits());
            hasher.write_u32(point.y.to_bits());
        }
    }
    hasher.finish()
}

/// How many raster pixels a vector path's fill gets per logical pixel along
/// each axis.
const VECTOR_PATH_SUPERSAMPLE: f32 = 2.0;

/// `path`'s fill, or with [`PathSide::Outside`] the rest of the mask, over
/// `size` raster pixels from `origin`: one alpha byte of a solid color per
/// pixel, or for a gradient the color it paints at each pixel's center with
/// the pixel's coverage.
fn rasterize_vector_path(
    path: &crate::VectorPath,
    brush: &Brush,
    brush_rect: Rect,
    origin: Point,
    (width, height): (usize, usize),
    side: PathSide,
) -> Option<ImageBitmap> {
    let byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    let covered = |opacity: f32| {
        let mut mask = path.coverage_mask(width, height, origin, VECTOR_PATH_SUPERSAMPLE, 1.0);
        for value in &mut mask {
            let share = match side {
                PathSide::Inside => *value,
                PathSide::Outside => u8::MAX - *value,
            };
            *value = (f32::from(share) * opacity + 0.5) as u8;
        }
        mask
    };
    if let Brush::Solid(color) = brush {
        let mask = match side {
            PathSide::Inside => path.coverage_mask(
                width,
                height,
                origin,
                VECTOR_PATH_SUPERSAMPLE,
                color.3.clamp(0.0, 1.0),
            ),
            PathSide::Outside => covered(color.3.clamp(0.0, 1.0)),
        };
        let rgb = [byte(color.0), byte(color.1), byte(color.2)];
        return ImageBitmap::from_alpha8(width as u32, height as u32, rgb, mask).ok();
    }
    let coverage = covered(1.0);
    let mut pixels = Vec::with_capacity(coverage.len() * 4);
    for (row, line) in coverage.chunks_exact(width).enumerate() {
        let y = origin.y + (row as f32 + 0.5) / VECTOR_PATH_SUPERSAMPLE;
        for (column, &covered) in line.iter().enumerate() {
            if covered == 0 {
                pixels.extend_from_slice(&[0; 4]);
                continue;
            }
            let x = origin.x + (column as f32 + 0.5) / VECTOR_PATH_SUPERSAMPLE;
            let color = brush.color_at(brush_rect, Point::new(x, y));
            let alpha = (color.3.clamp(0.0, 1.0) * f32::from(covered) + 0.5) as u8;
            pixels.extend_from_slice(&[byte(color.0), byte(color.1), byte(color.2), alpha]);
        }
    }
    ImageBitmap::from_rgba8(width as u32, height as u32, pixels).ok()
}

fn vector_path_mask_cache_get(key: u64) -> Option<ImageBitmap> {
    crate::thread_state::with(|state| state.vector_path_masks.borrow_mut().get(key))
}

fn vector_path_mask_cache_put(key: u64, image: ImageBitmap) {
    crate::thread_state::with(|state| state.vector_path_masks.borrow_mut().put(key, image));
}

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
    pub const ZERO: Point = Point { x: 0.0, y: 0.0 };
}

/// A width and a height, in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Size {
    /// Width, in logical pixels.
    pub width: f32,
    /// Height, in logical pixels.
    pub height: f32,
}

impl Size {
    /// Creates a size from a width and a height.
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }

    /// A size of zero width and zero height.
    pub const ZERO: Size = Size {
        width: 0.0,
        height: 0.0,
    };
}

/// An axis-aligned rectangle given by its top-left corner and its size, in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    /// Left edge, in logical pixels.
    pub x: f32,
    /// Top edge, in logical pixels.
    pub y: f32,
    /// Width, in logical pixels.
    pub width: f32,
    /// Height, in logical pixels.
    pub height: f32,
}

impl Rect {
    /// The rect that holds nothing: what two clips that do not overlap
    /// resolve to, so a clip that meets nothing stays a clip instead of
    /// lifting.
    pub const EMPTY: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    };

    /// Creates a rectangle with its top-left corner at `origin` and the given `size`.
    pub fn from_origin_size(origin: Point, size: Size) -> Self {
        Self {
            x: origin.x,
            y: origin.y,
            width: size.width,
            height: size.height,
        }
    }

    /// Creates a rectangle of `size` with its top-left corner at the origin.
    pub fn from_size(size: Size) -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: size.width,
            height: size.height,
        }
    }

    /// Returns the rectangle moved by `dx` horizontally and `dy` vertically.
    pub fn translate(&self, dx: f32, dy: f32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
            width: self.width,
            height: self.height,
        }
    }

    /// Whether the point `(x, y)` lies inside the rectangle, edges included.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x <= self.x + self.width && y <= self.y + self.height
    }

    /// Whether the rect covers no area, so nothing clipped to it can paint.
    pub fn is_empty(&self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
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

    /// Returns the smallest rectangle that contains both rectangles.
    pub fn union(&self, other: Rect) -> Rect {
        let left = self.x.min(other.x);
        let top = self.y.min(other.y);
        let right = (self.x + self.width).max(other.x + other.width);
        let bottom = (self.y + self.height).max(other.y + other.height);
        Rect {
            x: left,
            y: top,
            width: (right - left).max(0.0),
            height: (bottom - top).max(0.0),
        }
    }
}

/// Padding values for each edge of a rectangle.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EdgeInsets {
    /// Inset on the left edge, in logical pixels.
    pub left: f32,
    /// Inset on the top edge, in logical pixels.
    pub top: f32,
    /// Inset on the right edge, in logical pixels.
    pub right: f32,
    /// Inset on the bottom edge, in logical pixels.
    pub bottom: f32,
}

impl EdgeInsets {
    /// Creates insets with `all` on every edge.
    pub fn uniform(all: f32) -> Self {
        Self {
            left: all,
            top: all,
            right: all,
            bottom: all,
        }
    }

    /// Creates insets with `horizontal` on the left and right and 0.0 on the top and bottom.
    pub fn horizontal(horizontal: f32) -> Self {
        Self {
            left: horizontal,
            right: horizontal,
            ..Self::default()
        }
    }

    /// Creates insets with `vertical` on the top and bottom and 0.0 on the left and right.
    pub fn vertical(vertical: f32) -> Self {
        Self {
            top: vertical,
            bottom: vertical,
            ..Self::default()
        }
    }

    /// Creates insets with `horizontal` on the left and right and `vertical` on the top and bottom.
    pub fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self {
            left: horizontal,
            right: horizontal,
            top: vertical,
            bottom: vertical,
        }
    }

    /// Creates insets from separate left, top, right and bottom values.
    pub fn from_components(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    /// Whether all four insets are 0.0.
    pub fn is_zero(&self) -> bool {
        self.left == 0.0 && self.top == 0.0 && self.right == 0.0 && self.bottom == 0.0
    }

    /// The sum of the left and right insets.
    pub fn horizontal_sum(&self) -> f32 {
        self.left + self.right
    }

    /// The sum of the top and bottom insets.
    pub fn vertical_sum(&self) -> f32 {
        self.top + self.bottom
    }

    /// The part of a `size` rect these insets leave, at the leading insets;
    /// it never shrinks below zero.
    pub fn inset_rect(&self, size: Size) -> Rect {
        Rect {
            x: self.left,
            y: self.top,
            width: (size.width - self.horizontal_sum()).max(0.0),
            height: (size.height - self.vertical_sum()).max(0.0),
        }
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

/// The radius of each corner of a rounded rectangle, in logical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CornerRadii {
    /// Radius of the top-left corner, in logical pixels.
    pub top_left: f32,
    /// Radius of the top-right corner, in logical pixels.
    pub top_right: f32,
    /// Radius of the bottom-right corner, in logical pixels.
    pub bottom_right: f32,
    /// Radius of the bottom-left corner, in logical pixels.
    pub bottom_left: f32,
}

impl CornerRadii {
    /// Creates radii with the same value on all four corners.
    pub fn uniform(radius: f32) -> Self {
        Self {
            top_left: radius,
            top_right: radius,
            bottom_right: radius,
            bottom_left: radius,
        }
    }
}

/// A rectangle shape with rounded corners. Compose's `RoundedCornerShape`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoundedCornerShape {
    radii: CornerRadii,
}

impl RoundedCornerShape {
    /// Creates a shape with a separate radius for each corner, in logical pixels.
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

    /// Creates a shape with the same radius on all four corners.
    pub fn uniform(radius: f32) -> Self {
        Self {
            radii: CornerRadii::uniform(radius),
        }
    }

    /// Creates a shape from a `CornerRadii` value.
    pub fn with_radii(radii: CornerRadii) -> Self {
        Self { radii }
    }

    /// Returns the radii for a rectangle of `width` by `height`, with each radius limited to half
    /// the width and half the height so the corners fit.
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

    /// The radii as given, before any clamping to a size.
    pub fn radii(&self) -> CornerRadii {
        self.radii
    }
}

/// The pivot point for scaling and rotating a layer, given as fractions of its size. Compose's
/// `TransformOrigin`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransformOrigin {
    /// Horizontal pivot as a fraction of the layer's width, from 0.0 (left) to 1.0 (right).
    pub pivot_fraction_x: f32,
    /// Vertical pivot as a fraction of the layer's height, from 0.0 (top) to 1.0 (bottom).
    pub pivot_fraction_y: f32,
}

impl TransformOrigin {
    /// Creates an origin from fractions of the layer's width and height.
    pub const fn new(pivot_fraction_x: f32, pivot_fraction_y: f32) -> Self {
        Self {
            pivot_fraction_x,
            pivot_fraction_y,
        }
    }

    /// Pivot at the center of the layer.
    pub const CENTER: TransformOrigin = TransformOrigin::new(0.5, 0.5);
}

impl Default for TransformOrigin {
    fn default() -> Self {
        Self::CENTER
    }
}

/// The outline of a graphics layer, used for clipping and for its shadow.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum LayerShape {
    /// Square corners.
    #[default]
    Rectangle,
    /// Corners rounded by the given shape.
    Rounded(RoundedCornerShape),
}

/// The transform, opacity, clip, shadow and effect properties of a layer. Compose's
/// `GraphicsLayerScope`.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphicsLayer {
    /// Opacity of the layer, from 0.0 (transparent) to 1.0 (opaque).
    pub alpha: f32,
    /// Uniform scale factor for both axes. 1.0 leaves the size unchanged.
    pub scale: f32,
    /// Horizontal scale factor, multiplied with `scale`. 1.0 leaves the width unchanged.
    pub scale_x: f32,
    /// Vertical scale factor, multiplied with `scale`. 1.0 leaves the height unchanged.
    pub scale_y: f32,
    /// Rotation around the x axis, in degrees.
    pub rotation_x: f32,
    /// Rotation around the y axis, in degrees.
    pub rotation_y: f32,
    /// Rotation around the z axis, in degrees. Positive values turn clockwise on screen.
    pub rotation_z: f32,
    /// Distance of the camera from the layer plane. It sets the strength of the perspective for
    /// `rotation_x` and `rotation_y`; larger values flatten it.
    pub camera_distance: f32,
    /// The point that scaling and rotation turn around, as fractions of the layer's size.
    pub transform_origin: TransformOrigin,
    /// Horizontal translation, in layout coordinates.
    pub translation_x: f32,
    /// Vertical translation, in layout coordinates.
    pub translation_y: f32,
    /// Shadow elevation in layout units. A value of 0.0 or less draws no shadow.
    pub shadow_elevation: f32,
    /// Color of the ambient shadow.
    pub ambient_shadow_color: Color,
    /// Color of the spot shadow.
    pub spot_shadow_color: Color,
    /// Outline used for clipping and for the shadow.
    pub shape: LayerShape,
    /// Whether content is clipped to `shape`.
    pub clip: bool,
    /// Whether the layer renders offscreen before it is composited.
    pub compositing_strategy: CompositingStrategy,
    /// How the layer's content combines with what is already drawn behind it.
    pub blend_mode: BlendMode,
    /// Color filter applied to the layer's content, or `None` for none.
    pub color_filter: Option<ColorFilter>,
    /// Filter applied to the layer's rendered content as a whole, such as a blur. `None` applies
    /// none.
    pub render_effect: Option<crate::render_effect::RenderEffect>,
    /// Filters the pixels behind this layer. The result shares the layer's
    /// group opacity and blend mode with its content; `ModulateAlpha` applies
    /// opacity independently to the backdrop and each content draw.
    pub backdrop_effect: Option<crate::render_effect::RenderEffect>,
}

impl GraphicsLayer {
    /// Default properties for an untransformed, opaque layer without effects.
    pub const DEFAULT: Self = Self {
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
    };

    /// The alpha an isolated layer is composited at: an **eight-bit** one,
    /// truncated.
    ///
    /// The platform never composites a layer at a float alpha. HWUI hands an
    /// isolated `RenderNode` to the rasterizer as
    /// `canvas->saveLayerAlpha(&bounds, (int)(properties.getAlpha() * 255))`
    /// (`frameworks/base/libs/hwui/pipeline/skia/RenderNodeDrawable.cpp`,
    /// `setViewProperties`), and `(int)` truncates — 0.5 composites at 127/255,
    /// not at 128/255. The fraction below that byte is gone before a single pixel
    /// is blended, so anything that keeps it lands a level out wherever the byte
    /// and the float fall on opposite sides of a half.
    ///
    /// The sibling branch is a float on purpose: where `getHasOverlappingRendering()`
    /// is false HWUI takes `*alphaMultiplier = properties.getAlpha()` and folds it
    /// into each draw without ever making a byte of it. That is what
    /// `CompositingStrategy::ModulateAlpha` names.
    ///
    /// Truncating here and **rounding** in [`Color::srgb_8bit`] is not an
    /// inconsistency: they are different call sites in the platform. A colour's own
    /// alpha is snapped by `Color`'s constructor, which adds the half; a layer's
    /// alpha is snapped by HWUI's cast, which does not. Anything modelling a faded
    /// layer without allocating one — a canvas drawing a list row's fade by hand,
    /// say — wants this rule and not the other.
    pub fn composite_alpha_8bit(alpha: f32) -> f32 {
        (alpha.clamp(0.0, 1.0) * 255.0).floor() / 255.0
    }
}

impl Default for GraphicsLayer {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Blend mode used for draw primitives.
///
/// This mirrors Jetpack Compose's blend-mode vocabulary while the renderer
/// currently guarantees `SrcOver` and `DstOut` behavior.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BlendMode {
    /// Clears the destination to transparent where the source draws.
    Clear,
    /// Replaces the destination with the source.
    Src,
    /// Keeps the destination and discards the source.
    Dst,
    /// Draws the source over the destination. This is the default.
    #[default]
    SrcOver,
    /// Draws the destination over the source.
    DstOver,
    /// Keeps the source only where the destination is opaque.
    SrcIn,
    /// Keeps the destination only where the source is opaque.
    DstIn,
    /// Keeps the source only where the destination is transparent.
    SrcOut,
    /// Keeps the destination only where the source is transparent.
    DstOut,
    /// Draws the source over the destination, only where the destination is opaque.
    SrcAtop,
    /// Draws the destination over the source, only where the source is opaque.
    DstAtop,
    /// Keeps the source and the destination where the other is transparent.
    Xor,
    /// Adds the source and destination colors and alphas.
    Plus,
    /// Multiplies the source and destination colors and alphas.
    Modulate,
    /// Inverts, multiplies and inverts again, which lightens the result.
    Screen,
    /// Multiplies or screens depending on the destination color, which raises contrast.
    Overlay,
    /// Keeps the darker color of each channel.
    Darken,
    /// Keeps the lighter color of each channel.
    Lighten,
    /// Brightens the destination by the source color.
    ColorDodge,
    /// Darkens the destination by the source color.
    ColorBurn,
    /// Multiplies or screens depending on the source color.
    HardLight,
    /// A softer version of `HardLight`: darkens or lightens depending on the source color.
    SoftLight,
    /// Subtracts the darker color from the lighter one in each channel.
    Difference,
    /// Like `Difference` with lower contrast.
    Exclusion,
    /// Multiplies the source and destination colors, which darkens the result.
    Multiply,
    /// Takes the hue of the source with the saturation and luminosity of the destination.
    Hue,
    /// Takes the saturation of the source with the hue and luminosity of the destination.
    Saturation,
    /// Takes the hue and saturation of the source with the luminosity of the destination.
    Color,
    /// Takes the luminosity of the source with the hue and saturation of the destination.
    Luminosity,
}

impl BlendMode {
    /// Every mode in declaration order, so `mode as u32` indexes it.
    pub const ALL: [BlendMode; 29] = [
        BlendMode::Clear,
        BlendMode::Src,
        BlendMode::Dst,
        BlendMode::SrcOver,
        BlendMode::DstOver,
        BlendMode::SrcIn,
        BlendMode::DstIn,
        BlendMode::SrcOut,
        BlendMode::DstOut,
        BlendMode::SrcAtop,
        BlendMode::DstAtop,
        BlendMode::Xor,
        BlendMode::Plus,
        BlendMode::Modulate,
        BlendMode::Screen,
        BlendMode::Overlay,
        BlendMode::Darken,
        BlendMode::Lighten,
        BlendMode::ColorDodge,
        BlendMode::ColorBurn,
        BlendMode::HardLight,
        BlendMode::SoftLight,
        BlendMode::Difference,
        BlendMode::Exclusion,
        BlendMode::Multiply,
        BlendMode::Hue,
        BlendMode::Saturation,
        BlendMode::Color,
        BlendMode::Luminosity,
    ];
}

/// How a [`GraphicsLayer`] is composited into its parent. Compose's `CompositingStrategy`.
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

/// One recorded draw operation, in scope coordinates. Renderers consume these in draw order.
#[derive(Clone, Debug, PartialEq)]
pub enum DrawPrimitive {
    /// Marker emitted by `draw_content()` inside `draw_with_content`.
    /// This is consumed by the modifier pipeline and never rendered directly.
    Content,
    /// Wrapper to associate a draw primitive with a non-default blend mode.
    Blend {
        /// The primitive drawn with `blend_mode`.
        primitive: Box<DrawPrimitive>,
        /// How the wrapped primitive combines with what is already drawn.
        blend_mode: BlendMode,
    },
    /// A rectangle, filled or stroked.
    Rect {
        /// The rectangle in scope coordinates.
        rect: Rect,
        /// Paint for the fill or the stroke.
        brush: Brush,
        /// `None` fills the rect; `Some` strokes its outline, centered on the
        /// edge (so it bleeds `width / 2` outside `rect`).
        stroke: Option<Stroke>,
    },
    /// A rectangle with rounded corners, filled or stroked.
    RoundRect {
        /// The rectangle in scope coordinates.
        rect: Rect,
        /// Paint for the fill or the stroke.
        brush: Brush,
        /// Corner radii of the rectangle, in logical pixels.
        radii: CornerRadii,
        /// `None` fills the rounded rect; `Some` strokes its outline, centered
        /// on the edge.
        stroke: Option<Stroke>,
    },
    /// A circular band: a stroked arc, or a filled annular sector / pie wedge.
    ///
    /// Angles are radians, `0` = +X, increasing **clockwise** on screen (see
    /// `crate::stroke` for the full convention).
    ///
    /// * `stroke = Some(_)` — the band is `radius ± width/2`, its ends shaped
    ///   by the stroke cap. `inner_radius` is ignored.
    /// * `stroke = None` — the band is `inner_radius ..= radius` with flat
    ///   (butt) radial ends; `inner_radius = 0` is a filled pie wedge.
    Arc {
        /// Tight bounding box of the rendered band, caps included. Kept as the
        /// first field (like every other variant) so bbox/culling/clip logic
        /// treats an arc exactly like any other primitive.
        rect: Rect,
        /// Paint for the arc.
        brush: Brush,
        /// Center of the circle the arc lies on, in scope coordinates.
        center: Point,
        /// Radius of the stroked centerline, or the outer radius of a filled sector, in logical
        /// pixels.
        radius: f32,
        /// Where the arc starts, in radians. `0` points along +X and angles increase clockwise on
        /// screen.
        start_angle: f32,
        /// Angle swept from `start_angle`, in radians. A negative value sweeps counter-clockwise.
        sweep_angle: f32,
        /// `Some` strokes the arc; `None` fills the sector between `inner_radius` and `radius`.
        stroke: Option<Stroke>,
        /// `> 0` turns a filled wedge into an annular sector.
        inner_radius: f32,
    },
    /// A stroked straight segment from `start` to `end`, its ends shaped by
    /// the stroke's cap: Compose's `drawLine`.
    Line {
        /// The box of the two ends; the stroke reaches half its width past
        /// it, as a stroked rect's does past its rect.
        rect: Rect,
        /// Paint for the line.
        brush: Brush,
        /// Start of the segment, in scope coordinates.
        start: Point,
        /// End of the segment, in scope coordinates.
        end: Point,
        /// Width and cap of the line.
        stroke: Stroke,
    },
    /// One slice of a filled path, which the draw scope cuts a fill into
    /// so the GPU draws it: see [`Trapezoid`].
    Trapezoid {
        /// The rect `brush` resolves against: the draw scope's own, as a
        /// path's brush resolves in Compose.
        rect: Rect,
        /// Paint for the slice.
        brush: Brush,
        /// The slice to fill.
        trapezoid: Trapezoid,
    },
    /// An image drawn into a destination rectangle.
    Image {
        /// Destination rectangle in scope coordinates. The image is stretched to fill it.
        rect: Rect,
        /// The image to draw.
        image: ImageBitmap,
        /// Opacity from 0.0 (transparent) to 1.0 (opaque).
        alpha: f32,
        /// Color filter applied to the image, or `None` for none.
        color_filter: Option<ColorFilter>,
        /// How pixels are sampled when the image is scaled.
        sampling: ImageSampling,
        /// Optional source rectangle in image-pixel coordinates.
        /// When `None`, the entire image is drawn. When `Some`, only the
        /// specified sub-region of the source image is sampled.
        src_rect: Option<Rect>,
    },
    /// A laid-out run of text. See [`TextPrimitive`].
    Text(Box<TextPrimitive>),
    /// Shadow that requires blur processing. The renderer decides technique
    /// (GPU blur, CPU approximation, etc.).
    Shadow(ShadowPrimitive),
    /// Where a group of draws that [`DrawScope::with_transform`] places starts
    /// or ends. The primitives between are in the group's coordinates.
    Group(GroupMarker),
}

/// A run of text, positioned and ready to rasterize.
///
/// `rect` is *already resolved*: [`DrawScope::draw_text_at`] measures the
/// string, applies [`DrawTextStyle::align`] / [`DrawTextStyle::vertical_align`] inside
/// the requested box, and stores the result here. Renderers therefore lay the
/// glyphs out from `rect`'s top-left and never re-align — which is what keeps
/// what [`DrawScope::measure_text`] reported and what lands on screen the same
/// geometry.
#[derive(Clone, Debug, PartialEq)]
pub struct TextPrimitive {
    /// Tight block box: origin is the top-left of the first line's slot, size
    /// is the measured size.
    pub rect: Rect,
    /// Shared so redrawing an unchanged string each frame clones a pointer
    /// rather than the characters.
    pub text: std::rc::Rc<str>,
    /// Font, size, alignment and other styling the glyphs are laid out and drawn with.
    pub style: DrawTextStyle,
    /// Paint for the glyphs: a solid colour, or a gradient laid over the
    /// text block, as Compose's `drawText(brush)` lays it over the layout.
    pub brush: Brush,
}

/// Returns a shared `Rc<str>` for `text`, reusing the copy made on an earlier
/// frame when the content matches.
///
/// Apps hand `draw_text*` a `&str` every frame, and a score counter or label
/// is the same characters frame after frame — without this pool every call
/// copied them into a fresh `Rc<str>` anyway, defeating the sharing
/// [`TextPrimitive::text`] exists for. Hits are verified by content, so a hash
/// collision costs one fresh copy, never the wrong text. The pool clears
/// itself when full; a live scene re-warms within one frame.
fn shared_text_str(text: &str) -> Rc<str> {
    use std::hash::{Hash, Hasher};

    const POOL_CAPACITY: usize = 256;

    let mut hasher = crate::FxHasher::default();
    text.hash(&mut hasher);
    let key = hasher.finish();

    crate::thread_state::with(|state| {
        let mut pool = state.shared_texts.borrow_mut();
        if let Some(shared) = pool.get(&key)
            && &**shared == text
        {
            return Rc::clone(shared);
        }
        let shared: Rc<str> = Rc::from(text);
        if pool.len() >= POOL_CAPACITY {
            pool.clear();
        }
        pool.insert(key, Rc::clone(&shared));
        shared
    })
}

impl DrawPrimitive {
    /// The primitive moved by `dx`, `dy`. Brush geometry is relative to the
    /// primitive's rect, so it moves along.
    pub fn translate(self, dx: f32, dy: f32) -> Self {
        match self {
            DrawPrimitive::Content => DrawPrimitive::Content,
            DrawPrimitive::Blend {
                primitive,
                blend_mode,
            } => DrawPrimitive::Blend {
                primitive: Box::new(primitive.translate(dx, dy)),
                blend_mode,
            },
            DrawPrimitive::Rect {
                rect,
                brush,
                stroke,
            } => DrawPrimitive::Rect {
                rect: rect.translate(dx, dy),
                brush,
                stroke,
            },
            DrawPrimitive::RoundRect {
                rect,
                brush,
                radii,
                stroke,
            } => DrawPrimitive::RoundRect {
                rect: rect.translate(dx, dy),
                brush,
                radii,
                stroke,
            },
            DrawPrimitive::Arc {
                rect,
                brush,
                center,
                radius,
                start_angle,
                sweep_angle,
                stroke,
                inner_radius,
            } => DrawPrimitive::Arc {
                rect: rect.translate(dx, dy),
                brush,
                center: Point::new(center.x + dx, center.y + dy),
                radius,
                start_angle,
                sweep_angle,
                stroke,
                inner_radius,
            },
            DrawPrimitive::Line {
                rect,
                brush,
                start,
                end,
                stroke,
            } => DrawPrimitive::Line {
                rect: rect.translate(dx, dy),
                brush,
                start: Point::new(start.x + dx, start.y + dy),
                end: Point::new(end.x + dx, end.y + dy),
                stroke,
            },
            DrawPrimitive::Trapezoid {
                rect,
                brush,
                trapezoid,
            } => DrawPrimitive::Trapezoid {
                rect: rect.translate(dx, dy),
                brush,
                trapezoid: trapezoid.translate(dx, dy),
            },
            DrawPrimitive::Image {
                rect,
                image,
                alpha,
                color_filter,
                sampling,
                src_rect,
            } => DrawPrimitive::Image {
                rect: rect.translate(dx, dy),
                image,
                alpha,
                color_filter,
                sampling,
                src_rect,
            },
            DrawPrimitive::Text(mut text) => {
                text.rect = text.rect.translate(dx, dy);
                DrawPrimitive::Text(text)
            }
            DrawPrimitive::Shadow(ShadowPrimitive::Drop {
                shape,
                cutout,
                blur_radius,
                blend_mode,
            }) => DrawPrimitive::Shadow(ShadowPrimitive::Drop {
                shape: Box::new(shape.translate(dx, dy)),
                cutout: cutout.map(|cutout| Box::new(cutout.translate(dx, dy))),
                blur_radius,
                blend_mode,
            }),
            DrawPrimitive::Shadow(ShadowPrimitive::Inner {
                fill,
                cutout,
                blur_radius,
                blend_mode,
                clip_rect,
            }) => DrawPrimitive::Shadow(ShadowPrimitive::Inner {
                fill: Box::new(fill.translate(dx, dy)),
                cutout: Box::new(cutout.translate(dx, dy)),
                blur_radius,
                blend_mode,
                clip_rect: clip_rect.translate(dx, dy),
            }),
            DrawPrimitive::Group(GroupMarker::Start(group)) => {
                DrawPrimitive::Group(GroupMarker::Start(group.translate(dx, dy)))
            }
            DrawPrimitive::Group(GroupMarker::End) => DrawPrimitive::Group(GroupMarker::End),
        }
    }
}

/// A shadow that needs blur processing. Each renderer chooses how to blur. See
/// [`DrawPrimitive::Shadow`].
#[derive(Clone, Debug, PartialEq)]
pub enum ShadowPrimitive {
    /// Drop shadow: render shape behind content, blurred. `cutout` knocks
    /// the element's own (unoffset) shape out of the silhouette before the
    /// blur so translucent surfaces never sample their own shadow.
    Drop {
        /// The shape drawn and blurred behind the content.
        shape: Box<DrawPrimitive>,
        /// `Some` cuts the element's own unoffset shape out of the silhouette before the blur.
        cutout: Option<Box<DrawPrimitive>>,
        /// Blur radius of the shadow, in logical pixels.
        blur_radius: f32,
        /// How the shadow combines with what is already drawn.
        blend_mode: BlendMode,
    },
    /// Inner shadow: render fill + cutout to offscreen, blur, clip to bounds.
    Inner {
        /// The shape drawn and blurred to make the shadow.
        fill: Box<DrawPrimitive>,
        /// The shape cut out of the shadow's silhouette, so it appears only outside that shape.
        cutout: Box<DrawPrimitive>,
        /// Blur radius of the shadow, in logical pixels.
        blur_radius: f32,
        /// How the shadow combines with what is already drawn.
        blend_mode: BlendMode,
        /// Element bounds — blurred result must be clipped here.
        clip_rect: Rect,
    },
}

/// The drawing surface passed to draw modifiers. Compose's `DrawScope`. Every method records a
/// primitive in scope coordinates, with the origin at the top-left corner of the node, and the
/// `_blend` forms composite with a given blend mode instead of `SrcOver`.
pub trait DrawScope {
    /// Size of the drawing area, in logical pixels.
    fn size(&self) -> Size;
    /// Marks where the node's own content draws, as Compose's `drawContent()` does in
    /// `drawWithContent`. Draws recorded before the marker paint under the content and draws after
    /// it paint over it.
    fn draw_content(&mut self);
    /// Fills the whole scope rect with `brush`.
    fn draw_rect(&mut self, brush: Brush);
    /// As `draw_rect`, composited with `blend_mode` instead of `SrcOver`.
    fn draw_rect_blend(&mut self, brush: Brush, blend_mode: BlendMode);
    /// Draws a rectangle at the specified position and size.
    fn draw_rect_at(&mut self, rect: Rect, brush: Brush);
    /// As `draw_rect_at`, composited with `blend_mode` instead of `SrcOver`.
    fn draw_rect_at_blend(&mut self, rect: Rect, brush: Brush, blend_mode: BlendMode);
    /// Fills the whole scope rect with rounded corners of `radii`.
    fn draw_round_rect(&mut self, brush: Brush, radii: CornerRadii);
    /// As `draw_round_rect`, composited with `blend_mode` instead of `SrcOver`.
    fn draw_round_rect_blend(&mut self, brush: Brush, radii: CornerRadii, blend_mode: BlendMode);
    /// Draws a rounded rectangle at the specified position and size.
    fn draw_round_rect_at(&mut self, rect: Rect, brush: Brush, radii: CornerRadii);
    /// Fills a circle of `radius` around `center`, in scope coordinates.
    fn draw_circle(&mut self, brush: Brush, center: Point, radius: f32);
    /// As `draw_circle`, composited with `blend_mode` instead of `SrcOver`.
    fn draw_circle_blend(
        &mut self,
        brush: Brush,
        center: Point,
        radius: f32,
        blend_mode: BlendMode,
    );

    /// Strokes the outline of the whole scope rect.
    fn draw_rect_stroked(&mut self, brush: Brush, stroke: Stroke);
    /// As `draw_rect_stroked`, composited with `blend_mode` instead of `SrcOver`.
    fn draw_rect_stroked_blend(&mut self, brush: Brush, stroke: Stroke, blend_mode: BlendMode);
    /// Strokes the outline of `rect`.
    fn draw_rect_at_stroked(&mut self, rect: Rect, brush: Brush, stroke: Stroke);
    /// As `draw_rect_at_stroked`, composited with `blend_mode` instead of `SrcOver`.
    fn draw_rect_at_stroked_blend(
        &mut self,
        rect: Rect,
        brush: Brush,
        stroke: Stroke,
        blend_mode: BlendMode,
    );
    /// Strokes the outline of the whole scope rect with rounded corners.
    fn draw_round_rect_stroked(&mut self, brush: Brush, radii: CornerRadii, stroke: Stroke);
    /// As `draw_round_rect_stroked`, composited with `blend_mode` instead of `SrcOver`.
    fn draw_round_rect_stroked_blend(
        &mut self,
        brush: Brush,
        radii: CornerRadii,
        stroke: Stroke,
        blend_mode: BlendMode,
    );
    /// Strokes the outline of `rect` with rounded corners.
    fn draw_round_rect_at_stroked(
        &mut self,
        rect: Rect,
        brush: Brush,
        radii: CornerRadii,
        stroke: Stroke,
    );
    /// As `draw_round_rect_at_stroked`, composited with `blend_mode` instead of `SrcOver`.
    fn draw_round_rect_at_stroked_blend(
        &mut self,
        rect: Rect,
        brush: Brush,
        radii: CornerRadii,
        stroke: Stroke,
        blend_mode: BlendMode,
    );
    /// Strokes a circle outline. Lowers to a stroked rounded rect, so it shares
    /// the fill pipeline and batches with every other shape.
    fn draw_circle_stroked(&mut self, brush: Brush, center: Point, radius: f32, stroke: Stroke);
    /// As `draw_circle_stroked`, composited with `blend_mode` instead of `SrcOver`.
    fn draw_circle_stroked_blend(
        &mut self,
        brush: Brush,
        center: Point,
        radius: f32,
        stroke: Stroke,
        blend_mode: BlendMode,
    );

    /// Strokes a circular arc.
    ///
    /// Angles are in **radians**, `0` points along **+X**, and increasing
    /// angles sweep **clockwise on screen** (Cranpose uses y-down device
    /// coordinates, so this matches `atan2(dy, dx)` and the sweep-gradient
    /// brush). A negative `sweep_angle` sweeps counter-clockwise; `|sweep| >=
    /// 2π` draws a closed ring.
    ///
    /// The stroke is centered on `radius`, so the band covers
    /// `radius ± width/2`. [`StrokeCap`](crate::StrokeCap) shapes the two ends.
    /// Nothing is drawn for a zero sweep, a non-positive width, or non-finite
    /// input.
    fn draw_arc(
        &mut self,
        brush: Brush,
        center: Point,
        radius: f32,
        start_angle: f32,
        sweep_angle: f32,
        stroke: Stroke,
    );
    /// As `draw_arc`, composited with `blend_mode` instead of `SrcOver`.
    #[expect(clippy::too_many_arguments)]
    fn draw_arc_blend(
        &mut self,
        brush: Brush,
        center: Point,
        radius: f32,
        start_angle: f32,
        sweep_angle: f32,
        stroke: Stroke,
        blend_mode: BlendMode,
    );

    /// Strokes the straight segment from `start` to `end`, centred on it and
    /// `stroke.width` wide, its ends shaped by `stroke.cap`: Compose's
    /// `drawLine(brush, start, end, strokeWidth, cap)`. Nothing is drawn
    /// for a non-positive width, a non-finite end, or a segment of no length
    /// between butt caps; round and square caps draw a dot there.
    fn draw_line(&mut self, brush: Brush, start: Point, end: Point, stroke: Stroke);
    /// As `draw_line`, composited with `blend_mode` instead of `SrcOver`.
    fn draw_line_blend(
        &mut self,
        brush: Brush,
        start: Point,
        end: Point,
        stroke: Stroke,
        blend_mode: BlendMode,
    );

    /// Fills or strokes `path` with `brush`: Compose's
    /// `drawPath(path, brush, style)`. A fill closes every contour and
    /// rasterizes it as [`draw_vector_path`](Self::draw_vector_path) does. A
    /// stroke draws each flattened edge as a line on the GPU, capped at an
    /// open contour's ends and joined at its corners as
    /// [`crate::for_each_stroke_line`] describes; a dashed stroke draws the
    /// dashes its effect cuts. Where edges meet, a translucent stroke is
    /// painted twice.
    fn draw_path(&mut self, path: &Path, brush: Brush, style: DrawStyle);
    /// As `draw_path`, composited with `blend_mode` instead of `SrcOver`.
    fn draw_path_blend(
        &mut self,
        path: &Path,
        brush: Brush,
        style: DrawStyle,
        blend_mode: BlendMode,
    );

    /// Fills an annular sector — the region between `inner_radius` and
    /// `outer_radius`, limited to an angular sweep, with **flat radial ends**.
    ///
    /// This is the shape a stroked arc cannot express: its ends are straight
    /// lines through the center, not caps. `inner_radius = 0` fills a pie
    /// wedge. Angle convention is identical to [`draw_arc`](Self::draw_arc).
    /// Nothing is drawn when `inner_radius >= outer_radius`, the sweep is zero,
    /// or any input is non-finite.
    fn draw_annular_sector(
        &mut self,
        brush: Brush,
        center: Point,
        inner_radius: f32,
        outer_radius: f32,
        start_angle: f32,
        sweep_angle: f32,
    );
    /// As `draw_annular_sector`, composited with `blend_mode` instead of `SrcOver`.
    #[expect(clippy::too_many_arguments)]
    fn draw_annular_sector_blend(
        &mut self,
        brush: Brush,
        center: Point,
        inner_radius: f32,
        outer_radius: f32,
        start_angle: f32,
        sweep_angle: f32,
        blend_mode: BlendMode,
    );

    /// Draws `image` stretched over the whole scope rect.
    fn draw_image(&mut self, image: ImageBitmap);
    /// As `draw_image`, composited with `blend_mode` instead of `SrcOver`.
    fn draw_image_blend(&mut self, image: ImageBitmap, blend_mode: BlendMode);
    /// Draws `image` stretched into `rect`, with `alpha` from 0.0 to 1.0 and an optional
    /// `color_filter`.
    fn draw_image_at(
        &mut self,
        rect: Rect,
        image: ImageBitmap,
        alpha: f32,
        color_filter: Option<ColorFilter>,
    );
    /// As `draw_image_at`, sampling the image with `sampling` instead of nearest-neighbor.
    fn draw_image_at_sampled(
        &mut self,
        rect: Rect,
        image: ImageBitmap,
        alpha: f32,
        color_filter: Option<ColorFilter>,
        sampling: ImageSampling,
    );
    /// As `draw_image_at`, composited with `blend_mode` instead of `SrcOver`.
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
    /// As `draw_image_src`, sampling the source region with `sampling` instead of nearest-neighbor.
    fn draw_image_src_sampled(
        &mut self,
        image: ImageBitmap,
        src_rect: Rect,
        dst_rect: Rect,
        alpha: f32,
        color_filter: Option<ColorFilter>,
        sampling: ImageSampling,
    );
    /// As `draw_image_src_sampled`, composited with `blend_mode` instead of `SrcOver`.
    fn draw_image_src_blend(
        &mut self,
        image: ImageBitmap,
        src_rect: Rect,
        dst_rect: Rect,
        alpha: f32,
        color_filter: Option<ColorFilter>,
        blend_mode: BlendMode,
    );
    /// Fills a parsed SVG path in scope coordinates (path units are dp).
    ///
    /// The fill is rasterized on the CPU into a supersampled, anti-aliased
    /// bitmap covering the path bounds and drawn as an image primitive, so
    /// it works on every render backend. Parse the path once with
    /// [`crate::VectorPath::parse`] and redraw it per frame. A gradient brush
    /// resolves against the scope's size, as other fills do.
    fn draw_vector_path(&mut self, path: &crate::VectorPath, brush: Brush);
    /// Parses SVG path data (the `d` attribute syntax: `M/m L/l H/h V/v
    /// C/c S/s Q/q T/t A/a Z/z`) and fills it. Invalid path data draws
    /// nothing. Prefer [`crate::VectorPath::parse`] +
    /// [`draw_vector_path`](Self::draw_vector_path) to avoid re-parsing
    /// and to surface parse errors.
    fn draw_svg_path(&mut self, d: &str, brush: Brush) {
        if let Ok(path) = crate::VectorPath::parse(d) {
            self.draw_vector_path(&path, brush);
        }
    }

    /// The block size, line height and first baseline `text` would occupy in
    /// `style`.
    ///
    /// Free to call repeatedly: the underlying text stack caches metrics on
    /// `(text, style)`, so a game can measure every label every frame to center
    /// it without touching a font file more than once.
    fn measure_text(&self, text: &str, style: &DrawTextStyle) -> TextMeasurement;

    /// Draws `text` inside the whole scope rect, positioned by
    /// [`DrawTextStyle::align`] and [`DrawTextStyle::vertical_align`].
    fn draw_text(&mut self, brush: Brush, text: &str, style: &DrawTextStyle) {
        self.draw_text_at(Rect::from_size(self.size()), brush, text, style);
    }

    /// Draws `text` inside `rect`, positioned by [`DrawTextStyle::align`] and
    /// [`DrawTextStyle::vertical_align`].
    ///
    /// The glyphs are *not* clipped to `rect` — it is an alignment box, not a
    /// viewport. A `rect` narrower than the measured text overflows in the
    /// direction the alignment implies; clip the layer if that matters.
    fn draw_text_at(&mut self, rect: Rect, brush: Brush, text: &str, style: &DrawTextStyle);

    /// Draws `text` with the top-left corner of its block at `top_left`.
    ///
    /// Alignment is a no-op here because the box is the measurement — this is
    /// the "I already know where it goes" form, and the one to pair with
    /// [`measure_text`](Self::measure_text) for hand-rolled centering.
    fn draw_text_from(&mut self, top_left: Point, brush: Brush, text: &str, style: &DrawTextStyle) {
        if text.is_empty() {
            return;
        }
        let measurement = self.measure_text(text, style);
        self.draw_text_at(
            Rect::from_origin_size(top_left, measurement.size),
            brush,
            text,
            &DrawTextStyle {
                align: TextAlign::Left,
                vertical_align: TextVerticalAlign::Top,
                ..style.clone()
            },
        );
    }

    /// Draws `block` with every primitive it records, text and images
    /// included, placed by `transform`'s steps in order, each inside the
    /// ones before it: Compose's `withTransform`. The block sees the scope
    /// size the steps leave. Content that `draw_content` draws inside the
    /// block keeps its place: only the block's own draws move.
    fn with_transform(
        &mut self,
        transform: &[DrawTransform<'_>],
        block: &mut dyn FnMut(&mut dyn DrawScope),
    );

    /// Draws `block` moved by `dx`, `dy`. Compose's `translate`.
    fn translate(&mut self, dx: f32, dy: f32, block: &mut dyn FnMut(&mut dyn DrawScope)) {
        self.with_transform(&[DrawTransform::Translate { dx, dy }], block);
    }

    /// Draws `block` scaled by `scale_x`, `scale_y` about `pivot`, which
    /// stays where it is. Compose's `scale`.
    fn scale(
        &mut self,
        scale_x: f32,
        scale_y: f32,
        pivot: Point,
        block: &mut dyn FnMut(&mut dyn DrawScope),
    ) {
        self.with_transform(
            &[DrawTransform::Scale {
                scale_x,
                scale_y,
                pivot,
            }],
            block,
        );
    }

    /// Draws `block` turned clockwise by `degrees` about `pivot`. Compose's
    /// `rotate`.
    fn rotate(&mut self, degrees: f32, pivot: Point, block: &mut dyn FnMut(&mut dyn DrawScope)) {
        self.with_transform(&[DrawTransform::Rotate { degrees, pivot }], block);
    }

    /// Draws `block` in the scope shrunk by the four insets: it sees the
    /// smaller size and draws moved by `left`, `top`. Compose's `inset`.
    fn inset(
        &mut self,
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
        block: &mut dyn FnMut(&mut dyn DrawScope),
    ) {
        self.with_transform(
            &[DrawTransform::Inset {
                left,
                top,
                right,
                bottom,
            }],
            block,
        );
    }

    /// Draws `block` cut to `rect`. Compose's `clipRect`.
    fn clip_rect(&mut self, rect: Rect, block: &mut dyn FnMut(&mut dyn DrawScope)) {
        self.with_transform(&[DrawTransform::ClipRect(rect)], block);
    }

    /// Draws `block` cut to the fill of `path`, its edge anti-aliased.
    /// Compose's `clipPath`.
    fn clip_path(&mut self, path: &Path, block: &mut dyn FnMut(&mut dyn DrawScope)) {
        self.with_transform(&[DrawTransform::ClipPath(path)], block);
    }

    /// Consumes the scope and returns the recorded primitives in draw order, including the
    /// `Content` markers.
    fn into_primitives(self) -> Vec<DrawPrimitive>;
}

/// Resolves the top-left corner a text block of `measurement` gets when it is
/// aligned inside `rect`.
///
/// Split out so the placement rule is stated once and can be unit-tested
/// against the measurement it is derived from.
pub fn align_text_block(rect: Rect, measurement: TextMeasurement, style: &DrawTextStyle) -> Point {
    let x = match style.align {
        TextAlign::Left => rect.x,
        TextAlign::Center => rect.x + (rect.width - measurement.size.width) * 0.5,
        TextAlign::Right => rect.x + rect.width - measurement.size.width,
    };
    let y = match style.vertical_align {
        TextVerticalAlign::Top => rect.y,
        TextVerticalAlign::Center => rect.y + (rect.height - measurement.size.height) * 0.5,
        TextVerticalAlign::Bottom => rect.y + rect.height - measurement.size.height,
        TextVerticalAlign::Baseline => rect.y - measurement.first_baseline,
    };
    Point::new(x, y)
}

/// The rect a path's coverage mask covers: its bounds out to whole pixels and
/// one pixel more each way, so the anti-aliased edge fits.
fn vector_mask_rect(bounds: Rect) -> Rect {
    let x = bounds.x.floor() - 1.0;
    let y = bounds.y.floor() - 1.0;
    Rect {
        x,
        y,
        width: (bounds.x + bounds.width).ceil() - x + 1.0,
        height: (bounds.y + bounds.height).ceil() - y + 1.0,
    }
}

/// The `DrawScope` the framework passes to draw closures. It records the primitives drawn in draw
/// order, in place, into a [`CommandRecording`] its caller holds: a recording
/// kept from an earlier frame is drawn again in the storage it has.
pub struct DrawScopeDefault<'a> {
    size: Size,
    recording: CommandRecorder<'a>,
    text_measurer: Option<Rc<dyn DrawTextMeasurer>>,
}

impl<'a> DrawScopeDefault<'a> {
    /// Creates a scope of `size` recording into `recording` anew, without a
    /// text measurer, so text is measured by an estimate.
    pub fn new(size: Size, recording: &'a mut CommandRecording) -> Self {
        Self::with_storage(size, None, recording)
    }

    /// A scope that measures text with the app's fonts.
    ///
    /// The framework calls this for every draw closure it runs; `new` exists
    /// for callers that never draw text.
    pub fn with_text_measurer(
        size: Size,
        text_measurer: Rc<dyn DrawTextMeasurer>,
        recording: &'a mut CommandRecording,
    ) -> Self {
        Self::with_storage(size, Some(text_measurer), recording)
    }

    fn with_storage(
        size: Size,
        text_measurer: Option<Rc<dyn DrawTextMeasurer>>,
        recording: &'a mut CommandRecording,
    ) -> Self {
        Self {
            size,
            recording: recording.record(),
            text_measurer,
        }
    }

    /// How many `draw_content` markers this scope has recorded.
    pub fn content_marker_count(&self) -> u32 {
        self.recording.content_markers()
    }

    /// Runs `draw` in this scope's rect shrunk by `insets`, where a draw
    /// modifier placed after layout padding draws: `draw` sees the inner
    /// size, and what it records lands offset by the leading insets.
    pub fn draw_inset(&mut self, insets: EdgeInsets, draw: impl FnOnce(&mut DrawScopeDefault<'_>)) {
        let inner = insets.inset_rect(self.size);
        let mut recording = CommandRecording::default();
        let mut scope = DrawScopeDefault::with_storage(
            Size {
                width: inner.width,
                height: inner.height,
            },
            self.text_measurer.clone(),
            &mut recording,
        );
        draw(&mut scope);
        scope.finish();
        self.push_recorded(crate::translate_recorded(
            recording.primitives_with_markers(),
            inner.x,
            inner.y,
        ));
    }

    /// Records primitives already built, as if each had been drawn here.
    pub fn push_recorded(&mut self, primitives: impl IntoIterator<Item = DrawPrimitive>) {
        for primitive in primitives {
            self.recording.push_primitive(primitive);
        }
    }

    /// Completes the recording in the storage the scope records into.
    pub fn finish(self) {
        self.recording.finish();
    }

    fn push_blended_primitive(&mut self, primitive: DrawPrimitive, blend_mode: BlendMode) {
        if blend_mode != BlendMode::SrcOver {
            self.recording.push_other(DrawPrimitive::Blend {
                primitive: Box::new(primitive),
                blend_mode,
            });
            return;
        }
        self.recording.push_other(primitive);
    }

    #[expect(clippy::too_many_arguments)]
    #[inline]
    fn push_arc(
        &mut self,
        brush: Brush,
        center: Point,
        radius: f32,
        start_angle: f32,
        sweep_angle: f32,
        stroke: Option<Stroke>,
        inner_radius: f32,
        blend_mode: BlendMode,
    ) {
        let args = ArcRecordArgs {
            brush: &brush,
            center,
            radius,
            start_angle,
            sweep_angle,
            stroke,
            inner_radius,
            blend_mode,
        };
        let geometry = normalized_band(&args);
        if geometry.is_degenerate() {
            return;
        }
        self.recording.push_scope_arc(&args, &geometry);
    }

    /// Records the fill of `path` as the slices the GPU draws, its brush
    /// resolved against the scope's rect; a path that cannot be sliced is
    /// rasterized into a mask instead.
    fn push_path_fill(&mut self, path: &Path, brush: &Brush, blend_mode: BlendMode) {
        let brush_rect = Rect::from_size(self.size);
        let recording = &mut self.recording;
        let sliced = crate::thread_state::with(|state| {
            let mut slicer = state.path_slicer.borrow_mut();
            let contours = path
                .contours()
                .iter()
                .map(|contour| contour.points.as_slice());
            slicer
                .slice(contours, PathFillRule::NonZero)
                .map(|slices| recording.push_trapezoids(brush_rect, slices, brush, blend_mode))
                .is_some()
        });
        if !sliced {
            self.push_vector_path(
                &path.to_vector_path(PathFillRule::NonZero),
                brush,
                brush_rect,
                blend_mode,
                PathSide::Inside,
            );
        }
    }

    /// Draws `block` under the `rest` of its steps in a group that `mask`'s
    /// fill cuts. The group ends with a mask that erases what lies outside
    /// the fill: a blend every renderer draws, where keeping what lies inside
    /// is not.
    fn with_path_mask(
        &mut self,
        mask: &crate::VectorPath,
        rest: &[DrawTransform<'_>],
        block: &mut dyn FnMut(&mut dyn DrawScope),
    ) {
        let rect = vector_mask_rect(mask.bounds());
        self.recording.push_group_start(DrawGroup::masked(rect));
        self.with_transform(rest, block);
        self.push_vector_path(
            mask,
            &Brush::solid(Color::BLACK),
            rect,
            BlendMode::DstOut,
            PathSide::Outside,
        );
        self.recording.push_group_end();
    }

    /// Rasterizes the `side` of `path`'s edge into a cached image and
    /// records it, blended by `blend_mode`. A gradient `brush` resolves
    /// against `brush_rect`.
    fn push_vector_path(
        &mut self,
        path: &crate::VectorPath,
        brush: &Brush,
        brush_rect: Rect,
        blend_mode: BlendMode,
        side: PathSide,
    ) {
        const MAX_MASK_PIXELS: f32 = 4096.0;

        if path.is_empty() || !brush.paints() {
            return;
        }
        let bounds = path.bounds();
        if bounds.width <= 0.0 || bounds.height <= 0.0 {
            return;
        }

        let rect = vector_mask_rect(bounds);
        let origin = Point::new(rect.x, rect.y);
        let mask_width = (rect.width * VECTOR_PATH_SUPERSAMPLE)
            .ceil()
            .clamp(1.0, MAX_MASK_PIXELS) as usize;
        let mask_height = (rect.height * VECTOR_PATH_SUPERSAMPLE)
            .ceil()
            .clamp(1.0, MAX_MASK_PIXELS) as usize;
        let mask_size = (mask_width, mask_height);

        let key = vector_path_mask_key(path, origin, mask_size, brush, brush_rect, side);
        let image = match vector_path_mask_cache_get(key) {
            Some(image) => image,
            None => {
                let Some(image) =
                    rasterize_vector_path(path, brush, brush_rect, origin, mask_size, side)
                else {
                    return;
                };
                vector_path_mask_cache_put(key, image.clone());
                image
            }
        };

        let image = DrawPrimitive::Image {
            rect,
            image,
            alpha: 1.0,
            color_filter: None,
            sampling: ImageSampling::Linear,
            src_rect: None,
        };
        self.recording
            .push_other(if blend_mode == BlendMode::SrcOver {
                image
            } else {
                DrawPrimitive::Blend {
                    primitive: Box::new(image),
                    blend_mode,
                }
            });
    }
}

impl DrawScope for DrawScopeDefault<'_> {
    fn size(&self) -> Size {
        self.size
    }

    fn with_transform(
        &mut self,
        transform: &[DrawTransform<'_>],
        block: &mut dyn FnMut(&mut dyn DrawScope),
    ) {
        let Some((step, rest)) = transform.split_first() else {
            block(self);
            return;
        };
        let outer = self.size;
        let (group, size) = match *step {
            DrawTransform::Translate { dx, dy } => (DrawGroup::moved(dx, dy, outer), outer),
            DrawTransform::Scale {
                scale_x,
                scale_y,
                pivot,
            } => (DrawGroup::about(pivot, scale_x, scale_y, 0.0, outer), outer),
            DrawTransform::Rotate { degrees, pivot } => {
                (DrawGroup::about(pivot, 1.0, 1.0, degrees, outer), outer)
            }
            DrawTransform::Inset {
                left,
                top,
                right,
                bottom,
            } => {
                let inner = Size::new(
                    (outer.width - left - right).max(0.0),
                    (outer.height - top - bottom).max(0.0),
                );
                (DrawGroup::moved(left, top, inner), inner)
            }
            DrawTransform::ClipRect(rect) => (DrawGroup::plain(rect, true), outer),
            DrawTransform::ClipPath(path) => {
                let mask = path.to_vector_path(PathFillRule::NonZero);
                if !mask.is_empty() {
                    self.with_path_mask(&mask, rest, block);
                    return;
                }
                // A path with no fill leaves nothing of the block.
                (DrawGroup::plain(Rect::EMPTY, true), outer)
            }
        };
        self.recording.push_group_start(group);
        self.size = size;
        self.with_transform(rest, block);
        self.size = outer;
        self.recording.push_group_end();
    }

    fn draw_content(&mut self) {
        self.recording.push_content();
    }

    fn draw_rect(&mut self, brush: Brush) {
        self.draw_rect_blend(brush, BlendMode::SrcOver);
    }

    fn draw_rect_blend(&mut self, brush: Brush, blend_mode: BlendMode) {
        self.recording
            .push_rect(Rect::from_size(self.size), &brush, None, blend_mode);
    }

    fn draw_rect_at(&mut self, rect: Rect, brush: Brush) {
        self.draw_rect_at_blend(rect, brush, BlendMode::SrcOver);
    }

    fn draw_rect_at_blend(&mut self, rect: Rect, brush: Brush, blend_mode: BlendMode) {
        self.recording.push_rect(rect, &brush, None, blend_mode);
    }

    fn draw_round_rect(&mut self, brush: Brush, radii: CornerRadii) {
        self.draw_round_rect_blend(brush, radii, BlendMode::SrcOver);
    }

    fn draw_round_rect_blend(&mut self, brush: Brush, radii: CornerRadii, blend_mode: BlendMode) {
        self.recording
            .push_round_rect(Rect::from_size(self.size), &brush, radii, None, blend_mode);
    }

    fn draw_round_rect_at(&mut self, rect: Rect, brush: Brush, radii: CornerRadii) {
        self.recording
            .push_round_rect(rect, &brush, radii, None, BlendMode::SrcOver);
    }

    fn draw_rect_stroked(&mut self, brush: Brush, stroke: Stroke) {
        self.draw_rect_stroked_blend(brush, stroke, BlendMode::SrcOver);
    }

    fn draw_rect_stroked_blend(&mut self, brush: Brush, stroke: Stroke, blend_mode: BlendMode) {
        self.draw_rect_at_stroked_blend(Rect::from_size(self.size), brush, stroke, blend_mode);
    }

    fn draw_rect_at_stroked(&mut self, rect: Rect, brush: Brush, stroke: Stroke) {
        self.draw_rect_at_stroked_blend(rect, brush, stroke, BlendMode::SrcOver);
    }

    fn draw_rect_at_stroked_blend(
        &mut self,
        rect: Rect,
        brush: Brush,
        stroke: Stroke,
        blend_mode: BlendMode,
    ) {
        if !stroke.is_visible() {
            return;
        }
        self.recording
            .push_rect(rect, &brush, Some(stroke), blend_mode);
    }

    fn draw_round_rect_stroked(&mut self, brush: Brush, radii: CornerRadii, stroke: Stroke) {
        self.draw_round_rect_stroked_blend(brush, radii, stroke, BlendMode::SrcOver);
    }

    fn draw_round_rect_stroked_blend(
        &mut self,
        brush: Brush,
        radii: CornerRadii,
        stroke: Stroke,
        blend_mode: BlendMode,
    ) {
        self.draw_round_rect_at_stroked_blend(
            Rect::from_size(self.size),
            brush,
            radii,
            stroke,
            blend_mode,
        );
    }

    fn draw_round_rect_at_stroked(
        &mut self,
        rect: Rect,
        brush: Brush,
        radii: CornerRadii,
        stroke: Stroke,
    ) {
        self.draw_round_rect_at_stroked_blend(rect, brush, radii, stroke, BlendMode::SrcOver);
    }

    fn draw_round_rect_at_stroked_blend(
        &mut self,
        rect: Rect,
        brush: Brush,
        radii: CornerRadii,
        stroke: Stroke,
        blend_mode: BlendMode,
    ) {
        if !stroke.is_visible() {
            return;
        }
        self.recording
            .push_round_rect(rect, &brush, radii, Some(stroke), blend_mode);
    }

    fn draw_circle_stroked(&mut self, brush: Brush, center: Point, radius: f32, stroke: Stroke) {
        self.draw_circle_stroked_blend(brush, center, radius, stroke, BlendMode::SrcOver);
    }

    fn draw_circle_stroked_blend(
        &mut self,
        brush: Brush,
        center: Point,
        radius: f32,
        stroke: Stroke,
        blend_mode: BlendMode,
    ) {
        if !stroke.is_visible() || !radius.is_finite() {
            return;
        }
        let radius = radius.max(0.0);
        let diameter = radius * 2.0;
        self.draw_round_rect_at_stroked_blend(
            Rect {
                x: center.x - radius,
                y: center.y - radius,
                width: diameter,
                height: diameter,
            },
            brush,
            CornerRadii::uniform(radius),
            stroke,
            blend_mode,
        );
    }

    fn draw_arc(
        &mut self,
        brush: Brush,
        center: Point,
        radius: f32,
        start_angle: f32,
        sweep_angle: f32,
        stroke: Stroke,
    ) {
        self.draw_arc_blend(
            brush,
            center,
            radius,
            start_angle,
            sweep_angle,
            stroke,
            BlendMode::SrcOver,
        );
    }

    fn draw_arc_blend(
        &mut self,
        brush: Brush,
        center: Point,
        radius: f32,
        start_angle: f32,
        sweep_angle: f32,
        stroke: Stroke,
        blend_mode: BlendMode,
    ) {
        if !stroke.is_visible() {
            return;
        }
        self.push_arc(
            brush,
            center,
            radius,
            start_angle,
            sweep_angle,
            Some(stroke),
            0.0,
            blend_mode,
        );
    }

    fn draw_line(&mut self, brush: Brush, start: Point, end: Point, stroke: Stroke) {
        self.draw_line_blend(brush, start, end, stroke, BlendMode::SrcOver);
    }

    fn draw_line_blend(
        &mut self,
        brush: Brush,
        start: Point,
        end: Point,
        stroke: Stroke,
        blend_mode: BlendMode,
    ) {
        let line = LineGeometry::new(start, end, stroke);
        if line.is_degenerate() {
            return;
        }
        self.recording.push_line(&line, &brush, stroke, blend_mode);
    }

    fn draw_path(&mut self, path: &Path, brush: Brush, style: DrawStyle) {
        self.draw_path_blend(path, brush, style, BlendMode::SrcOver);
    }

    fn draw_path_blend(
        &mut self,
        path: &Path,
        brush: Brush,
        style: DrawStyle,
        blend_mode: BlendMode,
    ) {
        let (stroke, dash) = match style {
            DrawStyle::Fill => {
                self.push_path_fill(path, &brush, blend_mode);
                return;
            }
            DrawStyle::Stroke(stroke) => (stroke, None),
            DrawStyle::DashedStroke(stroke, dash) => (stroke, Some(dash)),
        };
        if !stroke.is_visible() {
            return;
        }
        let recording = &mut self.recording;
        let mut push = |line: LineGeometry| {
            if !line.is_degenerate() {
                let stroke = Stroke {
                    cap: line.cap,
                    ..stroke
                };
                recording.push_line(&line, &brush, stroke, blend_mode);
            }
        };
        for contour in path.contours() {
            let Some(dash) = dash else {
                crate::for_each_stroke_line(&contour.points, contour.closed, stroke, &mut push);
                continue;
            };
            let closing = contour
                .closed
                .then(|| contour.points.first().copied())
                .flatten();
            let points: std::borrow::Cow<'_, [Point]> = match closing {
                Some(first) => {
                    std::borrow::Cow::Owned(contour.points.iter().copied().chain([first]).collect())
                }
                None => std::borrow::Cow::Borrowed(&contour.points),
            };
            dash.for_each_dash(&points, |piece| {
                crate::for_each_stroke_line(piece, false, stroke, &mut push);
            });
        }
    }

    fn draw_annular_sector(
        &mut self,
        brush: Brush,
        center: Point,
        inner_radius: f32,
        outer_radius: f32,
        start_angle: f32,
        sweep_angle: f32,
    ) {
        self.draw_annular_sector_blend(
            brush,
            center,
            inner_radius,
            outer_radius,
            start_angle,
            sweep_angle,
            BlendMode::SrcOver,
        );
    }

    fn draw_annular_sector_blend(
        &mut self,
        brush: Brush,
        center: Point,
        inner_radius: f32,
        outer_radius: f32,
        start_angle: f32,
        sweep_angle: f32,
        blend_mode: BlendMode,
    ) {
        self.push_arc(
            brush,
            center,
            outer_radius,
            start_angle,
            sweep_angle,
            None,
            inner_radius,
            blend_mode,
        );
    }

    fn draw_circle(&mut self, brush: Brush, center: Point, radius: f32) {
        self.draw_circle_blend(brush, center, radius, BlendMode::SrcOver);
    }

    fn draw_circle_blend(
        &mut self,
        brush: Brush,
        center: Point,
        radius: f32,
        blend_mode: BlendMode,
    ) {
        let radius = radius.max(0.0);
        let diameter = radius * 2.0;
        self.recording.push_round_rect(
            Rect {
                x: center.x - radius,
                y: center.y - radius,
                width: diameter,
                height: diameter,
            },
            &brush,
            CornerRadii::uniform(radius),
            None,
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
                sampling: ImageSampling::Nearest,
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
        self.draw_image_at_sampled(rect, image, alpha, color_filter, ImageSampling::Nearest);
    }

    fn draw_image_at_sampled(
        &mut self,
        rect: Rect,
        image: ImageBitmap,
        alpha: f32,
        color_filter: Option<ColorFilter>,
        sampling: ImageSampling,
    ) {
        self.push_blended_primitive(
            DrawPrimitive::Image {
                rect,
                image,
                alpha: alpha.clamp(0.0, 1.0),
                color_filter,
                sampling,
                src_rect: None,
            },
            BlendMode::SrcOver,
        );
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
                sampling: ImageSampling::Nearest,
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

    fn draw_image_src_sampled(
        &mut self,
        image: ImageBitmap,
        src_rect: Rect,
        dst_rect: Rect,
        alpha: f32,
        color_filter: Option<ColorFilter>,
        sampling: ImageSampling,
    ) {
        self.push_blended_primitive(
            DrawPrimitive::Image {
                rect: dst_rect,
                image,
                alpha: alpha.clamp(0.0, 1.0),
                color_filter,
                sampling,
                src_rect: Some(src_rect),
            },
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
                sampling: ImageSampling::Nearest,
                src_rect: Some(src_rect),
            },
            blend_mode,
        );
    }

    fn draw_vector_path(&mut self, path: &crate::VectorPath, brush: Brush) {
        self.push_vector_path(
            path,
            &brush,
            Rect::from_size(self.size),
            BlendMode::SrcOver,
            PathSide::Inside,
        );
    }

    fn measure_text(&self, text: &str, style: &DrawTextStyle) -> TextMeasurement {
        match &self.text_measurer {
            Some(measurer) => measurer.measure_text(text, style),
            None => estimate_text_measurement(text, style),
        }
    }

    fn draw_text_at(&mut self, rect: Rect, brush: Brush, text: &str, style: &DrawTextStyle) {
        if text.is_empty() || !brush.paints() {
            return;
        }
        let measurement = self.measure_text(text, style);
        if !(measurement.size.width > 0.0 && measurement.size.height > 0.0) {
            return;
        }
        let origin = align_text_block(rect, measurement, style);
        if !origin.x.is_finite() || !origin.y.is_finite() {
            return;
        }
        self.recording
            .push_other(DrawPrimitive::Text(Box::new(TextPrimitive {
                rect: Rect::from_origin_size(origin, measurement.size),
                text: shared_text_str(text),
                style: style.clone(),
                brush,
            })));
    }

    fn into_primitives(self) -> Vec<DrawPrimitive> {
        self.recording.into_primitives()
    }
}

#[cfg(test)]
#[path = "tests/geometry_tests.rs"]
mod tests;
