//! A laid-out text's glyphs rasterized into pixels: an image of the text's
//! own, or a renderer's pixels painted over directly.

use cranpose_text::TextLayout;
use cranpose_ui::{
    Color,
    text::{TextMotion, TextStyle},
};
use cranpose_ui_graphics::{Point, Rect, whole};

use crate::{
    glyph_mask::GlyphMaskCache,
    text_canvas::{CanvasPaint, PlacedMask, SolidInk, TextCanvas, TextPixelWindow, TextPixels},
    text_glyphs::{SpanPaint, SpanPaints, StyledTextRef, visit_placed_glyphs},
    text_shadow::shadow_blur_sigma,
};

// Masks a raster keeps when the renderer's cache is in use elsewhere.
const SCRATCH_MASKS: usize = 64;

/// Whether a raster paints a text's glyphs, or only the shadows under them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GlyphInk {
    /// The glyphs, and the shadows of spans that cast one.
    #[default]
    Painted,
    /// Only the shadows, as the shadow a style casts under its text draws.
    Hidden,
}

/// What to rasterize: `layout`, the lines of `text`, its top left corner at
/// `origin` and its pixels cut to `rect`, both in device pixels, `scale` of
/// them a layout pixel. Glyphs whose style names no color paint in
/// `fallback_color`.
#[derive(Clone, Copy)]
pub struct TextRasterRequest<'a> {
    /// The lines and glyphs.
    pub layout: &'a TextLayout,
    /// The text and its span styles.
    pub text: StyledTextRef<'a>,
    /// The part of the text the pixels cover.
    pub rect: Rect,
    /// Where the text's top left corner sits.
    pub origin: Point,
    /// The style of the whole text, under its span styles.
    pub style: &'a TextStyle,
    /// The color of text whose style names none.
    pub fallback_color: Color,
    /// Device pixels per layout pixel.
    pub scale: f32,
    /// Whether the glyphs paint or only their shadows.
    pub ink: GlyphInk,
}

impl<'a> TextRasterRequest<'a> {
    /// `layout` of `text` with its top left corner at the corner of `rect`,
    /// which it covers whole.
    pub fn new(
        layout: &'a TextLayout,
        text: impl Into<StyledTextRef<'a>>,
        rect: Rect,
        style: &'a TextStyle,
        fallback_color: Color,
        scale: f32,
    ) -> Self {
        Self {
            layout,
            text: text.into(),
            rect,
            origin: Point::new(rect.x, rect.y),
            style,
            fallback_color,
            scale,
            ink: GlyphInk::Painted,
        }
    }

    /// The request with the text's top left corner at `origin`.
    pub fn with_origin(self, origin: Point) -> Self {
        Self { origin, ..self }
    }

    /// The request painting with `ink`.
    pub fn with_ink(self, ink: GlyphInk) -> Self {
        Self { ink, ..self }
    }

    pub(crate) fn is_degenerate(&self) -> bool {
        self.text.is_empty()
            || self.rect.width <= 0.0
            || self.rect.height <= 0.0
            || self.scale <= 0.0
            || !self.scale.is_finite()
    }

    // How the glyphs move: kept to whole device pixels when static.
    pub(crate) fn motion(&self) -> TextMotion {
        self.style
            .paragraph_style
            .text_motion
            .unwrap_or(TextMotion::Static)
    }

    // The box a brush is sampled across: the whole layout.
    fn brush_rect(&self) -> Rect {
        Rect {
            x: self.origin.x,
            y: self.origin.y,
            width: self.layout.width() * self.scale,
            height: self.layout.height() * self.scale,
        }
    }
}

/// `request`'s text rasterized into an image of its own covering
/// `request.rect`, straight alpha. `None` for an empty text or rect.
pub fn rasterize_text(
    request: TextRasterRequest<'_>,
    glyph_cache: Option<&mut GlyphMaskCache>,
) -> Option<TextPixels> {
    if request.is_degenerate() {
        return None;
    }
    let paints = SpanPaints::of(request.text, request.style, request.fallback_color);
    let solid = request.ink == GlyphInk::Painted
        && paints.all(|paint| paint.solid_color().is_some() && paint.shadow.is_none());
    let width = whole::to_i32(whole::ceil(request.rect.width)).max(1);
    let height = whole::to_i32(whole::ceil(request.rect.height)).max(1);
    let (width, height) = (u32::try_from(width).ok()?, u32::try_from(height).ok()?);
    let mut canvas = if solid {
        TextCanvas::solid(width, height)
    } else {
        TextCanvas::blended(width, height)
    };
    paint_glyphs(request, &paints, glyph_cache, &mut canvas);
    canvas.into_pixels()
}

