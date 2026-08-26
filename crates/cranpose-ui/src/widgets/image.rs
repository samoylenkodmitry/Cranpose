//! Image composable and painter primitives.

#![allow(non_snake_case)]
#![allow(clippy::too_many_arguments)] // API matches Jetpack Compose Image signature.

#[cfg(feature = "svg")]
use std::sync::{Mutex, MutexGuard};
use std::{
    hash::{Hash, Hasher},
    sync::Arc,
};

use cranpose_core::NodeId;
use cranpose_ui_graphics::{ColorFilter, DrawScope, ImageBitmap, ImageSampling};
use cranpose_ui_layout::{Constraints, MeasurePolicy, MeasureResult, MeasureScope, Placement};
use thiserror::Error;

use crate::{
    composable,
    layout::core::{Alignment, Measurable},
    modifier::{Modifier, Rect, Size},
    nine_patch::{NinePatchInsets, PatchFill, PatchQuad, nine_patch_quads, tile_quads},
    widgets::Layout,
};

#[cfg(feature = "svg")]
#[path = "image_svg.rs"]
mod image_svg;

pub const DEFAULT_ALPHA: f32 = 1.0;
#[cfg(feature = "svg")]
const SVG_RASTER_CACHE_LIMIT: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ContentScale {
    Fit,
    Crop,
    FillBounds,
    FillWidth,
    FillHeight,
    Inside,
    None,
}

