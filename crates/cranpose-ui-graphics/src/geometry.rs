//! Geometric primitives: Point, Size, Rect, Insets, Path

use crate::stroke::{arc_band, ArcGeometry, Stroke};
use crate::typography::{
    estimate_text_measurement, DrawTextMeasurer, TextAlign, TextMeasurement, TextStyle,
    TextVerticalAlign,
};
use crate::{Brush, Color, ColorFilter, ImageBitmap, ImageSampling};
use std::ops::AddAssign;
use std::rc::Rc;

const VECTOR_PATH_MASK_CACHE_ENTRIES: usize = 96;
const VECTOR_PATH_MASK_CACHE_BYTES: usize = 8 * 1024 * 1024;

struct VectorPathMaskCache {
    entries: Vec<(u64, ImageBitmap)>,
    bytes: usize,
}

impl VectorPathMaskCache {
    const fn new() -> Self {
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
        let bytes = image.width() as usize * image.height() as usize * 4;
        if bytes > VECTOR_PATH_MASK_CACHE_BYTES {
            return;
        }
        self.bytes += bytes;
        self.entries.push((key, image));
        while self.entries.len() > VECTOR_PATH_MASK_CACHE_ENTRIES
            || self.bytes > VECTOR_PATH_MASK_CACHE_BYTES
        {
            let (_, dropped) = self.entries.remove(0);
            self.bytes = self
                .bytes
                .saturating_sub(dropped.width() as usize * dropped.height() as usize * 4);
        }
    }
}

thread_local! {
    static VECTOR_PATH_MASKS: std::cell::RefCell<VectorPathMaskCache> =
        const { std::cell::RefCell::new(VectorPathMaskCache::new()) };
}

fn vector_path_mask_key(
    path: &crate::VectorPath,
    origin: Point,
    mask_size: (usize, usize),
    rgb: [u8; 3],
    alpha: f32,
) -> u64 {
    use std::hash::Hasher;
    let mut hasher = crate::fx_hash::FxHasher::default();
    hasher.write_u8(path.fill_rule() as u8);
    hasher.write_u32(origin.x.to_bits());
    hasher.write_u32(origin.y.to_bits());
    hasher.write_usize(mask_size.0);
    hasher.write_usize(mask_size.1);
    hasher.write(&rgb);
    hasher.write_u32(alpha.to_bits());
    for subpath in path.subpaths() {
        hasher.write_usize(subpath.len());
        for point in subpath {
            hasher.write_u32(point.x.to_bits());
            hasher.write_u32(point.y.to_bits());
        }
    }
    hasher.finish()
}

fn vector_path_mask_cache_get(key: u64) -> Option<ImageBitmap> {
    VECTOR_PATH_MASKS.with(|cache| cache.borrow_mut().get(key))
}

fn vector_path_mask_cache_put(key: u64, image: ImageBitmap) {
    VECTOR_PATH_MASKS.with(|cache| cache.borrow_mut().put(key, image));
}

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

impl GraphicsLayer {
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
        /// `None` fills the rect; `Some` strokes its outline, centered on the
        /// edge (so it bleeds `width / 2` outside `rect`).
        stroke: Option<Stroke>,
    },
    RoundRect {
        rect: Rect,
        brush: Brush,
        radii: CornerRadii,
        /// `None` fills the rounded rect; `Some` strokes its outline, centered
        /// on the edge.
        stroke: Option<Stroke>,
    },
    /// A circular band: a stroked arc, or a filled annular sector / pie wedge.
    ///
    /// Angles are radians, `0` = +X, increasing **clockwise** on screen (see
    /// [`crate::stroke`] for the full convention).
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
        brush: Brush,
        center: Point,
        radius: f32,
        start_angle: f32,
        sweep_angle: f32,
        stroke: Option<Stroke>,
        /// `> 0` turns a filled wedge into an annular sector.
        inner_radius: f32,
    },
    Image {
        rect: Rect,
        image: ImageBitmap,
        alpha: f32,
        color_filter: Option<ColorFilter>,
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
}

/// A run of text, positioned and ready to rasterize.
///
/// `rect` is *already resolved*: [`DrawScope::draw_text_at`] measures the
/// string, applies [`TextStyle::align`] / [`TextStyle::vertical_align`] inside
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
    pub style: TextStyle,
    /// Text is filled with a single color: the glyph atlas path modulates one
    /// vertex color per glyph. Gradient brushes are resolved to their first
    /// stop by the draw scope, exactly like [`DrawScope::draw_vector_path`].
    pub color: Color,
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
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::hash::{Hash, Hasher};

    const POOL_CAPACITY: usize = 256;
    thread_local! {
        static POOL: RefCell<HashMap<u64, Rc<str>>> = RefCell::new(HashMap::new());
    }

    let mut hasher = crate::FxHasher::default();
    text.hash(&mut hasher);
    let key = hasher.finish();

    POOL.with(|pool| {
        let mut pool = pool.borrow_mut();
        if let Some(shared) = pool.get(&key) {
            if &**shared == text {
                return Rc::clone(shared);
            }
        }
        let shared: Rc<str> = Rc::from(text);
        if pool.len() >= POOL_CAPACITY {
            pool.clear();
        }
        pool.insert(key, Rc::clone(&shared));
        shared
    })
}

/// Describes a shadow to be rendered. Each renderer chooses how to blur.
#[derive(Clone, Debug, PartialEq)]
pub enum ShadowPrimitive {
    /// Drop shadow: render shape behind content, blurred. `cutout` knocks
    /// the element's own (unoffset) shape out of the silhouette before the
    /// blur so translucent surfaces never sample their own shadow.
    Drop {
        shape: Box<DrawPrimitive>,
        cutout: Option<Box<DrawPrimitive>>,
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
    /// Draws a rounded rectangle at the specified position and size.
    fn draw_round_rect_at(&mut self, rect: Rect, brush: Brush, radii: CornerRadii);
    fn draw_circle(&mut self, brush: Brush, center: Point, radius: f32);
    fn draw_circle_blend(
        &mut self,
        brush: Brush,
        center: Point,
        radius: f32,
        blend_mode: BlendMode,
    );

    // ── Stroked outlines ────────────────────────────────────────────────────
    //
    // Strokes are *centered* on the geometry: a `width`-wide stroke covers
    // `width / 2` inside and `width / 2` outside the path, like Skia and
    // Jetpack Compose. A non-positive or non-finite width draws nothing.

    /// Strokes the outline of the whole scope rect.
    fn draw_rect_stroked(&mut self, brush: Brush, stroke: Stroke);
    fn draw_rect_stroked_blend(&mut self, brush: Brush, stroke: Stroke, blend_mode: BlendMode);
    /// Strokes the outline of `rect`.
    fn draw_rect_at_stroked(&mut self, rect: Rect, brush: Brush, stroke: Stroke);
    fn draw_rect_at_stroked_blend(
        &mut self,
        rect: Rect,
        brush: Brush,
        stroke: Stroke,
        blend_mode: BlendMode,
    );
    /// Strokes the outline of the whole scope rect with rounded corners.
    fn draw_round_rect_stroked(&mut self, brush: Brush, radii: CornerRadii, stroke: Stroke);
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
    #[allow(clippy::too_many_arguments)]
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
    fn draw_circle_stroked_blend(
        &mut self,
        brush: Brush,
        center: Point,
        radius: f32,
        stroke: Stroke,
        blend_mode: BlendMode,
    );

    // ── Arcs ────────────────────────────────────────────────────────────────

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
    #[allow(clippy::too_many_arguments)]
    fn draw_arc(
        &mut self,
        brush: Brush,
        center: Point,
        radius: f32,
        start_angle: f32,
        sweep_angle: f32,
        stroke: Stroke,
    );
    #[allow(clippy::too_many_arguments)]
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

    /// Fills an annular sector — the region between `inner_radius` and
    /// `outer_radius`, limited to an angular sweep, with **flat radial ends**.
    ///
    /// This is the shape a stroked arc cannot express: its ends are straight
    /// lines through the center, not caps. `inner_radius = 0` fills a pie
    /// wedge. Angle convention is identical to [`draw_arc`](Self::draw_arc).
    /// Nothing is drawn when `inner_radius >= outer_radius`, the sweep is zero,
    /// or any input is non-finite.
    #[allow(clippy::too_many_arguments)]
    fn draw_annular_sector(
        &mut self,
        brush: Brush,
        center: Point,
        inner_radius: f32,
        outer_radius: f32,
        start_angle: f32,
        sweep_angle: f32,
    );
    #[allow(clippy::too_many_arguments)]
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

    fn draw_image(&mut self, image: ImageBitmap);
    fn draw_image_blend(&mut self, image: ImageBitmap, blend_mode: BlendMode);
    fn draw_image_at(
        &mut self,
        rect: Rect,
        image: ImageBitmap,
        alpha: f32,
        color_filter: Option<ColorFilter>,
    );
    fn draw_image_at_sampled(
        &mut self,
        rect: Rect,
        image: ImageBitmap,
        alpha: f32,
        color_filter: Option<ColorFilter>,
        sampling: ImageSampling,
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
    fn draw_image_src_sampled(
        &mut self,
        image: ImageBitmap,
        src_rect: Rect,
        dst_rect: Rect,
        alpha: f32,
        color_filter: Option<ColorFilter>,
        sampling: ImageSampling,
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
    /// Fills a parsed SVG path in scope coordinates (path units are dp).
    ///
    /// The fill is rasterized on the CPU into a supersampled, anti-aliased
    /// bitmap covering the path bounds and drawn as an image primitive, so
    /// it works on every render backend. Parse the path once with
    /// [`crate::VectorPath::parse`] and redraw it per frame. Solid brushes
    /// are honored exactly; gradient brushes currently fall back to their
    /// first stop color.
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

    // ── Text ────────────────────────────────────────────────────────────────
    //
    // Text is the one primitive a draw scope cannot resolve on its own: it
    // needs fonts, which live above this crate. Measurement is therefore
    // delegated to whatever the UI layer installed on the scope, and the same
    // measurement decides where `draw_text*` puts the glyphs — so a caller that
    // centers text from `measure_text` and the renderer that rasterizes it are
    // reading the same numbers.
    //
    // There is deliberately no `_blend` variant: the text draw path has no
    // blend-mode channel (glyphs are composited `SrcOver` against the atlas
    // coverage mask), so a blended overload could only lie about what it does.

    /// The block size, line height and first baseline `text` would occupy in
    /// `style`.
    ///
    /// Free to call repeatedly: the underlying text stack caches metrics on
    /// `(text, style)`, so a game can measure every label every frame to center
    /// it without touching a font file more than once.
    fn measure_text(&self, text: &str, style: &TextStyle) -> TextMeasurement;

    /// Draws `text` inside the whole scope rect, positioned by
    /// [`TextStyle::align`] and [`TextStyle::vertical_align`].
    fn draw_text(&mut self, brush: Brush, text: &str, style: &TextStyle) {
        self.draw_text_at(Rect::from_size(self.size()), brush, text, style);
    }

    /// Draws `text` inside `rect`, positioned by [`TextStyle::align`] and
    /// [`TextStyle::vertical_align`].
    ///
    /// The glyphs are *not* clipped to `rect` — it is an alignment box, not a
    /// viewport. A `rect` narrower than the measured text overflows in the
    /// direction the alignment implies; clip the layer if that matters.
    fn draw_text_at(&mut self, rect: Rect, brush: Brush, text: &str, style: &TextStyle);

    /// Draws `text` with the top-left corner of its block at `top_left`.
    ///
    /// Alignment is a no-op here because the box is the measurement — this is
    /// the "I already know where it goes" form, and the one to pair with
    /// [`measure_text`](Self::measure_text) for hand-rolled centering.
    fn draw_text_from(&mut self, top_left: Point, brush: Brush, text: &str, style: &TextStyle) {
        if text.is_empty() {
            return;
        }
        let measurement = self.measure_text(text, style);
        self.draw_text_at(
            Rect::from_origin_size(top_left, measurement.size),
            brush,
            text,
            &TextStyle {
                align: TextAlign::Left,
                vertical_align: TextVerticalAlign::Top,
                ..style.clone()
            },
        );
    }

    fn into_primitives(self) -> Vec<DrawPrimitive>;
}

/// Resolves the top-left corner a text block of `measurement` gets when it is
/// aligned inside `rect`.
///
/// Split out so the placement rule is stated once and can be unit-tested
/// against the measurement it is derived from.
pub fn align_text_block(rect: Rect, measurement: TextMeasurement, style: &TextStyle) -> Point {
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

/// Which typed store holds one recorded draw. The tape preserves the exact
/// draw order across the per-kind stores.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum RecordKind {
    SolidRect,
    SolidRoundRect,
    SolidArc,
    Other,
}

/// Tagged reference to one recorded draw: kind in the top 2 bits, index into
/// that kind's typed store in the low 30. Four bytes per entry buys direct
/// dispatch everywhere a tape range is consumed — no per-kind cursor walks,
/// no per-frame view tables (sol P4a).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TapeRef(u32);

impl TapeRef {
    const KIND_SHIFT: u32 = 30;
    const INDEX_MASK: u32 = (1 << Self::KIND_SHIFT) - 1;

    pub(crate) fn new(kind: RecordKind, index: usize) -> Self {
        debug_assert!(index < (1usize << Self::KIND_SHIFT));
        Self(((kind as u32) << Self::KIND_SHIFT) | index as u32)
    }

    pub(crate) fn kind(self) -> RecordKind {
        match self.0 >> Self::KIND_SHIFT {
            0 => RecordKind::SolidRect,
            1 => RecordKind::SolidRoundRect,
            2 => RecordKind::SolidArc,
            _ => RecordKind::Other,
        }
    }

    pub(crate) fn index(self) -> usize {
        (self.0 & Self::INDEX_MASK) as usize
    }

    /// The packed word itself. Because per-store indices appear on the tape
    /// in strictly increasing order (see [`CommandRecording::tape`]), `d`
    /// consecutive tape entries are one kind's run exactly when
    /// `tape[p + d].raw() == tape[p].raw() + d` — same kind bits, index
    /// advanced by every step. Span verification decomposes tape ranges into
    /// per-kind contiguous store runs on this single comparison.
    pub(crate) fn raw(self) -> u32 {
        self.0
    }
}

/// A solid `SrcOver` rect, recorded as raw values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SolidRectRecord {
    pub rect: Rect,
    pub color: Color,
    pub stroke: Option<Stroke>,
}

/// A solid `SrcOver` rounded rect (also the lowering of `draw_circle`),
/// recorded as raw values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SolidRoundRectRecord {
    pub rect: Rect,
    pub radii: CornerRadii,
    pub color: Color,
    pub stroke: Option<Stroke>,
}

/// A solid `SrcOver` arc, recorded as the RAW draw parameters — before
/// band resolution, tight-bounds trigonometry, or the degeneracy check,
/// all of which happen at materialization. Retention verification must be
/// able to compare what the app said, ahead of everything deriving from it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SolidArcRecord {
    pub center: Point,
    pub radius: f32,
    pub start_angle: f32,
    pub sweep_angle: f32,
    pub inner_radius: f32,
    pub color: Color,
    pub stroke: Option<Stroke>,
}

