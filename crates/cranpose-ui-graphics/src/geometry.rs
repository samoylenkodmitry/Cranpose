//! Geometric primitives: Point, Size, Rect, Insets, Path

use std::{ops::AddAssign, rc::Rc};

use crate::{
    ArcRecordArgs, Brush, Color, ColorFilter, CommandRecorder, CommandRecording, ImageBitmap,
    ImageSampling, normalized_band,
    stroke::Stroke,
    typography::{
        DrawTextMeasurer, DrawTextStyle, TextAlign, TextMeasurement, TextVerticalAlign,
        estimate_text_measurement,
    },
};

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
    /// The rect that holds nothing: what two clips that do not overlap
    /// resolve to, so a clip that meets nothing stays a clip instead of
    /// lifting.
    pub const EMPTY: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    };

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
    pub style: DrawTextStyle,
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
    use std::{
        cell::RefCell,
        collections::HashMap,
        hash::{Hash, Hasher},
    };

    const POOL_CAPACITY: usize = 256;
    thread_local! {
        static POOL: RefCell<HashMap<u64, Rc<str>>> = RefCell::new(HashMap::new());
    }

    let mut hasher = crate::FxHasher::default();
    text.hash(&mut hasher);
    let key = hasher.finish();

    POOL.with(|pool| {
        let mut pool = pool.borrow_mut();
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

#[derive(Default)]
pub struct DrawScopeDefault {
    size: Size,
    recording: CommandRecorder,
    text_measurer: Option<Rc<dyn DrawTextMeasurer>>,
}

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
        Self::with_storage(size, None, CommandRecording::default())
    }

    /// A scope that measures text with the app's fonts.
    ///
    /// The framework calls this for every draw closure it runs; `new` exists
    /// for callers that never draw text.
    pub fn with_text_measurer(size: Size, text_measurer: Rc<dyn DrawTextMeasurer>) -> Self {
        Self::with_storage(size, Some(text_measurer), CommandRecording::default())
    }

    /// A scope recording into `storage`, a recording the caller kept from an
    /// earlier frame so its buffers keep the capacity they earned.
    pub fn with_text_measurer_reusing(
        size: Size,
        text_measurer: Rc<dyn DrawTextMeasurer>,
        storage: CommandRecording,
    ) -> Self {
        Self::with_storage(size, Some(text_measurer), storage)
    }

    fn with_storage(
        size: Size,
        text_measurer: Option<Rc<dyn DrawTextMeasurer>>,
        recording: CommandRecording,
    ) -> Self {
        let mut recording = CommandRecorder::reusing(recording);
        recording.reserve_shapes(recorded_primitive_capacity(size));
        Self {
            size,
            recording,
            text_measurer,
        }
    }

    /// How many `draw_content` markers this scope has recorded.
    pub fn content_marker_count(&self) -> u32 {
        self.recording.content_markers()
    }

    /// Records primitives already built, as if each had been drawn here.
    pub fn push_recorded(&mut self, primitives: impl IntoIterator<Item = DrawPrimitive>) {
        for primitive in primitives {
            self.recording.push_primitive(primitive);
        }
    }

    /// The recording, in the storage it was recorded into, so the caller
    /// can lend it to the same command's next recording.
    pub fn finish(self) -> CommandRecording {
        note_recorded_primitive_count(self.size, self.recording.len());
        self.recording.finish()
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

    #[allow(clippy::too_many_arguments)]
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
}

impl DrawScope for DrawScopeDefault {
    fn size(&self) -> Size {
        self.size
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
        const SUPERSAMPLE: f32 = 2.0;
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

        self.recording.push_other(DrawPrimitive::Image {
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

    fn measure_text(&self, text: &str, style: &DrawTextStyle) -> TextMeasurement {
        match &self.text_measurer {
            Some(measurer) => measurer.measure_text(text, style),
            None => estimate_text_measurement(text, style),
        }
    }

    fn draw_text_at(&mut self, rect: Rect, brush: Brush, text: &str, style: &DrawTextStyle) {
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
        self.recording
            .push_other(DrawPrimitive::Text(Box::new(TextPrimitive {
                rect: Rect::from_origin_size(origin, measurement.size),
                text: shared_text_str(text),
                style: style.clone(),
                color,
            })));
    }

    fn into_primitives(self) -> Vec<DrawPrimitive> {
        self.finish().into_primitives_with_markers()
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
#[path = "tests/geometry_tests.rs"]
mod tests;