/// Paints `request`'s glyphs straight over `window`, which covers
/// `request.rect`, as the text's own image would draw there. Returns false,
/// painting nothing, for a text that needs an image of its own: one in
/// motion, or one with a gradient or a shadow.
pub fn paint_text(
    request: TextRasterRequest<'_>,
    glyph_cache: Option<&mut GlyphMaskCache>,
    window: TextPixelWindow<'_>,
) -> bool {
    if request.is_degenerate() {
        return true;
    }
    let paints = SpanPaints::of(request.text, request.style, request.fallback_color);
    if request.motion() != TextMotion::Static
        || request.ink == GlyphInk::Hidden
        || !paints.all(|paint| paint.solid_color().is_some() && paint.shadow.is_none())
    {
        return false;
    }
    paint_glyphs(request, &paints, glyph_cache, &mut TextCanvas::over(window));
    true
}

// Paints each glyph of `request` over `canvas`, which covers
// `request.rect`, its mask from `glyph_cache` or from a cache of the call's
// own when that is in use.
fn paint_glyphs<'a>(
    request: TextRasterRequest<'a>,
    paints: &'a SpanPaints,
    glyph_cache: Option<&mut GlyphMaskCache>,
    canvas: &mut TextCanvas<'_>,
) {
    let mut scratch = None;
    let cache = match glyph_cache {
        Some(cache) => cache,
        None => scratch.insert(GlyphMaskCache::with_capacity_at_least_one(SCRATCH_MASKS)),
    };
    let motion = request.motion();
    let (rect, scale) = (request.rect, request.scale);
    let ink = match request.ink {
        GlyphInk::Painted => 1.0,
        GlyphInk::Hidden => 0.0,
    };
    let brush_rect = request.brush_rect();
    let area = (canvas.width, canvas.height);
    let canvas_paint = |span: &'a SpanPaint| {
        let alpha = span.alpha * ink;
        CanvasPaint {
            brush: &span.brush,
            alpha,
            solid: SolidInk::of(&span.brush, alpha),
            shadow: span.shadow,
            brush_rect,
            canvas_origin: Point::new(rect.x, rect.y),
            scale,
            static_motion: motion == TextMotion::Static,
        }
    };
    // How far past a glyph's mask its shadow reaches, in pixels.
    let margin = |span: &SpanPaint| {
        span.shadow.map_or(0, |shadow| {
            let reach = shadow.offset.x.abs().max(shadow.offset.y.abs()) * scale;
            let blur = shadow_blur_sigma((shadow.blur_radius * scale).max(0.0)) * 3.0;
            whole::to_i32(whole::ceil(reach + blur)) + 1
        })
    };
    // The span paint of the glyph before as the canvas paints it, and its
    // margin: a text's glyphs mostly share one.
    let mut last: Option<(&'a SpanPaint, CanvasPaint<'a>, i32)> = None;
    visit_placed_glyphs(
        request.layout,
        paints,
        (request.origin, scale),
        motion,
        |glyph| {
            let Some(mask) = cache.mask(glyph.typeface, glyph.glyph, glyph.size, glyph.raster)
            else {
                return;
            };
            let placed = PlacedMask {
                mask,
                x: whole::to_i32(whole::round(glyph.x - rect.x)) + mask.left,
                y: whole::to_i32(whole::round(glyph.y - rect.y)) + mask.top,
            };
            let (_, paint, margin) = match &last {
                Some(found) if std::ptr::eq(found.0, glyph.paint) => found,
                _ => &*last.insert((glyph.paint, canvas_paint(glyph.paint), margin(glyph.paint))),
            };
            if overlaps(placed, area, *margin) {
                canvas.paint(placed, paint);
            }
        },
    );
}

// Whether `placed`, grown by `margin` pixels, overlaps a canvas `area`.
fn overlaps(placed: PlacedMask<'_>, (width, height): (u32, u32), margin: i32) -> bool {
    let size = |value: usize| i32::try_from(value).unwrap_or(i32::MAX);
    let extent = |value: u32| i32::try_from(value).unwrap_or(i32::MAX);
    placed.x - margin < extent(width)
        && placed.y - margin < extent(height)
        && placed.x + size(placed.mask.width) + margin > 0
        && placed.y + size(placed.mask.height) + margin > 0
}