/// One draw command's recording in compact typed form: pure-numeric records
/// for the common solid shapes (no `Brush` destructor branch, roughly half
/// the bytes of the `DrawPrimitive` they materialize into) and ordinary
/// primitives for everything else, with `tape` preserving global order. The
/// index each tape entry carries into its per-kind store is the stable
/// compact handle for the rare resource-bearing entries (`others` holds
/// gradients, images, text, blends, and content markers whole).
#[derive(Clone, Debug, Default)]
pub struct CommandRecording {
    /// INVARIANT: per-store indices appear on the tape in strictly
    /// increasing order (0, 1, 2, ... per kind) — recording appends only.
    /// Sequential consumers (`finish`'s `others.drain(..)`) rely on it;
    /// random-access consumers use the index directly.
    pub(crate) tape: Vec<TapeRef>,
    pub(crate) rects: Vec<SolidRectRecord>,
    pub(crate) round_rects: Vec<SolidRoundRectRecord>,
    pub(crate) arcs: Vec<SolidArcRecord>,
    pub(crate) others: Vec<DrawPrimitive>,
}

impl CommandRecording {
    /// Total recorded entries (the tape length).
    pub fn len(&self) -> usize {
        self.tape.len()
    }

    /// Materializes one tape range into fresh primitives — the emergency
    /// path for a bypassed span whose retained draw fell through. `None`
    /// when the range does not lie within this recording (cleared buffers,
    /// stale range).
    pub fn materialize_range(
        &self,
        tape_start: usize,
        tape_end: usize,
    ) -> Option<Vec<DrawPrimitive>> {
        if tape_start > tape_end || tape_end > self.tape.len() {
            return None;
        }
        let mut out = Vec::with_capacity(tape_end - tape_start);
        for entry in &self.tape[tape_start..tape_end] {
            match entry.kind() {
                RecordKind::SolidRect => {
                    let record = self.rects.get(entry.index())?;
                    out.push(DrawPrimitive::Rect {
                        rect: record.rect,
                        brush: Brush::Solid(record.color),
                        stroke: record.stroke,
                    });
                }
                RecordKind::SolidRoundRect => {
                    let record = self.round_rects.get(entry.index())?;
                    out.push(DrawPrimitive::RoundRect {
                        rect: record.rect,
                        brush: Brush::Solid(record.color),
                        radii: record.radii,
                        stroke: record.stroke,
                    });
                }
                RecordKind::SolidArc => {
                    let record = self.arcs.get(entry.index())?;
                    if let Some(primitive) = materialize_solid_arc(record) {
                        out.push(primitive);
                    }
                }
                RecordKind::Other => {
                    out.push(self.others.get(entry.index())?.clone());
                }
            }
        }
        Some(out)
    }

    pub fn is_empty(&self) -> bool {
        self.tape.is_empty()
    }

    /// Test support: the identity of the tape's allocation, so buffer-reuse
    /// tests can assert that re-recording ping-pongs between the same two
    /// allocations instead of growing fresh ones every frame.
    #[doc(hidden)]
    pub fn tape_ptr(&self) -> *const u8 {
        self.tape.as_ptr() as *const u8
    }

    fn clear(&mut self) {
        self.tape.clear();
        self.rects.clear();
        self.round_rects.clear();
        self.arcs.clear();
        self.others.clear();
    }

    /// A buffer-reusing deep copy: `clone_from` on every store, so
    /// refreshing a long-lived recording (the replay snapshot, twice per
    /// convergence cycle on a heavy scene's ~17k-record tape) truncates and
    /// copies into the existing allocations instead of cloning five fresh
    /// buffers. Semantically identical to `*self = source.clone()`.
    pub(crate) fn clone_records_from(&mut self, source: &Self) {
        self.tape.clone_from(&source.tape);
        self.rects.clone_from(&source.rects);
        self.round_rects.clone_from(&source.round_rects);
        self.arcs.clone_from(&source.arcs);
        self.others.clone_from(&source.others);
    }
}

/// What [`DrawScopeDefault::finish`] hands back: the materialized primitives,
/// the marker count that travels with them, and the recording buffers so a
/// retaining caller can lend them to the same command's next recording.
pub struct FinishedRecording {
    pub primitives: Vec<DrawPrimitive>,
    pub content_markers: u32,
    pub recording: CommandRecording,
    /// Tape indices that materialized to nothing (degenerate arcs), in
    /// ascending order — empty in practice. Consumers translating tape
    /// ranges into primitive ranges subtract the drops before each
    /// boundary.
    pub dropped: Vec<u32>,
}

#[derive(Default)]
pub struct DrawScopeDefault {
    size: Size,
    /// The compact recording every draw call writes into; materialized into
    /// `out` once, when the scope finishes.
    rec: CommandRecording,
    /// Materialization target, owned by the consumer across frames (see
    /// [`Self::with_recording`]); untouched until [`Self::finish`].
    out: Vec<DrawPrimitive>,
    /// How many [`DrawPrimitive::Content`] markers this scope has recorded.
    /// Consumers splitting a command around its content would otherwise have
    /// to re-scan thousands of just-recorded primitives to learn "none".
    content_markers: u32,
    /// `None` falls back to [`estimate_text_measurement`]. Every scope the
    /// framework builds carries the app's real measurer; a hand-built one
    /// (tests, tooling) does not have to.
    text_measurer: Option<Rc<dyn DrawTextMeasurer>>,
}

/// Per-thread memory of how many primitives the scope of a given size emitted
/// last time, keyed by the scope's size bits. Draw closures re-record every
/// frame, and a heavy animated canvas emits thousands of primitives; starting
/// its vector at the previous count skips the whole doubling schedule of
/// reallocations. Keying by size keeps a small HUD scope from inheriting the
/// arena's multi-thousand capacity.
const RECORDED_PRIMITIVE_COUNTS_LIMIT: usize = 64;