impl ContentScale {
    pub fn scaled_size(self, src_size: Size, dst_size: Size) -> Size {
        if src_size.width <= 0.0
            || src_size.height <= 0.0
            || dst_size.width <= 0.0
            || dst_size.height <= 0.0
        {
            return Size::ZERO;
        }

        let scale_x = dst_size.width / src_size.width;
        let scale_y = dst_size.height / src_size.height;

        let (factor_x, factor_y) = match self {
            Self::Fit => {
                let factor = scale_x.min(scale_y);
                (factor, factor)
            }
            Self::Crop => {
                let factor = scale_x.max(scale_y);
                (factor, factor)
            }
            Self::FillBounds => (scale_x, scale_y),
            Self::FillWidth => (scale_x, scale_x),
            Self::FillHeight => (scale_y, scale_y),
            Self::Inside => {
                if src_size.width <= dst_size.width && src_size.height <= dst_size.height {
                    (1.0, 1.0)
                } else {
                    let factor = scale_x.min(scale_y);
                    (factor, factor)
                }
            }
            Self::None => (1.0, 1.0),
        };

        Size {
            width: src_size.width * factor_x,
            height: src_size.height * factor_y,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Painter {
    kind: PainterKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum PainterKind {
    Bitmap(ImageBitmap),
    BitmapRegion {
        bitmap: ImageBitmap,
        source: Quad,
        sampling: ImageSampling,
    },
    /// A source repeated at its own size across whatever space it is given,
    /// rather than scaled to fit it.
    BitmapTiled {
        bitmap: ImageBitmap,
        source: Quad,
        sampling: ImageSampling,
    },
    /// A source whose corners keep their size while its edges and middle grow.
    NinePatch {
        bitmap: ImageBitmap,
        source: Quad,
        insets: Quad,
        center: PatchFill,
        edges: PatchFill,
        sampling: ImageSampling,
    },
    Svg(SvgPainter),
}

/// Four `f32`s carried as bits.
///
/// A painter is compared and hashed so a composable can skip when it has not
/// changed, and floats are neither `Eq` nor `Hash`. Keeping the bits makes the
/// comparison exact — two painters built from the same numbers are the same
/// painter — without asking the caller to think about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Quad {
    a: u32,
    b: u32,
    c: u32,
    d: u32,
}

impl Quad {
    fn of(a: f32, b: f32, c: f32, d: f32) -> Self {
        Self {
            a: a.to_bits(),
            b: b.to_bits(),
            c: c.to_bits(),
            d: d.to_bits(),
        }
    }

    fn values(self) -> (f32, f32, f32, f32) {
        (
            f32::from_bits(self.a),
            f32::from_bits(self.b),
            f32::from_bits(self.c),
            f32::from_bits(self.d),
        )
    }

    fn from_rect(source: Rect) -> Self {
        Self::of(source.x, source.y, source.width, source.height)
    }

    fn rect(self) -> Rect {
        let (x, y, width, height) = self.values();
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    fn from_insets(insets: NinePatchInsets) -> Self {
        Self::of(insets.left, insets.top, insets.right, insets.bottom)
    }

    fn insets(self) -> NinePatchInsets {
        let (left, top, right, bottom) = self.values();
        NinePatchInsets::new(left, top, right, bottom)
    }
}

impl Painter {
    pub fn from_bitmap(bitmap: ImageBitmap) -> Self {
        Self {
            kind: PainterKind::Bitmap(bitmap),
        }
    }

    /// Creates a painter for one source region of a bitmap atlas.
    pub fn from_bitmap_region(bitmap: ImageBitmap, source: Rect, sampling: ImageSampling) -> Self {
        Self {
            kind: PainterKind::BitmapRegion {
                bitmap,
                source: Quad::from_rect(source),
                sampling,
            },
        }
    }

    /// Creates a painter that repeats `source` at its own size across whatever
    /// space it is given, instead of scaling it to fit.
    ///
    /// A texture, a hatch or a skin background covers an area of any size
    /// without going soft. `source` is a region of `bitmap`, so an application
    /// tiles one sprite out of an atlas and never the whole sheet.
    pub fn from_bitmap_tiled(bitmap: ImageBitmap, source: Rect, sampling: ImageSampling) -> Self {
        Self {
            kind: PainterKind::BitmapTiled {
                bitmap,
                source: Quad::from_rect(source),
                sampling,
            },
        }
    }

    /// Creates a painter whose corners keep their own size while its edges and
    /// middle grow — one drawing of a button, panel or trough used at every
    /// size.
    ///
    /// `center` and `edges` decide whether the growing parts are stretched or
    /// tiled. Insets that leave no middle, or a destination with no room for
    /// the corners, fall back to a plain scale rather than drawing corners over
    /// each other.
    pub fn from_nine_patch(
        bitmap: ImageBitmap,
        source: Rect,
        insets: NinePatchInsets,
        center: PatchFill,
        edges: PatchFill,
        sampling: ImageSampling,
    ) -> Self {
        Self {
            kind: PainterKind::NinePatch {
                bitmap,
                source: Quad::from_rect(source),
                insets: Quad::from_insets(insets),
                center,
                edges,
                sampling,
            },
        }
    }

    pub fn from_svg(svg: SvgPainter) -> Self {
        Self {
            kind: PainterKind::Svg(svg),
        }
    }

    pub fn intrinsic_size(&self) -> Size {
        match &self.kind {
            PainterKind::Bitmap(bitmap) => bitmap.intrinsic_size(),
            PainterKind::BitmapRegion { source, .. }
            | PainterKind::BitmapTiled { source, .. }
            | PainterKind::NinePatch { source, .. } => {
                let source = source.rect();
                Size::new(source.width.max(0.0), source.height.max(0.0))
            }
            PainterKind::Svg(svg) => svg.intrinsic_size(),
        }
    }

    pub fn as_bitmap(&self) -> Option<&ImageBitmap> {
        match &self.kind {
            PainterKind::Bitmap(bitmap) => Some(bitmap),
            PainterKind::BitmapRegion { bitmap, .. }
            | PainterKind::BitmapTiled { bitmap, .. }
            | PainterKind::NinePatch { bitmap, .. } => Some(bitmap),
            PainterKind::Svg(_) => None,
        }
    }

    /// Returns the underlying bitmap when this painter is bitmap-backed.
    pub fn bitmap(&self) -> Option<&ImageBitmap> {
        self.as_bitmap()
    }
}

impl From<ImageBitmap> for Painter {
    fn from(value: ImageBitmap) -> Self {
        Self::from_bitmap(value)
    }
}

impl From<SvgPainter> for Painter {
    fn from(value: SvgPainter) -> Self {
        Self::from_svg(value)
    }
}

pub fn BitmapPainter(bitmap: ImageBitmap) -> Painter {
    Painter::from_bitmap(bitmap)
}

/// Creates a painter that draws one region from a bitmap atlas.
pub fn BitmapRegionPainter(bitmap: ImageBitmap, source: Rect, sampling: ImageSampling) -> Painter {
    Painter::from_bitmap_region(bitmap, source, sampling)
}

/// Creates a painter that repeats one region of a bitmap across its bounds.
pub fn TiledPainter(bitmap: ImageBitmap, source: Rect, sampling: ImageSampling) -> Painter {
    Painter::from_bitmap_tiled(bitmap, source, sampling)
}

/// Creates a painter whose corners stay put while its edges and middle grow.
pub fn NinePatchPainter(
    bitmap: ImageBitmap,
    source: Rect,
    insets: NinePatchInsets,
    center: PatchFill,
    edges: PatchFill,
    sampling: ImageSampling,
) -> Painter {
    Painter::from_nine_patch(bitmap, source, insets, center, edges, sampling)
}

/// Errors returned while parsing or rasterizing an SVG painter.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SvgPainterError {
    #[error("SVG support is disabled; enable the cranpose-ui `svg` feature")]
    SvgFeatureDisabled,
    #[error("failed to parse SVG: {0}")]
    Parse(String),
    #[error("SVG raster dimensions must be greater than zero")]
    InvalidRasterDimensions,
    #[error("SVG raster dimensions are too large")]
    RasterDimensionsTooLarge,
    #[error("failed to allocate SVG raster {width}x{height}")]
    RasterAllocationFailed { width: u32, height: u32 },
    #[error("SVG raster cache is unavailable")]
    RasterCacheUnavailable,
    #[error(transparent)]
    ImageBitmap(#[from] cranpose_ui_graphics::ImageBitmapError),
}

/// Parsed SVG image data that rasterizes on demand for the requested draw size.
#[derive(Clone)]
pub struct SvgPainter {
    inner: Arc<SvgPainterInner>,
}

struct SvgPainterInner {
    #[cfg(feature = "svg")]
    document: image_svg::SvgDocument,
    intrinsic_size: Size,
    #[cfg(feature = "svg")]
    cache: Mutex<SvgRasterCache>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg(feature = "svg")]
struct SvgRasterKey {
    width: u32,
    height: u32,
}

#[derive(Clone, Debug)]
#[cfg(feature = "svg")]
struct SvgRasterEntry {
    key: SvgRasterKey,
    bitmap: ImageBitmap,
}

#[derive(Default, Debug)]
#[cfg(feature = "svg")]
struct SvgRasterCache {
    entries: Vec<SvgRasterEntry>,
}

impl SvgPainter {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, SvgPainterError> {
        #[cfg(not(feature = "svg"))]
        {
            let _ = bytes;
            Err(SvgPainterError::SvgFeatureDisabled)
        }
        #[cfg(feature = "svg")]
        {
            let document = image_svg::parse_svg_document(bytes)?;
            let intrinsic_size = document.intrinsic_size();

            Ok(Self {
                inner: Arc::new(SvgPainterInner {
                    document,
                    intrinsic_size,
                    cache: Mutex::new(SvgRasterCache::default()),
                }),
            })
        }
    }

    pub fn id(&self) -> u64 {
        Arc::as_ptr(&self.inner) as usize as u64
    }

    pub fn intrinsic_size(&self) -> Size {
        self.inner.intrinsic_size
    }

    pub fn rasterize(&self, pixel_size: Size) -> Result<ImageBitmap, SvgPainterError> {
        #[cfg(not(feature = "svg"))]
        {
            let _ = pixel_size;
            Err(SvgPainterError::SvgFeatureDisabled)
        }
        #[cfg(feature = "svg")]
        {
            let key = svg_raster_key(pixel_size)?;
            if let Some(bitmap) = self.cached_bitmap(key)? {
                return Ok(bitmap);
            }

            let bitmap = self.rasterize_uncached(key)?;
            self.cache_bitmap(key, bitmap.clone())?;
            Ok(bitmap)
        }
    }

    #[cfg(feature = "svg")]
    fn cached_bitmap(&self, key: SvgRasterKey) -> Result<Option<ImageBitmap>, SvgPainterError> {
        let mut cache = self.lock_cache();
        Ok(cache.get(key))
    }

    #[cfg(feature = "svg")]
    fn cache_bitmap(&self, key: SvgRasterKey, bitmap: ImageBitmap) -> Result<(), SvgPainterError> {
        let mut cache = self.lock_cache();
        cache.insert(key, bitmap);
        Ok(())
    }

    #[cfg(feature = "svg")]
    fn lock_cache(&self) -> MutexGuard<'_, SvgRasterCache> {
        self.inner
            .cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[cfg(feature = "svg")]
    fn rasterize_uncached(&self, key: SvgRasterKey) -> Result<ImageBitmap, SvgPainterError> {
        (key.width as usize)
            .checked_mul(key.height as usize)
            .and_then(|value| value.checked_mul(4))
            .ok_or(SvgPainterError::RasterDimensionsTooLarge)?;

        let mut pixmap = tiny_skia::Pixmap::new(key.width, key.height).ok_or(
            SvgPainterError::RasterAllocationFailed {
                width: key.width,
                height: key.height,
            },
        )?;
        image_svg::rasterize_svg_document(&self.inner.document, &mut pixmap);
        let pixels = image_svg::demultiplied_rgba_pixels(&pixmap);
        Ok(ImageBitmap::from_rgba8(key.width, key.height, pixels)?)
    }
}

impl std::fmt::Debug for SvgPainter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SvgPainter")
            .field("id", &self.id())
            .field("intrinsic_size", &self.intrinsic_size())
            .finish_non_exhaustive()
    }
}

impl PartialEq for SvgPainter {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}

impl Eq for SvgPainter {}

impl Hash for SvgPainter {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id().hash(state);
    }
}

#[cfg(feature = "svg")]
impl SvgRasterCache {
    fn get(&mut self, key: SvgRasterKey) -> Option<ImageBitmap> {
        let position = self.entries.iter().position(|entry| entry.key == key)?;
        let entry = self.entries.remove(position);
        let bitmap = entry.bitmap.clone();
        self.entries.push(entry);
        Some(bitmap)
    }