thread_local! {
    static RECORDED_PRIMITIVE_COUNTS: std::cell::RefCell<std::collections::HashMap<(u32, u32), usize>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

fn recorded_primitive_capacity(size: Size) -> usize {
    RECORDED_PRIMITIVE_COUNTS.with(|counts| {
        counts
            .borrow()
            .get(&(size.width.to_bits(), size.height.to_bits()))
            .copied()
            .unwrap_or(0)
    })
}

fn note_recorded_primitive_count(size: Size, count: usize) {
    RECORDED_PRIMITIVE_COUNTS.with(|counts| {
        let mut counts = counts.borrow_mut();
        if counts.len() >= RECORDED_PRIMITIVE_COUNTS_LIMIT {
            counts.clear();
        }
        counts.insert((size.width.to_bits(), size.height.to_bits()), count);
    });
}

impl DrawScopeDefault {
    pub fn new(size: Size) -> Self {
        Self::with_recording(size, None, CommandRecording::default(), Vec::new())
    }

    /// A scope that measures text with the app's fonts.
    ///
    /// The framework calls this for every draw closure it runs; `new` exists
    /// for callers that never draw text.
    pub fn with_text_measurer(size: Size, text_measurer: Rc<dyn DrawTextMeasurer>) -> Self {
        Self::with_recording(
            size,
            Some(text_measurer),
            CommandRecording::default(),
            Vec::new(),
        )
    }

    /// Like [`with_text_measurer`](Self::with_text_measurer), but records
    /// into storage the caller already owns. This is the retained-recording
    /// path: a command that re-records every frame keeps one buffer whose
    /// capacity was earned on earlier frames, instead of walking a fresh
    /// vector through the whole doubling schedule again.
    pub fn with_text_measurer_reusing(
        size: Size,
        text_measurer: Rc<dyn DrawTextMeasurer>,
        storage: Vec<DrawPrimitive>,
    ) -> Self {
        Self::with_recording(
            size,
            Some(text_measurer),
            CommandRecording::default(),
            storage,
        )
    }

    /// The full retained-recording form: compact recording buffers AND the
    /// materialization target both come from the caller, so a command that
    /// re-records every frame allocates nothing in the steady state.
    pub fn with_recording(
        size: Size,
        text_measurer: Option<Rc<dyn DrawTextMeasurer>>,
        mut recording: CommandRecording,
        out: Vec<DrawPrimitive>,
    ) -> Self {
        recording.clear();
        recording.tape.reserve(recorded_primitive_capacity(size));
        Self {
            size,
            rec: recording,
            out,
            content_markers: 0,
            text_measurer,
        }
    }

    /// How many [`DrawPrimitive::Content`] markers this scope has recorded.
    /// Command consumers split placements around the count instead of
    /// re-scanning thousands of just-recorded primitives to learn "none".
    pub fn content_marker_count(&self) -> u32 {
        self.content_markers
    }

    /// The compact recording as recorded so far. Retention verification
    /// reads this before materialization decides what to skip.
    pub fn recorded(&self) -> &CommandRecording {
        &self.rec
    }

    /// Appends already-recorded primitives verbatim. This is the replay path
    /// for pre-built primitive lists (deferred modifier draws recorded
    /// earlier, synthesized commands in tests); the marker count stays
    /// authoritative because the batch is scanned once on the way in.
    pub fn push_recorded(&mut self, primitives: Vec<DrawPrimitive>) {
        self.content_markers += primitives
            .iter()
            .filter(|primitive| matches!(primitive, DrawPrimitive::Content))
            .count() as u32;
        let base = self.rec.others.len();
        self.rec
            .tape
            .extend((0..primitives.len()).map(|i| TapeRef::new(RecordKind::Other, base + i)));
        self.rec.others.extend(primitives);
    }

    /// Materializes the recording into the `out` storage and hands both
    /// back, plus the compact buffers for the caller to retain. This — not
    /// recording — is where solid records become `DrawPrimitive`s and where
    /// arc bands, tight bounds, and the degeneracy drop happen, so the
    /// output is exactly what recording used to produce directly.
    pub fn finish(mut self) -> FinishedRecording {
        // Composition diagnostic for retention work: how much of a heavy
        // command is typed records vs ordinary primitives.
        if std::env::var_os("CRANPOSE_RECORD_MIX_DIAG").is_some() && self.rec.tape.len() > 400 {
            eprintln!(
                "[record-mix] tape={} rects={} round_rects={} arcs={} others={}",
                self.rec.tape.len(),
                self.rec.rects.len(),
                self.rec.round_rects.len(),
                self.rec.arcs.len(),
                self.rec.others.len(),
            );
        }
        let mut out = std::mem::take(&mut self.out);
        out.clear();
        out.reserve(self.rec.tape.len());
        let mut dropped: Vec<u32> = Vec::new();
        {
            // `others` moves out via drain; the increasing-index invariant
            // on the tape makes tape order equal drain order. Copy stores
            // are addressed directly by the entry's index.
            let mut others = self.rec.others.drain(..);
            for (tape_index, entry) in self.rec.tape.iter().enumerate() {
                match entry.kind() {
                    RecordKind::SolidRect => {
                        let record = &self.rec.rects[entry.index()];
                        out.push(DrawPrimitive::Rect {
                            rect: record.rect,
                            brush: Brush::Solid(record.color),
                            stroke: record.stroke,
                        });
                    }
                    RecordKind::SolidRoundRect => {
                        let record = &self.rec.round_rects[entry.index()];
                        out.push(DrawPrimitive::RoundRect {
                            rect: record.rect,
                            brush: Brush::Solid(record.color),
                            radii: record.radii,
                            stroke: record.stroke,
                        });
                    }
                    RecordKind::SolidArc => {
                        let record = &self.rec.arcs[entry.index()];
                        if let Some(primitive) = materialize_solid_arc(record) {
                            out.push(primitive);
                        } else {
                            dropped.push(tape_index as u32);
                        }
                    }
                    RecordKind::Other => {
                        out.push(others.next().expect("tape/others in sync"));
                    }
                }
            }
        }
        self.rec.clear();
        note_recorded_primitive_count(self.size, out.len());
        FinishedRecording {
            primitives: out,
            content_markers: self.content_markers,
            recording: self.rec,
            dropped,
        }
    }

    /// Like [`Self::finish`], but materializing nothing: the consumer is
    /// about to re-emit a PREVIOUS frame's saved emission in place of this
    /// recording (the stale-transition serve on a replay collapse frame),
    /// so building this frame's primitives — the very cost the serve
    /// exists to skip — would be pure waste. The recording and
    /// materialization buffers still return, cleared exactly as
    /// [`Self::finish`] leaves them, so the command's steady-state
    /// ping-pong keeps its earned capacity.
    pub fn finish_recording_only(mut self) -> FinishedRecording {
        let mut out = std::mem::take(&mut self.out);
        out.clear();
        note_recorded_primitive_count(self.size, self.rec.tape.len());
        self.rec.clear();
        FinishedRecording {
            primitives: out,
            content_markers: self.content_markers,
            recording: self.rec,
            dropped: Vec::new(),
        }
    }

    /// Like [`Self::finish`], but for a verified command: a retained span
    /// whose slot `bypass` approves is NOT materialized — its records cease
    /// to exist as per-frame primitives, which is the entire point of
    /// retention — and the replay frame comes back with primitive ranges
    /// assigned during the same single tape walk. Every other span
    /// materializes exactly as [`Self::finish`] would. `AllDynamic`
    /// degenerates to plain `finish`.
    pub fn finish_replay(
        mut self,
        center: Point,
        outcome: crate::record_replay::ReplayOutcome,
        bypass: &mut dyn FnMut(u32) -> bool,
    ) -> (
        FinishedRecording,
        Option<crate::record_replay::CommandReplayFrame>,
    ) {
        use crate::record_replay::{CommandReplayFrame, FrameSpan, ReplayOutcome, ReplaySpan};
        let ReplayOutcome::Spans(replay_spans) = outcome else {
            return (self.finish(), None);
        };
        let tape_len = self.rec.tape.len();
        let mut out = std::mem::take(&mut self.out);
        out.clear();
        let mut dropped: Vec<u32> = Vec::new();
        let mut spans: Vec<FrameSpan> = Vec::with_capacity(replay_spans.len());
        let mut any_retained = false;
        {
            // `others` moves out via drain (tape order equals drain order by
            // the increasing-index invariant); Copy stores are addressed
            // directly by each entry's index, so a bypassed span costs
            // nothing to step past.
            let mut others = self.rec.others.drain(..);
            let tape = &self.rec.tape;
            let rects = &self.rec.rects;
            let round_rects = &self.rec.round_rects;
            let arcs = &self.rec.arcs;
            // Materializes one contiguous tape range into `out`. Kept as a
            // macro so `out`, `dropped`, and the `others` cursor stay plain
            // locals the borrow checker can split by field.
            macro_rules! materialize_range {
                ($start:expr, $end:expr) => {{
                    let prim_start = out.len() as u32;
                    for tape_index in $start..$end {
                        let entry = tape[tape_index];
                        match entry.kind() {
                            RecordKind::SolidRect => {
                                let record = &rects[entry.index()];
                                out.push(DrawPrimitive::Rect {
                                    rect: record.rect,
                                    brush: Brush::Solid(record.color),
                                    stroke: record.stroke,
                                });
                            }
                            RecordKind::SolidRoundRect => {
                                let record = &round_rects[entry.index()];
                                out.push(DrawPrimitive::RoundRect {
                                    rect: record.rect,
                                    brush: Brush::Solid(record.color),
                                    radii: record.radii,
                                    stroke: record.stroke,
                                });
                            }
                            RecordKind::SolidArc => {
                                let record = &arcs[entry.index()];
                                if let Some(primitive) = materialize_solid_arc(record) {
                                    out.push(primitive);
                                } else {
                                    dropped.push(tape_index as u32);
                                }
                            }
                            RecordKind::Other => {
                                out.push(others.next().expect("tape/others in sync"));
                            }
                        }
                    }
                    (prim_start, out.len() as u32)
                }};
            }
            for span in replay_spans {
                match span {
                    ReplaySpan::Dynamic {
                        tape_start,
                        tape_end,
                    } => {
                        let range = materialize_range!(tape_start, tape_end);
                        if range.1 > range.0 {
                            spans.push(FrameSpan::Dynamic { range });
                        }
                    }
                    ReplaySpan::Retained {
                        slot,
                        capture,
                        slot_offset,
                        tape_start,
                        tape_end,
                        transform,
                        recolors,
                        bounds,
                    } => {
                        // A retained span holds solid arcs and circles by
                        // construction; anything else in its range means
                        // the ordinary path must draw it.
                        let compact = tape[tape_start..tape_end]
                            .iter()
                            .all(|entry| entry.kind() != RecordKind::Other);
                        if compact && !capture && bypass(slot) {
                            // The bypass: the records are never materialized
                            // — direct indexing leaves nothing to advance.
                            // Capture guaranteed each record one clean
                            // shape, so no drop tracking is needed on the
                            // way past.
                            any_retained = true;
                            let position = out.len() as u32;
                            spans.push(FrameSpan::Retained {
                                slot,
                                capture: false,
                                slot_offset: slot_offset as u32,
                                range: (position, position),
                                tape_range: (tape_start as u32, tape_end as u32),
                                transform,
                                recolors,
                                bounds,
                            });
                            continue;
                        }
                        let drops_before = dropped.len();
                        let range = materialize_range!(tape_start, tape_end);
                        if compact && dropped.len() == drops_before {
                            any_retained = true;
                            spans.push(FrameSpan::Retained {
                                slot,
                                capture,
                                slot_offset: slot_offset as u32,
                                range,
                                tape_range: (tape_start as u32, tape_end as u32),
                                transform,
                                recolors,
                                bounds,
                            });
                        } else if range.1 > range.0 {
                            spans.push(FrameSpan::Dynamic { range });
                        }
                    }
                }
            }
        }
        // Deliberately NOT cleared: the typed stores must survive until the
        // renderer has drawn this frame, so a bypassed span that cannot be
        // drawn retained (context drift, op cap) can still be materialized
        // on demand from the recording — which the consumer publishes and
        // pins to the frame as its owned `fallback`. The next recording's
        // scope clears the buffers on construction anyway.
        note_recorded_primitive_count(self.size, tape_len);
        // `fallback` is attached by the consumer once the recording is
        // published under its shared handle — the recording is still owned
        // by value here.
        let frame = any_retained.then_some(CommandReplayFrame {
            center,
            spans,
            fallback: None,
        });
        (
            FinishedRecording {
                primitives: out,
                content_markers: self.content_markers,
                recording: self.rec,
                dropped,
            },
            frame,
        )
    }

    fn push_other(&mut self, primitive: DrawPrimitive) {
        let idx = self.rec.others.len();
        self.rec.tape.push(TapeRef::new(RecordKind::Other, idx));
        self.rec.others.push(primitive);
    }

    fn push_blended_primitive(&mut self, primitive: DrawPrimitive, blend_mode: BlendMode) {
        if blend_mode != BlendMode::SrcOver {
            self.push_other(DrawPrimitive::Blend {
                primitive: Box::new(primitive),
                blend_mode,
            });
            return;
        }
        match primitive {
            DrawPrimitive::Rect {
                rect,
                brush: Brush::Solid(color),
                stroke,
            } => {
                let idx = self.rec.rects.len();
                self.rec.tape.push(TapeRef::new(RecordKind::SolidRect, idx));
                self.rec.rects.push(SolidRectRecord {
                    rect,
                    color,
                    stroke,
                });
            }
            DrawPrimitive::RoundRect {
                rect,
                brush: Brush::Solid(color),
                radii,
                stroke,
            } => {
                let idx = self.rec.round_rects.len();
                self.rec
                    .tape
                    .push(TapeRef::new(RecordKind::SolidRoundRect, idx));
                self.rec.round_rects.push(SolidRoundRectRecord {
                    rect,
                    radii,
                    color,
                    stroke,
                });
            }
            other => self.push_other(other),
        }
    }

    /// Shared lowering for [`DrawScope::draw_arc`] and
    /// [`DrawScope::draw_annular_sector`].
    ///
    /// Resolves the band, computes the *tight* bounding box (caps included) and
    /// drops degenerate geometry on the floor instead of emitting NaN-bearing
    /// primitives the renderers would have to defend against.
    #[allow(clippy::too_many_arguments)]
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
        // The common case records raw parameters only; band resolution,
        // tight bounds, and the degeneracy drop run at materialization
        // (see [`materialize_solid_arc`]), producing identical output.
        if blend_mode == BlendMode::SrcOver {
            if let Brush::Solid(color) = brush {
                let idx = self.rec.arcs.len();
                self.rec.tape.push(TapeRef::new(RecordKind::SolidArc, idx));
                self.rec.arcs.push(SolidArcRecord {
                    center,
                    radius,
                    start_angle,
                    sweep_angle,
                    inner_radius,
                    color,
                    stroke,
                });
                return;
            }
        }
        let (band_inner, band_outer, cap) = arc_band(radius, inner_radius, stroke);
        let geometry = ArcGeometry::new(
            center,
            band_inner,
            band_outer,
            start_angle,
            sweep_angle,
            cap,
        );
        if geometry.is_degenerate() {
            return;
        }
        self.push_blended_primitive(
            DrawPrimitive::Arc {
                rect: geometry.bounds(),
                brush,
                center,
                radius,
                start_angle,
                sweep_angle,
                stroke,
                inner_radius,
            },
            blend_mode,
        );
    }
}

/// The deferred half of the solid-arc fast path: exactly the lowering
/// [`DrawScopeDefault::push_arc`] applies to every other arc, run when the
/// recording materializes instead of when the app draws. `None` is the
/// degenerate drop.
fn materialize_solid_arc(record: &SolidArcRecord) -> Option<DrawPrimitive> {
    let (band_inner, band_outer, cap) = arc_band(record.radius, record.inner_radius, record.stroke);
    let geometry = ArcGeometry::new(
        record.center,
        band_inner,
        band_outer,
        record.start_angle,
        record.sweep_angle,
        cap,
    );
    if geometry.is_degenerate() {
        return None;
    }
    Some(DrawPrimitive::Arc {
        rect: geometry.bounds(),
        brush: Brush::Solid(record.color),
        center: record.center,
        radius: record.radius,
        start_angle: record.start_angle,
        sweep_angle: record.sweep_angle,
        stroke: record.stroke,
        inner_radius: record.inner_radius,
    })
}

impl DrawScope for DrawScopeDefault {
    fn size(&self) -> Size {
        self.size
    }

    fn draw_content(&mut self) {
        self.content_markers += 1;
        self.push_other(DrawPrimitive::Content);
    }

    fn draw_rect(&mut self, brush: Brush) {
        self.draw_rect_blend(brush, BlendMode::SrcOver);
    }

    fn draw_rect_blend(&mut self, brush: Brush, blend_mode: BlendMode) {
        self.push_blended_primitive(
            DrawPrimitive::Rect {
                rect: Rect::from_size(self.size),
                brush,
                stroke: None,
            },
            blend_mode,
        );
    }

    fn draw_rect_at(&mut self, rect: Rect, brush: Brush) {
        self.draw_rect_at_blend(rect, brush, BlendMode::SrcOver);
    }