    fn insert(&mut self, key: SvgRasterKey, bitmap: ImageBitmap) {
        if let Some(position) = self.entries.iter().position(|entry| entry.key == key) {
            self.entries.remove(position);
        } else if self.entries.len() >= SVG_RASTER_CACHE_LIMIT {
            self.entries.remove(0);
        }
        self.entries.push(SvgRasterEntry { key, bitmap });
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct SvgBytesKey {
    ptr: usize,
    len: usize,
}

impl SvgBytesKey {
    fn new(bytes: &'static [u8]) -> Self {
        Self {
            ptr: bytes.as_ptr() as usize,
            len: bytes.len(),
        }
    }
}

pub fn rememberSvg(bytes: &'static [u8]) -> Result<SvgPainter, SvgPainterError> {
    let key = SvgBytesKey::new(bytes);
    cranpose_core::withCurrentComposer(|composer| {
        composer.with_key(&key, |composer| {
            composer
                .remember(|| SvgPainter::from_bytes(bytes))
                .with(|result| result.clone())
        })
    })
}

/// Measure policy for Image that preserves aspect ratio when constraints
/// force the image smaller than its intrinsic size.
///
/// Unlike [`LeafMeasurePolicy`] which clamps width and height independently,
/// this scales both dimensions by the same factor so the image is never
/// distorted by layout constraints.
#[derive(Clone, Debug, PartialEq)]
struct ImageMeasurePolicy {
    intrinsic_size: Size,
}

impl MeasurePolicy for ImageMeasurePolicy {
    fn measure(
        &self,
        scope: &dyn MeasureScope,
        _measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        let mut placements = Vec::new();
        let size = self.measure_into(scope, &[], constraints, &mut placements);
        MeasureResult::new(size, placements)
    }

    fn measure_into(
        &self,
        _scope: &dyn MeasureScope,
        _measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
        placements: &mut Vec<Placement>,
    ) -> Size {
        placements.clear();
        let iw = self.intrinsic_size.width;
        let ih = self.intrinsic_size.height;

        if iw <= 0.0 || ih <= 0.0 {
            let (w, h) = constraints.constrain(0.0, 0.0);
            return Size {
                width: w,
                height: h,
            };
        }

        // Clamp each axis to its constraint range.
        let cw = iw.clamp(constraints.min_width, constraints.max_width);
        let ch = ih.clamp(constraints.min_height, constraints.max_height);

        // If either axis had to shrink, scale both by the smaller factor
        // so the aspect ratio is preserved.
        let scale_x = cw / iw;
        let scale_y = ch / ih;

        let (width, height) = if scale_x < 1.0 || scale_y < 1.0 {
            let factor = scale_x.min(scale_y);
            let w = (iw * factor).clamp(constraints.min_width, constraints.max_width);
            let h = (ih * factor).clamp(constraints.min_height, constraints.max_height);
            (w, h)
        } else {
            (cw, ch)
        };

        Size { width, height }
    }

    fn min_intrinsic_width(&self, _measurables: &[Box<dyn Measurable>], _height: f32) -> f32 {
        self.intrinsic_size.width
    }

    fn max_intrinsic_width(&self, _measurables: &[Box<dyn Measurable>], _height: f32) -> f32 {
        self.intrinsic_size.width
    }

    fn min_intrinsic_height(&self, _measurables: &[Box<dyn Measurable>], _width: f32) -> f32 {
        self.intrinsic_size.height
    }

    fn max_intrinsic_height(&self, _measurables: &[Box<dyn Measurable>], _width: f32) -> f32 {
        self.intrinsic_size.height
    }
}

fn destination_rect(
    src_size: Size,
    dst_size: Size,
    alignment: Alignment,
    content_scale: ContentScale,
) -> Rect {
    let draw_size = content_scale.scaled_size(src_size, dst_size);
    let allows_overflow = content_scale == ContentScale::Crop;
    let offset_x = aligned_x_offset(
        alignment.horizontal,
        dst_size.width,
        draw_size.width,
        allows_overflow,
    );
    let offset_y = aligned_y_offset(
        alignment.vertical,
        dst_size.height,
        draw_size.height,
        allows_overflow,
    );
    Rect {
        x: offset_x,
        y: offset_y,
        width: draw_size.width,
        height: draw_size.height,
    }
}

fn aligned_x_offset(
    alignment: cranpose_ui_layout::HorizontalAlignment,
    available: f32,
    child: f32,
    allows_overflow: bool,
) -> f32 {
    if !allows_overflow {
        return alignment.align(available, child);
    }

    match alignment {
        cranpose_ui_layout::HorizontalAlignment::Start => 0.0,
        cranpose_ui_layout::HorizontalAlignment::CenterHorizontally => (available - child) / 2.0,
        cranpose_ui_layout::HorizontalAlignment::End => available - child,
    }
}

fn aligned_y_offset(
    alignment: cranpose_ui_layout::VerticalAlignment,
    available: f32,
    child: f32,
    allows_overflow: bool,
) -> f32 {
    if !allows_overflow {
        return alignment.align(available, child);
    }

    match alignment {
        cranpose_ui_layout::VerticalAlignment::Top => 0.0,
        cranpose_ui_layout::VerticalAlignment::CenterVertically => (available - child) / 2.0,
        cranpose_ui_layout::VerticalAlignment::Bottom => available - child,
    }
}

fn map_destination_clip_to_source(
    src_rect: Rect,
    dst_rect: Rect,
    clipped_dst_rect: Rect,
) -> Option<Rect> {
    if src_rect.width <= 0.0
        || src_rect.height <= 0.0
        || dst_rect.width <= 0.0
        || dst_rect.height <= 0.0
        || clipped_dst_rect.width <= 0.0
        || clipped_dst_rect.height <= 0.0
    {
        return None;
    }

    let scale_x = src_rect.width / dst_rect.width;
    let scale_y = src_rect.height / dst_rect.height;

    let src_min_x = src_rect.x;
    let src_min_y = src_rect.y;
    let src_max_x = src_rect.x + src_rect.width;
    let src_max_y = src_rect.y + src_rect.height;

    let raw_left = src_rect.x + (clipped_dst_rect.x - dst_rect.x) * scale_x;
    let raw_top = src_rect.y + (clipped_dst_rect.y - dst_rect.y) * scale_y;
    let raw_right =
        src_rect.x + ((clipped_dst_rect.x + clipped_dst_rect.width) - dst_rect.x) * scale_x;
    let raw_bottom =
        src_rect.y + ((clipped_dst_rect.y + clipped_dst_rect.height) - dst_rect.y) * scale_y;

    let left = raw_left.clamp(src_min_x, src_max_x);
    let top = raw_top.clamp(src_min_y, src_max_y);
    let right = raw_right.clamp(src_min_x, src_max_x);
    let bottom = raw_bottom.clamp(src_min_y, src_max_y);
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

fn image_destination_clip(
    src_size: Size,
    container_size: Size,
    alignment: Alignment,
    content_scale: ContentScale,
) -> Option<(Rect, Rect)> {
    let dst_rect = destination_rect(src_size, container_size, alignment, content_scale);
    if dst_rect.width <= 0.0 || dst_rect.height <= 0.0 {
        return None;
    }

    let container_rect = Rect::from_size(container_size);
    let clipped_dst_rect = dst_rect.intersect(container_rect)?;
    Some((dst_rect, clipped_dst_rect))
}

fn draw_bitmap_painter(
    scope: &mut dyn DrawScope,
    bitmap: ImageBitmap,
    intrinsic_size: Size,
    alignment: Alignment,
    content_scale: ContentScale,
    alpha: f32,
    color_filter: Option<ColorFilter>,
) {
    let container_size = scope.size();
    let Some((dst_rect, clipped_dst_rect)) =
        image_destination_clip(intrinsic_size, container_size, alignment, content_scale)
    else {
        return;
    };
    let full_src_rect = Rect::from_size(Size::new(bitmap.width() as f32, bitmap.height() as f32));
    let Some(clipped_src_rect) =
        map_destination_clip_to_source(full_src_rect, dst_rect, clipped_dst_rect)
    else {
        return;
    };
    scope.draw_image_src_sampled(
        bitmap,
        clipped_src_rect,
        clipped_dst_rect,
        alpha,
        color_filter,
        ImageSampling::Linear,
    );
}

fn draw_bitmap_region_painter(
    scope: &mut dyn DrawScope,
    bitmap: ImageBitmap,
    source: Rect,
    alignment: Alignment,
    content_scale: ContentScale,
    alpha: f32,
    color_filter: Option<ColorFilter>,
    sampling: ImageSampling,
) {
    let source_size = Size::new(source.width.max(0.0), source.height.max(0.0));
    let Some((dst_rect, clipped_dst_rect)) =
        image_destination_clip(source_size, scope.size(), alignment, content_scale)
    else {
        return;
    };
    let Some(clipped_source) = map_destination_clip_to_source(source, dst_rect, clipped_dst_rect)
    else {
        return;
    };
    scope.draw_image_src_sampled(
        bitmap,
        clipped_source,
        clipped_dst_rect,
        alpha,
        color_filter,
        sampling,
    );
}

/// Draws a list of source-to-destination quads, clipped to the container.
///
/// Every fill that is not a plain scale — tiling, nine-patch — reduces to this,
/// so the clipping rule and the sampling choice are stated once.
fn draw_patch_quads(
    scope: &mut dyn DrawScope,
    bitmap: &ImageBitmap,
    quads: &[PatchQuad],
    container: Rect,
    alpha: f32,
    color_filter: Option<ColorFilter>,
    sampling: ImageSampling,
) {
    for quad in quads {
        let Some(clipped_destination) = intersect(quad.destination, container) else {
            continue;
        };
        let Some(clipped_source) =
            map_destination_clip_to_source(quad.source, quad.destination, clipped_destination)
        else {
            continue;
        };
        scope.draw_image_src_sampled(
            bitmap.clone(),
            clipped_source,
            clipped_destination,
            alpha,
            color_filter,
            sampling,
        );
    }
}

fn intersect(rect: Rect, bounds: Rect) -> Option<Rect> {
    let left = rect.x.max(bounds.x);
    let top = rect.y.max(bounds.y);
    let right = (rect.x + rect.width).min(bounds.x + bounds.width);
    let bottom = (rect.y + rect.height).min(bounds.y + bounds.height);
    if right <= left || bottom <= top {
        return None;
    }
    Some(Rect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
}

/// Draws a tiled painter across the whole container.
///
/// Content scale and alignment do not apply: a tiled fill covers its bounds by
/// definition, and scaling the source would defeat the reason for tiling it.
fn draw_bitmap_tiled_painter(
    scope: &mut dyn DrawScope,
    bitmap: ImageBitmap,
    source: Rect,
    alpha: f32,
    color_filter: Option<ColorFilter>,
    sampling: ImageSampling,
) {
    let container = Rect::from_size(scope.size());
    let quads = tile_quads(source, container);
    draw_patch_quads(
        scope,
        &bitmap,
        &quads,
        container,
        alpha,
        color_filter,
        sampling,
    );
}

/// Draws a nine-patch painter across the whole container.
///
/// Like tiling, this fills its bounds by construction, so content scale and
/// alignment have nothing left to decide.
#[allow(clippy::too_many_arguments)]
fn draw_nine_patch_painter(
    scope: &mut dyn DrawScope,
    bitmap: ImageBitmap,
    source: Rect,
    insets: NinePatchInsets,
    center: PatchFill,
    edges: PatchFill,
    alpha: f32,
    color_filter: Option<ColorFilter>,
    sampling: ImageSampling,
) {
    let container = Rect::from_size(scope.size());
    let quads = nine_patch_quads(source, container, insets, center, edges);
    draw_patch_quads(
        scope,
        &bitmap,
        &quads,
        container,
        alpha,
        color_filter,
        sampling,
    );
}

fn draw_svg_painter(
    scope: &mut dyn DrawScope,
    svg: SvgPainter,
    intrinsic_size: Size,
    alignment: Alignment,
    content_scale: ContentScale,
    alpha: f32,
    color_filter: Option<ColorFilter>,
) {
    let container_size = scope.size();
    let Some((dst_rect, clipped_dst_rect)) =
        image_destination_clip(intrinsic_size, container_size, alignment, content_scale)
    else {
        return;
    };
    let density = crate::render_state::current_density();
    let pixel_size = Size::new(dst_rect.width * density, dst_rect.height * density);
    let bitmap = match svg.rasterize(pixel_size) {
        Ok(bitmap) => bitmap,
        Err(error) => {
            log::warn!("failed to rasterize SVG painter: {error}");
            return;
        }
    };
    let full_src_rect = Rect::from_size(Size::new(bitmap.width() as f32, bitmap.height() as f32));
    let Some(clipped_src_rect) =
        map_destination_clip_to_source(full_src_rect, dst_rect, clipped_dst_rect)
    else {
        return;
    };
    scope.draw_image_src_sampled(
        bitmap,
        clipped_src_rect,
        clipped_dst_rect,
        alpha,
        color_filter,
        ImageSampling::Linear,
    );
}

#[cfg(feature = "svg")]
fn svg_raster_key(pixel_size: Size) -> Result<SvgRasterKey, SvgPainterError> {
    let width = svg_raster_axis(pixel_size.width)?;
    let height = svg_raster_axis(pixel_size.height)?;
    width
        .checked_mul(height)
        .and_then(|value| value.checked_mul(4))
        .ok_or(SvgPainterError::RasterDimensionsTooLarge)?;
    Ok(SvgRasterKey { width, height })
}

#[cfg(feature = "svg")]
fn svg_raster_axis(value: f32) -> Result<u32, SvgPainterError> {
    if !value.is_finite() || value <= 0.0 {
        return Err(SvgPainterError::InvalidRasterDimensions);
    }

    let rounded = value.ceil();
    if rounded > u32::MAX as f32 {
        return Err(SvgPainterError::RasterDimensionsTooLarge);
    }
    Ok(rounded as u32)
}

#[composable]
pub fn Image<P>(
    painter: P,
    content_description: Option<String>,
    modifier: Modifier,
    alignment: Alignment,
    content_scale: ContentScale,
    alpha: f32,
    color_filter: Option<ColorFilter>,
) -> NodeId
where
    P: Into<Painter> + Clone + PartialEq + 'static,
{
    let painter = painter.into();
    // Painter intrinsic units are logical dp. Bitmap painters use 1 source pixel
    // per dp; SVG painters rasterize at draw size and current density.
    let intrinsic_dp = painter.intrinsic_size();
    let draw_alpha = alpha.clamp(0.0, 1.0);
    let draw_painter = painter.clone();

    let semantics_modifier = if let Some(description) = content_description {
        Modifier::empty().semantics(move |config| {
            config.content_description = Some(description.clone());
        })
    } else {
        Modifier::empty()
    };

    let image_modifier =
        modifier
            .then(semantics_modifier)
            .draw_behind(move |scope: &mut dyn DrawScope| {
                if draw_alpha <= 0.0 {
                    return;
                }
                let container_size = scope.size();
                if container_size.width <= 0.0 || container_size.height <= 0.0 {
                    return;
                }
                match &draw_painter.kind {
                    PainterKind::Bitmap(bitmap) => draw_bitmap_painter(
                        scope,
                        bitmap.clone(),
                        intrinsic_dp,
                        alignment,
                        content_scale,
                        draw_alpha,
                        color_filter,
                    ),
                    PainterKind::BitmapRegion {
                        bitmap,
                        source,
                        sampling,
                    } => draw_bitmap_region_painter(
                        scope,
                        bitmap.clone(),
                        source.rect(),
                        alignment,
                        content_scale,
                        draw_alpha,
                        color_filter,
                        *sampling,
                    ),
                    PainterKind::BitmapTiled {
                        bitmap,
                        source,
                        sampling,
                    } => draw_bitmap_tiled_painter(
                        scope,
                        bitmap.clone(),
                        source.rect(),
                        draw_alpha,
                        color_filter,
                        *sampling,
                    ),
                    PainterKind::NinePatch {
                        bitmap,
                        source,
                        insets,
                        center,
                        edges,
                        sampling,
                    } => draw_nine_patch_painter(
                        scope,
                        bitmap.clone(),
                        source.rect(),
                        insets.insets(),
                        *center,
                        *edges,
                        draw_alpha,
                        color_filter,
                        *sampling,
                    ),
                    PainterKind::Svg(svg) => draw_svg_painter(
                        scope,
                        svg.clone(),
                        intrinsic_dp,
                        alignment,
                        content_scale,
                        draw_alpha,
                        color_filter,
                    ),
                }
            });

    Layout(
        image_modifier,
        ImageMeasurePolicy {
            intrinsic_size: intrinsic_dp,
        },
        || {},
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::core::Alignment;

    #[cfg(feature = "svg")]
    const RED_RECT_SVG: &[u8] = br##"
        <svg xmlns="http://www.w3.org/2000/svg" width="10" height="20" viewBox="0 0 10 20">
          <rect x="0" y="0" width="10" height="20" fill="#ff0000"/>
        </svg>
    "##;

    #[cfg(feature = "svg")]
    const TRANSPARENT_CENTER_SVG: &[u8] = br##"
        <svg xmlns="http://www.w3.org/2000/svg" width="4" height="4" viewBox="0 0 4 4">
          <rect x="1" y="1" width="2" height="2" fill="#00ff00"/>
        </svg>
    "##;

    fn sample_bitmap() -> ImageBitmap {
        ImageBitmap::from_rgba8(4, 2, vec![255; 4 * 2 * 4]).expect("bitmap")
    }

    #[test]
    fn svg_painter_ids_do_not_use_process_global_counter() {
        let source = include_str!("image.rs");
        let svg_counter = ["static ", "NEXT_SVG_PAINTER_ID"].concat();

        assert!(
            !source.contains(&svg_counter),
            "SVG painter ids must come from retained painter identity"
        );
    }

    #[cfg(feature = "svg")]
    fn cache_test_bitmap(width: u32) -> ImageBitmap {
        ImageBitmap::from_rgba8(width, 1, vec![255; width as usize * 4]).expect("bitmap")
    }

    #[cfg(feature = "svg")]
    fn pixel_at(bitmap: &ImageBitmap, x: u32, y: u32) -> [u8; 4] {
        let offset = ((y * bitmap.width() + x) * 4) as usize;
        let pixels = bitmap.pixels();
        [
            pixels[offset],
            pixels[offset + 1],
            pixels[offset + 2],
            pixels[offset + 3],
        ]
    }

    #[test]
    fn painter_reports_intrinsic_size_and_bitmap() {
        let bitmap = sample_bitmap();
        let painter = BitmapPainter(bitmap.clone());
        assert_eq!(painter.intrinsic_size(), Size::new(4.0, 2.0));
        assert_eq!(painter.bitmap(), Some(&bitmap));
    }

    #[test]
    fn bitmap_region_painter_uses_region_as_intrinsic_size() {
        let bitmap = sample_bitmap();
        let source = Rect {
            x: 1.0,
            y: 0.0,
            width: 2.0,
            height: 1.0,
        };
        let painter = BitmapRegionPainter(bitmap.clone(), source, ImageSampling::Nearest);
        assert_eq!(painter.intrinsic_size(), Size::new(2.0, 1.0));
        assert_eq!(painter.bitmap(), Some(&bitmap));
        assert_eq!(
            painter,
            Painter::from_bitmap_region(bitmap, source, ImageSampling::Nearest)
        );
    }

    fn atlas_bitmap() -> ImageBitmap {
        ImageBitmap::from_rgba8(64, 64, vec![255; 64 * 64 * 4]).expect("bitmap")
    }

    fn source_rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    fn drawn_images(
        size: Size,
        draw: impl FnOnce(&mut cranpose_ui_graphics::DrawScopeDefault),
    ) -> Vec<(Rect, Rect)> {
        let mut scope = cranpose_ui_graphics::DrawScopeDefault::new(size);
        draw(&mut scope);
        scope
            .into_primitives()
            .into_iter()
            .filter_map(|primitive| match primitive {
                cranpose_ui_graphics::DrawPrimitive::Image { src_rect, rect, .. } => {
                    Some((src_rect?, rect))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_tiled_painter_keeps_the_region_as_its_intrinsic_size() {
        let bitmap = atlas_bitmap();
        let source = source_rect(8.0, 8.0, 16.0, 4.0);
        let painter = TiledPainter(bitmap.clone(), source, ImageSampling::Nearest);
        assert_eq!(painter.intrinsic_size(), Size::new(16.0, 4.0));
        assert_eq!(painter.bitmap(), Some(&bitmap));
        assert_eq!(
            painter,
            Painter::from_bitmap_tiled(bitmap, source, ImageSampling::Nearest)
        );
    }

    #[test]
    fn a_tiled_painter_repeats_its_region_at_its_own_size() {
        let bitmap = atlas_bitmap();
        let source = source_rect(8.0, 8.0, 16.0, 4.0);
        let drawn = drawn_images(Size::new(32.0, 8.0), |scope| {
            draw_bitmap_tiled_painter(
                scope,
                bitmap.clone(),
                source,
                1.0,
                None,
                ImageSampling::Nearest,
            );
        });

        assert_eq!(drawn.len(), 4, "two columns by two rows");
        assert!(
            drawn
                .iter()
                .all(|(src, dst)| src.width == dst.width && src.height == dst.height),
            "a tile is drawn at 1:1, never scaled"
        );
        assert!(
            drawn.iter().all(|(src, _)| src.x >= 8.0
                && src.y >= 8.0
                && src.x + src.width <= 24.0
                && src.y + src.height <= 12.0),
            "a tile never reads outside the region it was given"
        );
        assert_eq!(drawn[0].1, source_rect(0.0, 0.0, 16.0, 4.0));
        assert_eq!(drawn[3].1, source_rect(16.0, 4.0, 16.0, 4.0));
    }

    #[test]
    fn a_tiled_painter_clips_the_last_row_and_column() {
        let bitmap = atlas_bitmap();
        let source = source_rect(0.0, 0.0, 10.0, 10.0);
        let drawn = drawn_images(Size::new(25.0, 10.0), |scope| {
            draw_bitmap_tiled_painter(
                scope,
                bitmap.clone(),
                source,
                1.0,
                None,
                ImageSampling::Nearest,
            );
        });
        assert_eq!(drawn.len(), 3);
        let (src, dst) = drawn[2];
        assert_eq!(dst, source_rect(20.0, 0.0, 5.0, 10.0));
        assert_eq!(src, source_rect(0.0, 0.0, 5.0, 10.0));
    }

    #[test]
    fn a_nine_patch_painter_keeps_the_region_as_its_intrinsic_size() {
        let bitmap = atlas_bitmap();
        let source = source_rect(0.0, 0.0, 30.0, 30.0);
        let painter = NinePatchPainter(
            bitmap.clone(),
            source,
            NinePatchInsets::uniform(10.0),
            PatchFill::Stretch,
            PatchFill::Stretch,
            ImageSampling::Nearest,
        );
        assert_eq!(painter.intrinsic_size(), Size::new(30.0, 30.0));
        assert_eq!(painter.bitmap(), Some(&bitmap));
        assert_eq!(
            painter,
            Painter::from_nine_patch(
                bitmap,
                source,
                NinePatchInsets::uniform(10.0),
                PatchFill::Stretch,
                PatchFill::Stretch,
                ImageSampling::Nearest,
            ),
            "two painters built from the same numbers are the same painter"
        );
    }

    #[test]
    fn a_nine_patch_painter_draws_its_corners_at_their_own_size() {
        let bitmap = atlas_bitmap();
        let drawn = drawn_images(Size::new(100.0, 60.0), |scope| {
            draw_nine_patch_painter(
                scope,
                bitmap.clone(),
                source_rect(0.0, 0.0, 30.0, 30.0),
                NinePatchInsets::uniform(10.0),
                PatchFill::Stretch,
                PatchFill::Stretch,
                1.0,
                None,
                ImageSampling::Nearest,
            );
        });

        assert_eq!(drawn.len(), 9);
        assert_eq!(
            drawn[0],
            (
                source_rect(0.0, 0.0, 10.0, 10.0),
                source_rect(0.0, 0.0, 10.0, 10.0),
            )
        );
        assert_eq!(
            drawn[8],
            (
                source_rect(20.0, 20.0, 10.0, 10.0),
                source_rect(90.0, 50.0, 10.0, 10.0),
            )
        );
        assert_eq!(
            drawn[4],
            (
                source_rect(10.0, 10.0, 10.0, 10.0),
                source_rect(10.0, 10.0, 80.0, 40.0),
            ),
            "the middle grows on both axes"
        );
    }

    #[test]
    fn a_nine_patch_painter_reads_only_its_region_of_an_atlas() {
        let bitmap = atlas_bitmap();
        let drawn = drawn_images(Size::new(80.0, 40.0), |scope| {
            draw_nine_patch_painter(
                scope,
                bitmap.clone(),
                source_rect(32.0, 16.0, 24.0, 24.0),
                NinePatchInsets::uniform(8.0),
                PatchFill::Tile,
                PatchFill::Tile,
                1.0,
                None,
                ImageSampling::Nearest,
            );
        });

        assert!(!drawn.is_empty());
        assert!(
            drawn.iter().all(|(src, _)| src.x >= 32.0
                && src.y >= 16.0
                && src.x + src.width <= 56.0
                && src.y + src.height <= 40.0),
            "no patch may read a neighbouring sprite out of the atlas"
        );
    }

    #[test]
    #[cfg(feature = "svg")]
    fn svg_painter_reports_intrinsic_size() {
        let painter = SvgPainter::from_bytes(RED_RECT_SVG).expect("svg painter");
        let clone = painter.clone();
        assert_eq!(painter.intrinsic_size(), Size::new(10.0, 20.0));
        assert_eq!(painter.id(), clone.id());
        assert_eq!(Painter::from_svg(painter).bitmap(), None);
    }

    #[test]
    #[cfg(feature = "svg")]
    fn svg_painter_rasterizes_requested_dimensions() {
        let painter = SvgPainter::from_bytes(RED_RECT_SVG).expect("svg painter");
        let bitmap = painter
            .rasterize(Size::new(5.0, 10.0))
            .expect("rasterized svg");

        assert_eq!(bitmap.width(), 5);
        assert_eq!(bitmap.height(), 10);
        assert_eq!(pixel_at(&bitmap, 2, 5), [255, 0, 0, 255]);
    }

    #[test]
    #[cfg(feature = "svg")]
    fn svg_painter_preserves_transparency() {
        let painter = SvgPainter::from_bytes(TRANSPARENT_CENTER_SVG).expect("svg painter");
        let bitmap = painter
            .rasterize(Size::new(4.0, 4.0))
            .expect("rasterized svg");

        assert_eq!(pixel_at(&bitmap, 0, 0), [0, 0, 0, 0]);
        assert_eq!(pixel_at(&bitmap, 2, 2), [0, 255, 0, 255]);
    }

    #[test]
    #[cfg(feature = "svg")]
    fn svg_painter_reuses_cached_raster_for_same_size() {
        let painter = SvgPainter::from_bytes(RED_RECT_SVG).expect("svg painter");
        let first = painter
            .rasterize(Size::new(8.0, 8.0))
            .expect("first raster");
        let second = painter
            .rasterize(Size::new(8.0, 8.0))
            .expect("second raster");

        assert_eq!(first.id(), second.id());
    }

    #[test]
    #[cfg(feature = "svg")]
    fn svg_raster_cache_evicts_least_recently_used_entry() {
        let mut cache = SvgRasterCache::default();
        let keys: Vec<SvgRasterKey> = (0..SVG_RASTER_CACHE_LIMIT)
            .map(|index| SvgRasterKey {
                width: index as u32 + 1,
                height: 1,
            })
            .collect();

        for key in &keys {
            cache.insert(*key, cache_test_bitmap(key.width));
        }

        let recent = cache.get(keys[0]).expect("cached raster");
        let new_key = SvgRasterKey {
            width: SVG_RASTER_CACHE_LIMIT as u32 + 1,
            height: 1,
        };
        cache.insert(new_key, cache_test_bitmap(new_key.width));

        assert!(cache.get(keys[1]).is_none());
        assert_eq!(
            cache.get(keys[0]).expect("retained raster").id(),
            recent.id()
        );
        assert!(cache.get(new_key).is_some());
    }

    #[test]
    #[cfg(feature = "svg")]
    fn svg_painter_rasterizes_distinct_sizes_separately() {
        let painter = SvgPainter::from_bytes(RED_RECT_SVG).expect("svg painter");
        let small = painter
            .rasterize(Size::new(8.0, 8.0))
            .expect("small raster");
        let large = painter
            .rasterize(Size::new(16.0, 16.0))
            .expect("large raster");

        assert_ne!(small.id(), large.id());
        assert_eq!(large.width(), 16);
        assert_eq!(large.height(), 16);
    }

    #[test]
    #[cfg(feature = "svg")]
    fn svg_painter_cache_recovers_after_poison() {
        let painter = SvgPainter::from_bytes(RED_RECT_SVG).expect("svg painter");
        let poison_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = painter
                .inner
                .cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            panic!("poison svg raster cache for recovery test");
        }));

        assert!(poison_result.is_err());

        let bitmap = painter
            .rasterize(Size::new(8.0, 8.0))
            .expect("svg raster cache should recover after poisoning");
        assert_eq!(bitmap.width(), 8);
        assert_eq!(bitmap.height(), 8);
    }

    #[test]
    fn svg_draw_density_has_no_outside_context_fallback() {
        let source = include_str!("image.rs");
        let fallback_call = ["try_current", "_density()", ".unwrap_or"].concat();
        assert!(
            !source.contains(&fallback_call),
            "SVG image drawing must use the active AppContext density"
        );
    }

    #[test]
    #[cfg(feature = "svg")]
    fn svg_painter_rejects_invalid_bytes() {
        let err = SvgPainter::from_bytes(b"not svg").expect_err("invalid svg");
        assert!(matches!(err, SvgPainterError::Parse(_)));
    }

    #[test]
    #[cfg(not(feature = "svg"))]
    fn svg_painter_reports_disabled_feature_without_resvg() {
        let err = SvgPainter::from_bytes(b"not svg").expect_err("svg feature disabled");
        assert!(matches!(err, SvgPainterError::SvgFeatureDisabled));
    }

    #[test]
    fn fit_keeps_aspect_ratio() {
        let src = Size::new(200.0, 100.0);
        let dst = Size::new(300.0, 300.0);
        let result = ContentScale::Fit.scaled_size(src, dst);
        assert_eq!(result, Size::new(300.0, 150.0));
    }

    #[test]
    fn crop_fills_bounds() {
        let src = Size::new(200.0, 100.0);
        let dst = Size::new(300.0, 300.0);
        let result = ContentScale::Crop.scaled_size(src, dst);
        assert_eq!(result, Size::new(600.0, 300.0));
    }

    #[test]
    fn destination_rect_aligns_center() {
        let src = Size::new(200.0, 100.0);
        let dst = Size::new(300.0, 300.0);
        let rect = destination_rect(src, dst, Alignment::CENTER, ContentScale::Fit);
        assert_eq!(
            rect,
            Rect {
                x: 0.0,
                y: 75.0,
                width: 300.0,
                height: 150.0,
            }
        );
    }

    #[test]
    fn crop_destination_clip_maps_centered_wide_source() {
        let src = Size::new(200.0, 100.0);
        let dst = Size::new(100.0, 100.0);
        let (dst_rect, clipped_dst_rect) =
            image_destination_clip(src, dst, Alignment::CENTER, ContentScale::Crop)
                .expect("destination clip");
        let rect = map_destination_clip_to_source(Rect::from_size(src), dst_rect, clipped_dst_rect)
            .expect("source clip");
        assert_eq!(
            rect,
            Rect {
                x: 50.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            }
        );
    }

    #[test]
    fn crop_destination_clip_honors_start_alignment() {
        let src = Size::new(200.0, 100.0);
        let dst = Size::new(100.0, 100.0);
        let (dst_rect, clipped_dst_rect) =
            image_destination_clip(src, dst, Alignment::TOP_START, ContentScale::Crop)
                .expect("destination clip");
        let rect = map_destination_clip_to_source(Rect::from_size(src), dst_rect, clipped_dst_rect)
            .expect("source clip");
        assert_eq!(
            rect,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            }
        );
    }

    fn approx_eq(left: f32, right: f32) {
        assert!((left - right).abs() < 1e-4, "left={left}, right={right}");
    }

    #[test]
    fn map_destination_clip_to_source_scales_proportionally() {
        let src = Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };
        let dst = Rect {
            x: -50.0,
            y: 0.0,
            width: 200.0,
            height: 100.0,
        };
        let clipped_dst = Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };
        let mapped = map_destination_clip_to_source(src, dst, clipped_dst).expect("mapped");
        approx_eq(mapped.x, 25.0);
        approx_eq(mapped.y, 0.0);
        approx_eq(mapped.width, 50.0);
        approx_eq(mapped.height, 100.0);
    }

    #[test]
    fn map_destination_clip_to_source_returns_full_source_without_clipping() {
        let src = Rect {
            x: 0.0,
            y: 0.0,
            width: 120.0,
            height: 80.0,
        };
        let dst = Rect {
            x: 10.0,
            y: 5.0,
            width: 60.0,
            height: 40.0,
        };
        let mapped = map_destination_clip_to_source(src, dst, dst).expect("mapped");
        approx_eq(mapped.x, src.x);
        approx_eq(mapped.y, src.y);
        approx_eq(mapped.width, src.width);
        approx_eq(mapped.height, src.height);
    }

    // --- ImageMeasurePolicy tests ---

    fn measure_image(intrinsic: Size, constraints: Constraints) -> Size {
        let policy = ImageMeasurePolicy {
            intrinsic_size: intrinsic,
        };
        let scope = crate::density::DensityMeasureScope::new(crate::density::Density::default());
        policy.measure(&scope, &[], constraints).size
    }

    #[test]
    fn image_measure_unconstrained() {
        let size = measure_image(
            Size::new(800.0, 600.0),
            Constraints::loose(f32::INFINITY, f32::INFINITY),
        );
        assert_eq!(size, Size::new(800.0, 600.0));
    }

    #[test]
    fn image_measure_width_constrained_preserves_aspect_ratio() {
        // 800×600 constrained to max_width=400 → should scale to 400×300
        let size = measure_image(
            Size::new(800.0, 600.0),
            Constraints::loose(400.0, f32::INFINITY),
        );
        assert_eq!(size, Size::new(400.0, 300.0));
    }

    #[test]
    fn image_measure_height_constrained_preserves_aspect_ratio() {
        // 800×600 constrained to max_height=300 → should scale to 400×300
        let size = measure_image(
            Size::new(800.0, 600.0),
            Constraints::loose(f32::INFINITY, 300.0),
        );
        assert_eq!(size, Size::new(400.0, 300.0));
    }

    #[test]
    fn image_measure_both_constrained_uses_smaller_factor() {
        // 800×600 constrained to 200×400 → width is the bottleneck (0.25)
        // scaled: 200×150
        let size = measure_image(Size::new(800.0, 600.0), Constraints::loose(200.0, 400.0));
        assert_eq!(size, Size::new(200.0, 150.0));
    }

    #[test]
    fn image_measure_fits_within_constraints() {
        // 200×100 in a 400×400 container → stays at intrinsic size
        let size = measure_image(Size::new(200.0, 100.0), Constraints::loose(400.0, 400.0));
        assert_eq!(size, Size::new(200.0, 100.0));
    }

    #[test]
    fn image_measure_zero_intrinsic() {
        let size = measure_image(Size::ZERO, Constraints::loose(400.0, 400.0));
        assert_eq!(size, Size::new(0.0, 0.0));
    }
}