    fn draw_rect_at_blend(&mut self, rect: Rect, brush: Brush, blend_mode: BlendMode) {
        self.push_blended_primitive(
            DrawPrimitive::Rect {
                rect,
                brush,
                stroke: None,
            },
            blend_mode,
        );
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
                stroke: None,
            },
            blend_mode,
        );
    }

    fn draw_round_rect_at(&mut self, rect: Rect, brush: Brush, radii: CornerRadii) {
        self.push_blended_primitive(
            DrawPrimitive::RoundRect {
                rect,
                brush,
                radii,
                stroke: None,
            },
            BlendMode::SrcOver,
        );
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
        self.push_blended_primitive(
            DrawPrimitive::Rect {
                rect,
                brush,
                stroke: Some(stroke),
            },
            blend_mode,
        );
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
        self.push_blended_primitive(
            DrawPrimitive::RoundRect {
                rect,
                brush,
                radii,
                stroke: Some(stroke),
            },
            blend_mode,
        );
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
        self.push_blended_primitive(
            DrawPrimitive::RoundRect {
                rect: Rect {
                    x: center.x - radius,
                    y: center.y - radius,
                    width: diameter,
                    height: diameter,
                },
                brush,
                radii: CornerRadii::uniform(radius),
                stroke: None,
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
        /// Rasterization supersampling factor relative to scope units.
        /// Combined with the rasterizer's own sub-scanline anti-aliasing
        /// and linear image sampling, this keeps icon edges crisp on
        /// high-density screens.
        const SUPERSAMPLE: f32 = 2.0;
        /// Safety cap for the rasterized mask dimensions.
        const MAX_MASK_PIXELS: f32 = 4096.0;

        if path.is_empty() {
            return;
        }
        let bounds = path.bounds();
        if bounds.width <= 0.0 || bounds.height <= 0.0 {
            return;
        }

        let color = match &brush {
            Brush::Solid(color) => *color,
            Brush::LinearGradient { colors, .. }
            | Brush::RadialGradient { colors, .. }
            | Brush::SweepGradient { colors, .. } => match colors.first() {
                Some(color) => *color,
                None => return,
            },
        };
        if color.3 <= 0.0 {
            return;
        }

        // Rasterize a padded, integer-aligned bounding box so anti-aliased
        // edges are never clipped by the mask border.
        let origin = Point::new(bounds.x.floor() - 1.0, bounds.y.floor() - 1.0);
        let rect_width = (bounds.x + bounds.width).ceil() - origin.x + 1.0;
        let rect_height = (bounds.y + bounds.height).ceil() - origin.y + 1.0;
        let mask_width = (rect_width * SUPERSAMPLE)
            .ceil()
            .clamp(1.0, MAX_MASK_PIXELS) as usize;
        let mask_height = (rect_height * SUPERSAMPLE)
            .ceil()
            .clamp(1.0, MAX_MASK_PIXELS) as usize;

        let red = (color.0.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        let green = (color.1.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        let blue = (color.2.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        let alpha = color.3.clamp(0.0, 1.0);
        let key = vector_path_mask_key(
            path,
            origin,
            (mask_width, mask_height),
            [red, green, blue],
            alpha,
        );
        let cached = vector_path_mask_cache_get(key);
        let image = match cached {
            Some(image) => image,
            None => {
                let mask = path.coverage_mask(mask_width, mask_height, origin, SUPERSAMPLE);
                let mut pixels = Vec::with_capacity(mask.len() * 4);
                for coverage in mask {
                    pixels.extend_from_slice(&[
                        red,
                        green,
                        blue,
                        (alpha * coverage as f32 + 0.5) as u8,
                    ]);
                }
                let Ok(image) =
                    ImageBitmap::from_rgba8(mask_width as u32, mask_height as u32, pixels)
                else {
                    return;
                };
                vector_path_mask_cache_put(key, image.clone());
                image
            }
        };

        self.push_other(DrawPrimitive::Image {
            rect: Rect {
                x: origin.x,
                y: origin.y,
                width: rect_width,
                height: rect_height,
            },
            image,
            alpha: 1.0,
            color_filter: None,
            sampling: ImageSampling::Linear,
            src_rect: None,
        });
    }

    fn measure_text(&self, text: &str, style: &TextStyle) -> TextMeasurement {
        match &self.text_measurer {
            Some(measurer) => measurer.measure_text(text, style),
            None => estimate_text_measurement(text, style),
        }
    }

    fn draw_text_at(&mut self, rect: Rect, brush: Brush, text: &str, style: &TextStyle) {
        // An empty run has no glyphs and a zero-area box, which every renderer
        // would drop anyway — stop here so it never reaches the scene.
        if text.is_empty() {
            return;
        }
        let Some(color) = solid_fill_color(&brush) else {
            return;
        };
        if color.3 <= 0.0 {
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
        self.push_other(DrawPrimitive::Text(Box::new(TextPrimitive {
            rect: Rect::from_origin_size(origin, measurement.size),
            text: shared_text_str(text),
            style: style.clone(),
            color,
        })));
    }

    fn into_primitives(self) -> Vec<DrawPrimitive> {
        self.finish().primitives
    }
}

/// The single color a brush paints with, or its first stop for a gradient.
///
/// Text is filled per glyph from one vertex color, so a gradient cannot be
/// honored; this mirrors the fallback [`DrawScope::draw_vector_path`] documents.
fn solid_fill_color(brush: &Brush) -> Option<Color> {
    match brush {
        Brush::Solid(color) => Some(*color),
        Brush::LinearGradient { colors, .. }
        | Brush::RadialGradient { colors, .. }
        | Brush::SweepGradient { colors, .. } => colors.first().copied(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, FontStyle, FontWeight, ImageBitmap, RenderEffect};

    /// The compact recorder routes solid `SrcOver` shapes through typed
    /// records and everything else through ordinary primitives; the tape
    /// must reassemble the exact sequence recording used to produce
    /// directly, arc lowering and degeneracy drops included.
    #[test]
    fn compact_recording_materializes_in_recorded_order() {
        let size = Size::new(100.0, 100.0);
        let solid = Brush::solid(Color::WHITE);
        let gradient = Brush::vertical_gradient(vec![Color::RED, Color::BLUE], 0.0, 100.0);
        let center = Point::new(50.0, 50.0);
        let stroke = Stroke::new(4.0);
        let rect = Rect {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0,
        };
        let batch = vec![
            DrawPrimitive::Content,
            DrawPrimitive::Rect {
                rect,
                brush: solid.clone(),
                stroke: None,
            },
        ];

        // Interleave every routing path.
        let record = |scope: &mut DrawScopeDefault| {
            scope.draw_rect_at(rect, solid.clone());
            scope.draw_arc(solid.clone(), center, 30.0, 0.5, 1.5, stroke);
            scope.draw_rect_at(rect, gradient.clone());
            scope.draw_circle(solid.clone(), center, 12.0);
            scope.draw_arc(solid.clone(), center, 30.0, 0.5, 0.0, stroke); // degenerate: dropped
            scope.draw_rect_at_blend(rect, solid.clone(), BlendMode::Plus);
            scope.draw_content();
            scope.draw_annular_sector(gradient.clone(), center, 10.0, 20.0, 0.0, 2.0);
            scope.push_recorded(batch.clone());
        };

        let mut compact = DrawScopeDefault::new(size);
        record(&mut compact);
        let finished = compact.finish();

        // The expected sequence, built through the primitives the ordinary
        // lowering produces (the non-solid arc still takes that path, so it
        // serves as its own reference for the solid one's geometry).
        let arc_via_ordinary = |brush: Brush, radius: f32, start: f32, sweep: f32| {
            let mut scope = DrawScopeDefault::new(size);
            scope.draw_arc(brush, center, radius, start, sweep, stroke);
            scope.into_primitives().remove(0)
        };
        let expected = vec![
            DrawPrimitive::Rect {
                rect,
                brush: solid.clone(),
                stroke: None,
            },
            arc_via_ordinary(solid.clone(), 30.0, 0.5, 1.5),
            DrawPrimitive::Rect {
                rect,
                brush: gradient.clone(),
                stroke: None,
            },
            DrawPrimitive::RoundRect {
                rect: Rect {
                    x: center.x - 12.0,
                    y: center.y - 12.0,
                    width: 24.0,
                    height: 24.0,
                },
                brush: solid.clone(),
                radii: CornerRadii::uniform(12.0),
                stroke: None,
            },
            DrawPrimitive::Blend {
                primitive: Box::new(DrawPrimitive::Rect {
                    rect,
                    brush: solid.clone(),
                    stroke: None,
                }),
                blend_mode: BlendMode::Plus,
            },
            DrawPrimitive::Content,
            {
                let mut scope = DrawScopeDefault::new(size);
                scope.draw_annular_sector(gradient.clone(), center, 10.0, 20.0, 0.0, 2.0);
                scope.into_primitives().remove(0)
            },
            DrawPrimitive::Content,
            DrawPrimitive::Rect {
                rect,
                brush: solid.clone(),
                stroke: None,
            },
        ];
        assert_eq!(finished.primitives, expected);
        assert_eq!(finished.content_markers, 2);
    }

    /// A recording that reuses another command's buffers (junk capacity in
    /// every store) must be byte-identical to one recorded fresh.
    #[test]
    fn reused_recording_buffers_record_identically_to_fresh() {
        let size = Size::new(64.0, 64.0);
        let record = |scope: &mut DrawScopeDefault| {
            scope.draw_circle(Brush::solid(Color::RED), Point::new(32.0, 32.0), 10.0);
            scope.draw_arc(
                Brush::solid(Color::BLUE),
                Point::new(32.0, 32.0),
                20.0,
                0.0,
                3.0,
                Stroke::new(2.0),
            );
        };

        let mut fresh = DrawScopeDefault::new(size);
        record(&mut fresh);
        let fresh = fresh.finish();

        // Dirty the buffers with an unrelated recording first.
        let mut dirty = DrawScopeDefault::new(size);
        dirty.draw_rect(Brush::solid(Color::BLACK));
        dirty.draw_content();
        dirty.draw_arc(
            Brush::solid(Color::WHITE),
            Point::new(1.0, 1.0),
            5.0,
            1.0,
            1.0,
            Stroke::new(1.0),
        );
        let dirty = dirty.finish();

        let mut reused =
            DrawScopeDefault::with_recording(size, None, dirty.recording, dirty.primitives);
        record(&mut reused);
        let reused = reused.finish();

        assert_eq!(fresh.primitives, reused.primitives);
        assert_eq!(fresh.content_markers, reused.content_markers);
    }

    #[test]
    fn redrawing_the_same_text_shares_one_str_allocation() {
        let first = shared_text_str("BREAK THE RING");
        let second = shared_text_str("BREAK THE RING");
        assert!(Rc::ptr_eq(&first, &second));
        assert_eq!(&*second, "BREAK THE RING");
    }

    #[test]
    fn different_text_gets_its_own_str() {
        let first = shared_text_str("340");
        let second = shared_text_str("350");
        assert!(!Rc::ptr_eq(&first, &second));
        assert_eq!(&*first, "340");
        assert_eq!(&*second, "350");
    }

    #[test]
    fn the_text_pool_survives_overflowing_its_capacity() {
        for index in 0..600 {
            let text = format!("run-{index}");
            assert_eq!(&*shared_text_str(&text), text.as_str());
        }
        assert_eq!(&*shared_text_str("still correct"), "still correct");
    }

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
    fn draw_svg_path_emits_supersampled_image_over_path_bounds() {
        let mut scope = DrawScopeDefault::new(Size::new(32.0, 32.0));
        scope.draw_svg_path("M 4 4 H 20 V 20 H 4 Z", Brush::solid(Color::RED));

        let primitives = scope.into_primitives();
        assert_eq!(primitives.len(), 1);
        let DrawPrimitive::Image { rect, image, .. } = &primitives[0] else {
            panic!("expected image primitive, got {:?}", primitives[0]);
        };

        // Padded, integer-aligned bounds: (3,3) to (21,21).
        assert_eq!((rect.x, rect.y), (3.0, 3.0));
        assert_eq!((rect.width, rect.height), (18.0, 18.0));
        // Rasterized at 2x supersampling.
        assert_eq!((image.width(), image.height()), (36, 36));

        // Probe the pixel at path point (12, 12): mask position
        // ((12 - 3) * 2, (12 - 3) * 2) = (18, 18) — fully covered red.
        let pixels = image.pixels();
        let index = (18 * 36 + 18) * 4;
        assert_eq!(
            &pixels[index..index + 4],
            &[255, 0, 0, 255],
            "path interior must be opaque brush color"
        );
        // A corner outside the square must be transparent.
        assert_eq!(pixels[3], 0, "outside the path must stay transparent");
    }

    #[test]
    fn draw_svg_path_ignores_invalid_data() {
        let mut scope = DrawScopeDefault::new(Size::new(16.0, 16.0));
        scope.draw_svg_path("definitely not a path", Brush::solid(Color::WHITE));
        assert!(scope.into_primitives().is_empty());
    }

    #[test]
    fn draw_vector_path_applies_brush_alpha() {
        let path = crate::VectorPath::parse("M 0 0 H 8 V 8 H 0 Z").expect("valid path");
        let mut scope = DrawScopeDefault::new(Size::new(16.0, 16.0));
        scope.draw_vector_path(&path, Brush::solid(Color::rgba(0.0, 0.0, 1.0, 0.5)));

        let primitives = scope.into_primitives();
        let DrawPrimitive::Image { image, .. } = &primitives[0] else {
            panic!("expected image primitive");
        };
        let pixels = image.pixels();
        // Center of the mask: interior pixel with half-alpha blue.
        let width = image.width() as usize;
        let index = ((image.height() as usize / 2) * width + width / 2) * 4;
        assert_eq!(&pixels[index..index + 3], &[0, 0, 255]);
        let alpha = pixels[index + 3];
        assert!(
            (alpha as i32 - 128).abs() <= 2,
            "interior alpha must honor the brush alpha, got {alpha}"
        );
    }

    #[test]
    fn the_same_path_and_color_reuse_one_raster() {
        let path = crate::VectorPath::parse("M 0 0 H 7 V 7 H 0 Z").expect("valid path");
        let raster_of = |brush: Brush| {
            let mut scope = DrawScopeDefault::new(Size::new(16.0, 16.0));
            scope.draw_vector_path(&path, brush);
            let primitives = scope.into_primitives();
            let DrawPrimitive::Image { image, .. } = &primitives[0] else {
                panic!("expected image primitive");
            };
            image.clone()
        };

        let first = raster_of(Brush::solid(Color::rgba(0.0, 0.0, 1.0, 1.0)));
        let second = raster_of(Brush::solid(Color::rgba(0.0, 0.0, 1.0, 1.0)));
        assert_eq!(first.id(), second.id());

        let other_color = raster_of(Brush::solid(Color::rgba(1.0, 0.0, 0.0, 1.0)));
        assert_ne!(first.id(), other_color.id());

        let wider = crate::VectorPath::parse("M 0 0 H 9 V 7 H 0 Z").expect("valid path");
        let mut scope = DrawScopeDefault::new(Size::new(16.0, 16.0));
        scope.draw_vector_path(&wider, Brush::solid(Color::rgba(0.0, 0.0, 1.0, 1.0)));
        let primitives = scope.into_primitives();
        let DrawPrimitive::Image { image, .. } = &primitives[0] else {
            panic!("expected image primitive");
        };
        assert_ne!(first.id(), image.id());
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

    /// The retained-recording path exists so a command that re-records every
    /// frame keeps the buffers it already grew. Handing storage in and getting
    /// it back has to record exactly what a fresh scope would, and the count of
    /// content markers has to come back without re-scanning the primitives.
    #[test]
    fn a_reused_recording_buffer_records_what_a_fresh_one_would() {
        let size = Size::new(16.0, 16.0);
        let measurer: Rc<dyn DrawTextMeasurer> = FixedAdvanceTextMeasurer::shared(10.0, 20.0);

        let draw = |scope: &mut DrawScopeDefault| {
            scope.draw_rect(Brush::solid(Color::WHITE));
            scope.draw_content();
            scope.draw_rect(Brush::solid(Color::BLACK));
            scope.draw_content();
        };

        let mut fresh = DrawScopeDefault::with_text_measurer(size, Rc::clone(&measurer));
        draw(&mut fresh);
        assert_eq!(fresh.content_marker_count(), 2);
        let expected = fresh.finish();

        // Storage the caller already owns, carrying capacity from an earlier
        // frame. What comes out has to be the same recording.
        let storage = Vec::with_capacity(64);
        let mut reused = DrawScopeDefault::with_text_measurer_reusing(size, measurer, storage);
        draw(&mut reused);
        assert_eq!(reused.content_marker_count(), 2);
        let reused = reused.finish();

        assert_eq!(reused.primitives.len(), expected.primitives.len());
        assert_eq!(reused.content_markers, expected.content_markers);
        assert_eq!(reused.dropped, expected.dropped);
    }

    /// A frame that is about to serve a previous frame's primitives should not
    /// pay to build this frame's. Finishing recording-only returns the buffers
    /// cleared, and the marker count, without materializing anything.
    #[test]
    fn finishing_recording_only_materializes_nothing_but_still_reports_markers() {
        let mut scope = DrawScopeDefault::new(Size::new(8.0, 8.0));
        scope.draw_rect(Brush::solid(Color::WHITE));
        scope.draw_content();
        assert_eq!(scope.content_marker_count(), 1);

        let finished = scope.finish_recording_only();
        assert!(
            finished.primitives.is_empty(),
            "nothing should have been materialized"
        );
        assert_eq!(finished.content_markers, 1);
        assert!(finished.dropped.is_empty());
        assert_eq!(
            finished.recording.tape.len(),
            0,
            "the recording buffer comes back cleared, ready to be recorded into again"
        );
    }

    /// Where a block of text lands inside the rect it was given. Split out of
    /// the draw so the rule is stated once; a wrong vertical rule is what makes
    /// baseline-aligned text sit a line too low.
    #[test]
    fn a_text_block_is_placed_by_its_alignment_inside_the_rect() {
        let rect = Rect {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 40.0,
        };
        let measurement = TextMeasurement {
            size: Size::new(60.0, 16.0),
            line_height: 16.0,
            first_baseline: 12.0,
            line_count: 1,
        };
        let style = |align, vertical| {
            TextStyle::default()
                .with_align(align)
                .with_vertical_align(vertical)
        };

        let left = align_text_block(
            rect,
            measurement,
            &style(TextAlign::Left, TextVerticalAlign::Top),
        );
        assert_eq!(left, Point::new(10.0, 20.0));

        let centered = align_text_block(
            rect,
            measurement,
            &style(TextAlign::Center, TextVerticalAlign::Center),
        );
        assert_eq!(centered, Point::new(10.0 + 20.0, 20.0 + 12.0));

        let right = align_text_block(
            rect,
            measurement,
            &style(TextAlign::Right, TextVerticalAlign::Bottom),
        );
        assert_eq!(right, Point::new(50.0, 44.0));

        // Baseline alignment places the baseline on the rect's top edge, which
        // is what lets a caller line text up with something else.
        let baseline = align_text_block(
            rect,
            measurement,
            &style(TextAlign::Left, TextVerticalAlign::Baseline),
        );
        assert_eq!(baseline, Point::new(10.0, 20.0 - 12.0));
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
    fn draw_circle_records_centered_round_rect() {
        let mut scope = DrawScopeDefault::new(Size::new(40.0, 40.0));
        scope.draw_circle(Brush::solid(Color::BLUE), Point::new(12.0, 16.0), 5.0);

        let primitives = scope.into_primitives();
        assert_eq!(primitives.len(), 1);
        match &primitives[0] {
            DrawPrimitive::RoundRect { rect, radii, .. } => {
                assert_eq!(
                    *rect,
                    Rect {
                        x: 7.0,
                        y: 11.0,
                        width: 10.0,
                        height: 10.0,
                    }
                );
                assert_eq!(*radii, CornerRadii::uniform(5.0));
            }
            other => panic!("expected circular round rect, got {other:?}"),
        }
    }

    #[test]
    fn draw_circle_blend_wraps_non_default_modes() {
        let mut scope = DrawScopeDefault::new(Size::new(10.0, 10.0));
        scope.draw_circle_blend(
            Brush::solid(Color::RED),
            Point::new(5.0, 5.0),
            3.0,
            BlendMode::Plus,
        );

        let primitives = scope.into_primitives();
        assert_eq!(primitives.len(), 1);
        match &primitives[0] {
            DrawPrimitive::Blend {
                primitive,
                blend_mode,
            } => {
                assert_eq!(*blend_mode, BlendMode::Plus);
                assert!(matches!(**primitive, DrawPrimitive::RoundRect { .. }));
            }
            other => panic!("expected blended circle primitive, got {other:?}"),
        }
    }

    #[test]
    fn rect_union_encloses_both_inputs() {
        let lhs = Rect {
            x: 10.0,
            y: 5.0,
            width: 8.0,
            height: 4.0,
        };
        let rhs = Rect {
            x: 4.0,
            y: 7.0,
            width: 10.0,
            height: 6.0,
        };

        assert_eq!(
            lhs.union(rhs),
            Rect {
                x: 4.0,
                y: 5.0,
                width: 14.0,
                height: 8.0,
            }
        );
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
                sampling,
                src_rect,
            } => {
                assert_eq!(*rect, Rect::from_size(Size::new(40.0, 24.0)));
                assert_eq!(*actual, image);
                assert_eq!(*alpha, 1.0);
                assert!(color_filter.is_none());
                assert_eq!(*sampling, ImageSampling::Nearest);
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
                sampling,
                src_rect,
                ..
            } => {
                assert_eq!(*rect, dst);
                assert_eq!(*actual, image);
                assert!((alpha - 0.8).abs() < 1e-5);
                assert_eq!(*sampling, ImageSampling::Nearest);
                assert_eq!(*src_rect, Some(src));
            }
            other => panic!("expected image primitive, got {other:?}"),
        }
    }

    #[test]
    fn draw_image_at_sampled_records_requested_sampling() {
        let mut scope = DrawScopeDefault::new(Size::new(100.0, 100.0));
        let image = ImageBitmap::from_rgba8(8, 8, vec![255; 8 * 8 * 4]).expect("image");
        let dst = Rect {
            x: 2.0,
            y: 3.0,
            width: 40.0,
            height: 30.0,
        };

        scope.draw_image_at_sampled(dst, image.clone(), 0.7, None, ImageSampling::Linear);

        let primitives = scope.into_primitives();
        assert_eq!(primitives.len(), 1);
        match unwrap_image(&primitives[0]) {
            DrawPrimitive::Image {
                rect,
                image: actual,
                alpha,
                sampling,
                src_rect,
                ..
            } => {
                assert_eq!(*rect, dst);
                assert_eq!(*actual, image);
                assert!((alpha - 0.7).abs() < 1e-5);
                assert_eq!(*sampling, ImageSampling::Linear);
                assert!(src_rect.is_none());
            }
            other => panic!("expected image primitive, got {other:?}"),
        }
    }

    #[test]
    fn draw_image_src_sampled_records_requested_sampling() {
        let mut scope = DrawScopeDefault::new(Size::new(100.0, 100.0));
        let image = ImageBitmap::from_rgba8(64, 64, vec![255; 64 * 64 * 4]).expect("image");
        let src = Rect {
            x: 4.0,
            y: 6.0,
            width: 16.0,
            height: 20.0,
        };
        let dst = Rect {
            x: 8.0,
            y: 10.0,
            width: 32.0,
            height: 40.0,
        };

        scope.draw_image_src_sampled(image.clone(), src, dst, 0.5, None, ImageSampling::Linear);

        let primitives = scope.into_primitives();
        assert_eq!(primitives.len(), 1);
        match unwrap_image(&primitives[0]) {
            DrawPrimitive::Image {
                rect,
                image: actual,
                alpha,
                sampling,
                src_rect,
                ..
            } => {
                assert_eq!(*rect, dst);
                assert_eq!(*actual, image);
                assert!((alpha - 0.5).abs() < 1e-5);
                assert_eq!(*sampling, ImageSampling::Linear);
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

    // ── Stroke / arc lowering ───────────────────────────────────────────────

    use crate::{StrokeCap, StrokeJoin};
    use std::f32::consts::{FRAC_PI_2, PI};

    /// Arc bounds are conservative-approximate (fast endpoint trig plus a
    /// containment pad, see `stroke.rs`); geometry tests compare within that
    /// documented slack. The strict containment guard lives in the stroke
    /// module's property test.
    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.25
    }

    fn scope(size: f32) -> DrawScopeDefault {
        DrawScopeDefault::new(Size::new(size, size))
    }

    #[test]
    fn draw_rect_stroked_records_scope_rect_and_stroke() {
        let mut scope = scope(20.0);
        scope.draw_rect_stroked(
            Brush::solid(Color::RED),
            Stroke::new(3.0).with_join(StrokeJoin::Bevel),
        );

        let primitives = scope.into_primitives();
        assert_eq!(primitives.len(), 1);
        match &primitives[0] {
            DrawPrimitive::Rect {
                rect,
                stroke: Some(stroke),
                ..
            } => {
                // The stored rect stays the *geometric* rect; the renderer
                // inflates it by half the stroke width when it builds the quad.
                assert_eq!(*rect, Rect::from_size(Size::new(20.0, 20.0)));
                assert_eq!(stroke.width, 3.0);
                assert_eq!(stroke.join, StrokeJoin::Bevel);
            }
            other => panic!("expected stroked rect, got {other:?}"),
        }
    }

    #[test]
    fn draw_rect_at_stroked_records_requested_rect() {
        let mut scope = scope(50.0);
        let rect = Rect {
            x: 4.0,
            y: 6.0,
            width: 12.0,
            height: 9.0,
        };
        scope.draw_rect_at_stroked(rect, Brush::solid(Color::BLUE), Stroke::new(2.0));
        match &scope.into_primitives()[0] {
            DrawPrimitive::Rect {
                rect: actual,
                stroke: Some(stroke),
                ..
            } => {
                assert_eq!(*actual, rect);
                assert_eq!(stroke.width, 2.0);
            }
            other => panic!("expected stroked rect, got {other:?}"),
        }
    }

    #[test]
    fn draw_round_rect_stroked_keeps_radii_and_stroke() {
        let mut scope = scope(30.0);
        scope.draw_round_rect_stroked(
            Brush::solid(Color::GREEN),
            CornerRadii::uniform(5.0),
            Stroke::new(4.0).with_join(StrokeJoin::Round),
        );
        match &scope.into_primitives()[0] {
            DrawPrimitive::RoundRect {
                rect,
                radii,
                stroke: Some(stroke),
                ..
            } => {
                assert_eq!(*rect, Rect::from_size(Size::new(30.0, 30.0)));
                assert_eq!(*radii, CornerRadii::uniform(5.0));
                assert_eq!(stroke.width, 4.0);
                assert_eq!(stroke.join, StrokeJoin::Round);
            }
            other => panic!("expected stroked round rect, got {other:?}"),
        }
    }

    #[test]
    fn draw_round_rect_at_stroked_records_requested_rect() {
        let mut scope = scope(60.0);
        let rect = Rect {
            x: 1.0,
            y: 2.0,
            width: 20.0,
            height: 10.0,
        };
        scope.draw_round_rect_at_stroked(
            rect,
            Brush::solid(Color::WHITE),
            CornerRadii::uniform(3.0),
            Stroke::new(1.5),
        );
        match &scope.into_primitives()[0] {
            DrawPrimitive::RoundRect {
                rect: actual,
                radii,
                stroke: Some(stroke),
                ..
            } => {
                assert_eq!(*actual, rect);
                assert_eq!(*radii, CornerRadii::uniform(3.0));
                assert_eq!(stroke.width, 1.5);
            }
            other => panic!("expected stroked round rect, got {other:?}"),
        }
    }

    #[test]
    fn draw_circle_stroked_lowers_to_stroked_round_rect() {
        // A stroked circle must reuse the round-rect path so it shares the
        // shape pipeline (and therefore the batch) with every other shape.
        let mut scope = scope(40.0);
        scope.draw_circle_stroked(
            Brush::solid(Color::BLUE),
            Point::new(12.0, 16.0),
            5.0,
            Stroke::new(2.0),
        );
        match &scope.into_primitives()[0] {
            DrawPrimitive::RoundRect {
                rect,
                radii,
                stroke: Some(stroke),
                ..
            } => {
                assert_eq!(
                    *rect,
                    Rect {
                        x: 7.0,
                        y: 11.0,
                        width: 10.0,
                        height: 10.0,
                    }
                );
                assert_eq!(*radii, CornerRadii::uniform(5.0));
                assert_eq!(stroke.width, 2.0);
            }
            other => panic!("expected stroked circular round rect, got {other:?}"),
        }
    }

    #[test]
    fn draw_arc_records_arc_primitive_with_tight_bounds() {
        let mut scope = scope(200.0);
        scope.draw_arc(
            Brush::solid(Color::RED),
            Point::new(100.0, 100.0),
            50.0,
            0.0,
            FRAC_PI_2,
            Stroke::new(10.0),
        );
        let primitives = scope.into_primitives();
        assert_eq!(primitives.len(), 1);
        match &primitives[0] {
            DrawPrimitive::Arc {
                rect,
                center,
                radius,
                start_angle,
                sweep_angle,
                stroke: Some(stroke),
                inner_radius,
                ..
            } => {
                assert_eq!(*center, Point::new(100.0, 100.0));
                assert_eq!(*radius, 50.0);
                assert_eq!(*start_angle, 0.0);
                assert!(approx(*sweep_angle, FRAC_PI_2));
                assert_eq!(stroke.width, 10.0);
                assert_eq!(*inner_radius, 0.0);
                // Band is 45..55; a 0..90 degree sweep with butt caps spans
                // x = 100..155 and y = 100..155.
                assert!(approx(rect.x, 100.0), "{rect:?}");
                assert!(approx(rect.y, 100.0), "{rect:?}");
                assert!(approx(rect.width, 55.0), "{rect:?}");
                assert!(approx(rect.height, 55.0), "{rect:?}");
            }
            other => panic!("expected arc primitive, got {other:?}"),
        }
    }

    #[test]
    fn draw_arc_bounds_cover_a_quadrant_spanning_sweep() {
        let mut scope = scope(200.0);
        // 0 -> 270 degrees: the bounds must be the full outer circle, not the
        // chord between the two endpoints.
        scope.draw_arc(
            Brush::solid(Color::RED),
            Point::new(100.0, 100.0),
            50.0,
            0.0,
            3.0 * FRAC_PI_2,
            Stroke::new(4.0),
        );
        let DrawPrimitive::Arc { rect, .. } = &scope.into_primitives()[0] else {
            panic!("expected arc primitive");
        };
        assert!(approx(rect.x, 48.0), "{rect:?}");
        assert!(approx(rect.y, 48.0), "{rect:?}");
        assert!(approx(rect.width, 104.0), "{rect:?}");
        assert!(approx(rect.height, 104.0), "{rect:?}");
    }

    #[test]
    fn draw_annular_sector_records_inner_radius_and_no_stroke() {
        let mut scope = scope(200.0);
        scope.draw_annular_sector(
            Brush::solid(Color::WHITE),
            Point::new(100.0, 100.0),
            30.0,
            50.0,
            0.0,
            PI,
        );
        match &scope.into_primitives()[0] {
            DrawPrimitive::Arc {
                rect,
                center,
                radius,
                inner_radius,
                stroke,
                sweep_angle,
                ..
            } => {
                assert!(stroke.is_none(), "annular sectors are filled, not stroked");
                assert_eq!(*center, Point::new(100.0, 100.0));
                assert_eq!(*radius, 50.0);
                assert_eq!(*inner_radius, 30.0);
                assert!(approx(*sweep_angle, PI));
                // 0 -> 180 degrees: x spans -50..+50, y spans 0..+50.
                assert!(approx(rect.x, 50.0), "{rect:?}");
                assert!(approx(rect.y, 100.0), "{rect:?}");
                assert!(approx(rect.width, 100.0), "{rect:?}");
                assert!(approx(rect.height, 50.0), "{rect:?}");
            }
            other => panic!("expected arc primitive, got {other:?}"),
        }
    }

    #[test]
    fn draw_arc_blend_wraps_non_default_modes() {
        let mut scope = scope(100.0);
        scope.draw_arc_blend(
            Brush::solid(Color::RED),
            Point::new(50.0, 50.0),
            20.0,
            0.0,
            1.0,
            Stroke::new(2.0),
            BlendMode::DstOut,
        );
        match &scope.into_primitives()[0] {
            DrawPrimitive::Blend {
                primitive,
                blend_mode,
            } => {
                assert_eq!(*blend_mode, BlendMode::DstOut);
                assert!(matches!(**primitive, DrawPrimitive::Arc { .. }));
            }
            other => panic!("expected blended arc, got {other:?}"),
        }
    }

    #[test]
    fn draw_annular_sector_blend_wraps_non_default_modes() {
        let mut scope = scope(100.0);
        scope.draw_annular_sector_blend(
            Brush::solid(Color::RED),
            Point::new(50.0, 50.0),
            5.0,
            20.0,
            0.0,
            1.0,
            BlendMode::Plus,
        );
        assert!(matches!(
            &scope.into_primitives()[0],
            DrawPrimitive::Blend {
                blend_mode: BlendMode::Plus,
                ..
            }
        ));
    }

    #[test]
    fn stroked_blend_variants_wrap_non_default_modes() {
        let mut scope = scope(20.0);
        scope.draw_rect_stroked_blend(
            Brush::solid(Color::RED),
            Stroke::new(2.0),
            BlendMode::DstOut,
        );
        scope.draw_round_rect_stroked_blend(
            Brush::solid(Color::RED),
            CornerRadii::uniform(2.0),
            Stroke::new(2.0),
            BlendMode::DstOut,
        );
        scope.draw_circle_stroked_blend(
            Brush::solid(Color::RED),
            Point::new(10.0, 10.0),
            5.0,
            Stroke::new(2.0),
            BlendMode::DstOut,
        );
        let primitives = scope.into_primitives();
        assert_eq!(primitives.len(), 3);
        for primitive in &primitives {
            assert!(
                matches!(
                    primitive,
                    DrawPrimitive::Blend {
                        blend_mode: BlendMode::DstOut,
                        ..
                    }
                ),
                "expected blended primitive, got {primitive:?}"
            );
        }
    }

    #[test]
    fn negative_sweeps_and_overlong_sweeps_produce_finite_bounds() {
        let mut scope = scope(200.0);
        scope.draw_arc(
            Brush::solid(Color::RED),
            Point::new(100.0, 100.0),
            40.0,
            FRAC_PI_2,
            -FRAC_PI_2,
            Stroke::new(4.0),
        );
        scope.draw_arc(
            Brush::solid(Color::RED),
            Point::new(100.0, 100.0),
            40.0,
            0.3,
            crate::stroke::TAU * 4.0,
            Stroke::new(4.0),
        );
        let primitives = scope.into_primitives();
        assert_eq!(primitives.len(), 2);

        let DrawPrimitive::Arc { rect: negative, .. } = &primitives[0] else {
            panic!("expected arc");
        };
        // 0 -> 90 degrees clockwise, band 38..42.
        assert!(approx(negative.x, 100.0), "{negative:?}");
        assert!(approx(negative.y, 100.0), "{negative:?}");
        assert!(approx(negative.width, 42.0), "{negative:?}");

        let DrawPrimitive::Arc { rect: full, .. } = &primitives[1] else {
            panic!("expected arc");
        };
        // Anything past a full turn is a closed ring: the whole outer circle.
        assert!(approx(full.x, 58.0), "{full:?}");
        assert!(approx(full.width, 84.0), "{full:?}");
        assert!(approx(full.height, 84.0), "{full:?}");
    }

    #[test]
    fn degenerate_stroke_and_arc_inputs_emit_nothing_and_never_panic() {
        let mut scope = scope(50.0);
        let brush = Brush::solid(Color::RED);
        let center = Point::new(25.0, 25.0);

        // Zero / negative / non-finite stroke widths.
        scope.draw_rect_stroked(brush.clone(), Stroke::new(0.0));
        scope.draw_rect_stroked(brush.clone(), Stroke::new(-4.0));
        scope.draw_rect_stroked(brush.clone(), Stroke::new(f32::NAN));
        scope.draw_round_rect_stroked(brush.clone(), CornerRadii::uniform(2.0), Stroke::new(0.0));
        scope.draw_circle_stroked(brush.clone(), center, 10.0, Stroke::new(0.0));
        scope.draw_circle_stroked(brush.clone(), center, f32::NAN, Stroke::new(2.0));
        // Zero and non-finite sweeps.
        scope.draw_arc(brush.clone(), center, 10.0, 0.0, 0.0, Stroke::new(2.0));
        scope.draw_arc(brush.clone(), center, 10.0, 0.0, f32::NAN, Stroke::new(2.0));
        scope.draw_arc(
            brush.clone(),
            center,
            f32::INFINITY,
            0.0,
            1.0,
            Stroke::new(2.0),
        );
        // Zero-width arc stroke and zero radius with zero width.
        scope.draw_arc(brush.clone(), center, 10.0, 0.0, 1.0, Stroke::new(0.0));
        scope.draw_arc(brush.clone(), center, 0.0, 0.0, 1.0, Stroke::new(0.0));
        // Annular sectors with an empty band.
        scope.draw_annular_sector(brush.clone(), center, 10.0, 10.0, 0.0, 1.0);
        scope.draw_annular_sector(brush.clone(), center, 20.0, 10.0, 0.0, 1.0);
        scope.draw_annular_sector(brush.clone(), center, 0.0, 0.0, 0.0, 1.0);
        scope.draw_annular_sector(brush.clone(), center, 0.0, 10.0, 0.0, 0.0);
        scope.draw_annular_sector(brush, center, f32::NAN, 10.0, 0.0, 1.0);

        assert!(
            scope.into_primitives().is_empty(),
            "degenerate stroke/arc requests must not reach the renderer"
        );
    }

    #[test]
    fn zero_radius_arc_with_positive_width_stays_finite() {
        // radius 0 with a fat stroke is a filled wedge of radius width/2 —
        // legal, and it must not produce NaN bounds.
        let mut scope = scope(50.0);
        scope.draw_arc(
            Brush::solid(Color::RED),
            Point::new(25.0, 25.0),
            0.0,
            0.0,
            FRAC_PI_2,
            Stroke::new(6.0).with_cap(StrokeCap::Round),
        );
        let primitives = scope.into_primitives();
        assert_eq!(primitives.len(), 1);
        let DrawPrimitive::Arc { rect, .. } = &primitives[0] else {
            panic!("expected arc");
        };
        for value in [rect.x, rect.y, rect.width, rect.height] {
            assert!(value.is_finite(), "{rect:?}");
        }
        assert!(rect.width > 0.0 && rect.height > 0.0, "{rect:?}");
    }

    // ── Text ────────────────────────────────────────────────────────────────

    /// Measures every character as a fixed box, so a test can predict the block
    /// a draw is supposed to occupy without depending on a font.
    struct FixedAdvanceTextMeasurer {
        advance: f32,
        line_height: f32,
        calls: std::cell::Cell<usize>,
    }

    impl FixedAdvanceTextMeasurer {
        fn shared(advance: f32, line_height: f32) -> Rc<Self> {
            Rc::new(Self {
                advance,
                line_height,
                calls: std::cell::Cell::new(0),
            })
        }
    }

    impl DrawTextMeasurer for FixedAdvanceTextMeasurer {
        fn measure_text(&self, text: &str, _style: &TextStyle) -> TextMeasurement {
            self.calls.set(self.calls.get() + 1);
            let lines: Vec<&str> = text.split('\n').collect();
            let width = lines
                .iter()
                .map(|line| line.chars().count() as f32 * self.advance)
                .fold(0.0_f32, f32::max);
            TextMeasurement {
                size: Size::new(width, lines.len() as f32 * self.line_height),
                line_height: self.line_height,
                first_baseline: self.line_height * 0.75,
                line_count: lines.len(),
            }
        }
    }

    fn text_scope(size: Size) -> (DrawScopeDefault, Rc<FixedAdvanceTextMeasurer>) {
        let measurer = FixedAdvanceTextMeasurer::shared(10.0, 20.0);
        (
            DrawScopeDefault::with_text_measurer(size, measurer.clone()),
            measurer,
        )
    }

    fn unwrap_text(primitive: &DrawPrimitive) -> &TextPrimitive {
        match primitive {
            DrawPrimitive::Text(text) => text,
            other => panic!("expected text primitive, got {other:?}"),
        }
    }

    #[test]
    fn drawn_text_occupies_exactly_the_box_measure_text_reported() {
        let (mut scope, _) = text_scope(Size::new(200.0, 100.0));
        let style = TextStyle::new(16.0);
        let measured = scope.measure_text("ABCD", &style);

        scope.draw_text_from(
            Point::new(7.0, 11.0),
            Brush::solid(Color::WHITE),
            "ABCD",
            &style,
        );

        let primitives = scope.into_primitives();
        assert_eq!(primitives.len(), 1);
        let text = unwrap_text(&primitives[0]);
        assert_eq!(
            text.rect,
            Rect {
                x: 7.0,
                y: 11.0,
                width: measured.size.width,
                height: measured.size.height,
            },
            "the drawn block must be the measured block, or callers cannot center text"
        );
        assert_eq!(&*text.text, "ABCD");
        assert_eq!(text.color, Color::WHITE);
    }

    #[test]
    fn text_alignment_positions_the_measured_block_inside_the_box() {
        let box_rect = Rect {
            x: 100.0,
            y: 50.0,
            width: 200.0,
            height: 80.0,
        };
        // "AB" measures 20x20 with the fixed-advance measurer.
        let cases = [
            (TextAlign::Left, TextVerticalAlign::Top, 100.0, 50.0),
            (TextAlign::Center, TextVerticalAlign::Center, 190.0, 80.0),
            (TextAlign::Right, TextVerticalAlign::Bottom, 280.0, 110.0),
        ];
        for (align, vertical_align, expected_x, expected_y) in cases {
            let (mut scope, _) = text_scope(Size::new(400.0, 400.0));
            let style = TextStyle::new(16.0)
                .with_align(align)
                .with_vertical_align(vertical_align);
            scope.draw_text_at(box_rect, Brush::solid(Color::WHITE), "AB", &style);
            let primitives = scope.into_primitives();
            let text = unwrap_text(&primitives[0]);
            assert!(
                approx(text.rect.x, expected_x) && approx(text.rect.y, expected_y),
                "{align:?}/{vertical_align:?} placed the block at {:?}",
                text.rect
            );
            assert!(approx(text.rect.width, 20.0) && approx(text.rect.height, 20.0));
        }
    }

    #[test]
    fn baseline_aligned_text_hangs_above_the_box_edge() {
        let (mut scope, _) = text_scope(Size::new(200.0, 200.0));
        let style = TextStyle::new(16.0).with_vertical_align(TextVerticalAlign::Baseline);
        let measured = scope.measure_text("Ag", &style);
        scope.draw_text_at(
            Rect {
                x: 0.0,
                y: 100.0,
                width: 200.0,
                height: 0.0,
            },
            Brush::solid(Color::WHITE),
            "Ag",
            &style,
        );
        let primitives = scope.into_primitives();
        let text = unwrap_text(&primitives[0]);
        // The box edge is the baseline, so the block starts one ascent above it.
        assert!(
            approx(text.rect.y, 100.0 - measured.first_baseline),
            "{:?}",
            text.rect
        );
    }

    #[test]
    fn draw_text_fills_the_whole_scope_rect() {
        let (mut scope, _) = text_scope(Size::new(120.0, 60.0));
        let style = TextStyle::new(16.0)
            .with_align(TextAlign::Right)
            .with_vertical_align(TextVerticalAlign::Bottom);
        scope.draw_text(Brush::solid(Color::WHITE), "AB", &style);
        let primitives = scope.into_primitives();
        let text = unwrap_text(&primitives[0]);
        assert!(
            approx(text.rect.x, 100.0) && approx(text.rect.y, 40.0),
            "{:?}",
            text.rect
        );
    }

    #[test]
    fn draw_text_from_ignores_alignment_and_anchors_the_top_left() {
        let (mut scope, _) = text_scope(Size::new(400.0, 400.0));
        // Alignment would move the block if the anchor form honored it.
        let style = TextStyle::new(16.0)
            .with_align(TextAlign::Center)
            .with_vertical_align(TextVerticalAlign::Bottom);
        scope.draw_text_from(
            Point::new(30.0, 40.0),
            Brush::solid(Color::WHITE),
            "AB",
            &style,
        );
        let primitives = scope.into_primitives();
        let text = unwrap_text(&primitives[0]);
        assert!(
            approx(text.rect.x, 30.0) && approx(text.rect.y, 40.0),
            "{:?}",
            text.rect
        );
    }

    #[test]
    fn multiline_text_measures_the_widest_line_and_stacks_the_lines() {
        let (mut scope, _) = text_scope(Size::new(400.0, 400.0));
        let style = TextStyle::new(16.0);
        scope.draw_text_from(Point::ZERO, Brush::solid(Color::WHITE), "AB\nABCDE", &style);
        let primitives = scope.into_primitives();
        let text = unwrap_text(&primitives[0]);
        assert!(approx(text.rect.width, 50.0), "{:?}", text.rect);
        assert!(approx(text.rect.height, 40.0), "{:?}", text.rect);
    }

    #[test]
    fn empty_text_draws_nothing_and_never_measures() {
        let (mut scope, measurer) = text_scope(Size::new(100.0, 100.0));
        scope.draw_text(Brush::solid(Color::WHITE), "", &TextStyle::new(16.0));
        scope.draw_text_at(
            Rect::from_size(Size::new(10.0, 10.0)),
            Brush::solid(Color::WHITE),
            "",
            &TextStyle::new(16.0),
        );
        scope.draw_text_from(
            Point::ZERO,
            Brush::solid(Color::WHITE),
            "",
            &TextStyle::new(16.0),
        );
        assert!(scope.into_primitives().is_empty());
        assert_eq!(
            measurer.calls.get(),
            0,
            "an empty string must not cost a measurement"
        );
    }

    #[test]
    fn invisible_text_draws_nothing() {
        let (mut scope, _) = text_scope(Size::new(100.0, 100.0));
        let style = TextStyle::new(16.0);
        scope.draw_text(Brush::solid(Color(1.0, 1.0, 1.0, 0.0)), "AB", &style);
        scope.draw_text(
            Brush::LinearGradient {
                colors: Vec::new(),
                stops: None,
                start: Point::ZERO,
                end: Point::new(1.0, 1.0),
                tile_mode: crate::render_effect::TileMode::Clamp,
            },
            "AB",
            &style,
        );
        assert!(scope.into_primitives().is_empty());
    }

    #[test]
    fn gradient_text_brushes_fall_back_to_their_first_stop() {
        let (mut scope, _) = text_scope(Size::new(100.0, 100.0));
        scope.draw_text(
            Brush::linear_gradient(vec![Color::RED, Color::BLUE]),
            "AB",
            &TextStyle::new(16.0),
        );
        let primitives = scope.into_primitives();
        assert_eq!(unwrap_text(&primitives[0]).color, Color::RED);
    }

    #[test]
    fn a_scope_without_a_measurer_falls_back_to_the_font_free_estimate() {
        let mut scope = DrawScopeDefault::new(Size::new(100.0, 100.0));
        let style = TextStyle::new(16.0);
        assert_eq!(
            scope.measure_text("ABC", &style),
            crate::estimate_text_measurement("ABC", &style)
        );
        scope.draw_text_from(Point::ZERO, Brush::solid(Color::WHITE), "ABC", &style);
        let primitives = scope.into_primitives();
        let text = unwrap_text(&primitives[0]);
        assert!(text.rect.width > 0.0 && text.rect.height > 0.0);
    }

    #[test]
    fn degenerate_text_geometry_emits_nothing_and_never_panics() {
        struct DegenerateTextMeasurer;
        impl DrawTextMeasurer for DegenerateTextMeasurer {
            fn measure_text(&self, _text: &str, _style: &TextStyle) -> TextMeasurement {
                TextMeasurement {
                    size: Size::new(f32::NAN, 0.0),
                    line_height: f32::NAN,
                    first_baseline: f32::NAN,
                    line_count: 1,
                }
            }
        }

        let mut scope = DrawScopeDefault::with_text_measurer(
            Size::new(50.0, 50.0),
            Rc::new(DegenerateTextMeasurer),
        );
        scope.draw_text(Brush::solid(Color::WHITE), "AB", &TextStyle::new(16.0));
        scope.draw_text_at(
            Rect {
                x: f32::NAN,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            Brush::solid(Color::WHITE),
            "AB",
            &TextStyle::new(16.0),
        );
        assert!(
            scope.into_primitives().is_empty(),
            "unmeasurable text must not reach the renderer"
        );
    }

    #[test]
    fn text_style_survives_lowering_into_the_primitive() {
        let (mut scope, _) = text_scope(Size::new(100.0, 100.0));
        let style = TextStyle::new(21.0)
            .with_font_family("Fira Sans")
            .with_weight(FontWeight::BOLD)
            .with_style(FontStyle::Italic)
            .with_letter_spacing(2.0)
            .with_line_height(26.0);
        scope.draw_text(Brush::solid(Color::WHITE), "AB", &style);
        let primitives = scope.into_primitives();
        assert_eq!(unwrap_text(&primitives[0]).style, style);
    }

    #[test]
    fn a_layers_composite_alpha_is_a_truncated_byte() {
        for byte in 0..=255u32 {
            let exact = byte as f32 / 255.0;
            assert!(
                (GraphicsLayer::composite_alpha_8bit(exact) - exact).abs() < 1e-6,
                "byte {byte} moved"
            );
            if byte < 255 {
                // Anything above a byte and below the next composites at the
                // byte below it, however close to the next it sits. Rounding
                // would take the top of that range up, and HWUI's `(int)` does
                // not.
                let nearly_next = (byte as f32 + 0.999) / 255.0;
                assert!(
                    (GraphicsLayer::composite_alpha_8bit(nearly_next) - exact).abs() < 1e-6,
                    "byte {byte} + 0.999 did not truncate"
                );
            }
        }
        assert_eq!(GraphicsLayer::composite_alpha_8bit(1.0), 1.0);
        assert_eq!(GraphicsLayer::composite_alpha_8bit(0.0), 0.0);
        assert_eq!(GraphicsLayer::composite_alpha_8bit(-3.0), 0.0);
        assert_eq!(GraphicsLayer::composite_alpha_8bit(7.0), 1.0);
    }
}
