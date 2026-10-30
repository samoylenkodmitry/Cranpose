//! Glyph masks: the coverage of a glyph at one size, drawn filled or stroked
//! and with the weight or slant a face lacks synthesized, and the cache the
//! renderers keep them in.

use std::sync::Arc;

use ab_glyph_rasterizer::Rasterizer;
use cranpose_core::collections::bounded_lru::BoundedLruCache;
use cranpose_text::{GlyphId, GlyphOutline, OutlineSegment, OutlineTransform, Typeface};
use cranpose_ui_graphics::whole;
use tiny_skia::{LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform};

use crate::text_mask_gamma::unit_to_u8;

// Compose's stroke miter limit for stroked text.
const STROKE_MITER_LIMIT: f32 = 4.0;

/// How a glyph is drawn: filled, or stroked `stroke` device pixels wide,
/// grown by `embolden` device pixels for a synthesized bold and slanted by
/// `skew` for a synthesized italic.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GlyphRaster {
    /// The stroke width of stroked text, `None` for filled.
    pub stroke: Option<f32>,
    /// How much a synthesized bold widens each stroke of the glyph, 0 for
    /// none: the stroke Skia's fake bold adds around the outline.
    pub embolden: f32,
    /// How far right a unit of height moves the outline, 0 for none.
    pub skew: f32,
}

/// Identifies one glyph mask: the face, the glyph, its size and how it is
/// drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GlyphMaskKey {
    /// The face's [`Typeface::content_hash`].
    pub font_hash: u64,
    /// The glyph.
    pub glyph_id: u32,
    /// Bits of the size in device pixels per em.
    pub size_bits: u32,
    /// Bits of the stroke width, 0 for a filled glyph.
    pub stroke_bits: u32,
    /// Bits of the synthesized bold's growth.
    pub embolden_bits: u32,
    /// Bits of the synthesized italic's skew.
    pub skew_bits: u32,
}

impl GlyphMaskKey {
    /// The key of `glyph` of `typeface` at `size` device pixels per em drawn
    /// as `raster` says.
    pub fn of(typeface: &Typeface, glyph: GlyphId, size: f32, raster: GlyphRaster) -> Self {
        Self {
            font_hash: typeface.content_hash(),
            glyph_id: glyph.0,
            size_bits: size.to_bits(),
            stroke_bits: raster.stroke.map_or(0, f32::to_bits),
            embolden_bits: raster.embolden.to_bits(),
            skew_bits: raster.skew.to_bits(),
        }
    }

    /// Whether the glyph is filled with no synthesis, as a glyph atlas holds it.
    pub fn is_plain_fill(self) -> bool {
        self.stroke_bits == 0 && self.embolden_bits == 0 && self.skew_bits == 0
    }
}

/// A glyph's coverage: a byte a pixel, row by row, 255 covering the pixel
/// whole, and where its box sits from the glyph's origin.
#[derive(Clone, Debug, PartialEq)]
pub struct GlyphMask {
    /// `width` times `height` coverage bytes.
    pub alpha: Arc<[u8]>,
    /// The box's width in pixels.
    pub width: usize,
    /// The box's height in pixels.
    pub height: usize,
    /// How far right of the glyph's origin the box starts.
    pub left: i32,
    /// How far below the baseline the box starts; negative above it.
    pub top: i32,
}

/// The mask of `glyph` of `typeface` at `size` device pixels per em drawn as
/// `raster` says, its origin on a whole pixel. `None` for a glyph that
/// covers no pixel, such as a space.
pub fn rasterize_glyph(
    typeface: &Typeface,
    glyph: GlyphId,
    size: f32,
    raster: GlyphRaster,
    rasterizer: &mut Rasterizer,
) -> Option<GlyphMask> {
    let transform = OutlineTransform {
        size,
        skew: raster.skew,
        ..OutlineTransform::default()
    };
    let outline = GlyphOutline::of(typeface, glyph, transform)?;
    let stroked = match raster.stroke {
        Some(width) if width.is_finite() && width > 0.0 => Some((width, StrokeKind::Stroke)),
        _ if raster.embolden > 0.0 => Some((raster.embolden, StrokeKind::StrokeAndFill)),
        _ => None,
    };
    match stroked {
        Some((width, kind)) => stroked_mask(&outline, width, kind),
        None => filled_mask(&outline, rasterizer),
    }
}

fn filled_mask(outline: &GlyphOutline, rasterizer: &mut Rasterizer) -> Option<GlyphMask> {
    let (width, height) = (usize_of(outline.width()), usize_of(outline.height()));
    if width == 0 || height == 0 {
        return None;
    }
    let mut alpha = vec![0u8; width * height];
    outline.draw(rasterizer, |x, y, coverage| {
        if let Some(pixel) = alpha.get_mut(usize_of(y) * width + usize_of(x)) {
            *pixel = unit_to_u8(coverage);
        }
    });
    Some(GlyphMask {
        alpha: alpha.into(),
        width,
        height,
        left: outline.left(),
        top: outline.top(),
    })
}

#[derive(Clone, Copy)]
enum StrokeKind {
    // The outline stroked, as `TextDrawStyle::Stroke` draws it.
    Stroke,
    // The outline filled and stroked, as Skia's fake bold draws it.
    StrokeAndFill,
}

// The outline stroked `width` wide, with the fill under it for
// `StrokeAndFill`, its box grown to hold the stroke.
fn stroked_mask(outline: &GlyphOutline, width: f32, kind: StrokeKind) -> Option<GlyphMask> {
    let pad = whole::to_i32(whole::ceil(width * 0.5 * STROKE_MITER_LIMIT)).max(1) + 1;
    let raster_width = outline.width() + 2 * u32::try_from(pad).ok()?;
    let raster_height = outline.height() + 2 * u32::try_from(pad).ok()?;
    let mut pixmap = Pixmap::new(raster_width, raster_height)?;
    let path = outline_path(outline, f32_of(pad))?;
    let mut paint = Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    paint.anti_alias = true;
    if let StrokeKind::StrokeAndFill = kind {
        pixmap.fill_path(
            &path,
            &paint,
            tiny_skia::FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
    let stroke = Stroke {
        width,
        line_cap: LineCap::Butt,
        line_join: LineJoin::Miter,
        miter_limit: STROKE_MITER_LIMIT,
        ..Stroke::default()
    };
    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    let alpha: Arc<[u8]> = pixmap
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|pixel| pixel[3])
        .collect();
    Some(GlyphMask {
        alpha,
        width: usize_of(raster_width),
        height: usize_of(raster_height),
        left: outline.left() - pad,
        top: outline.top() - pad,
    })
}

// The outline as a path moved `pad` pixels right and down, each contour
// closed.
fn outline_path(outline: &GlyphOutline, pad: f32) -> Option<tiny_skia::Path> {
    let mut builder = PathBuilder::new();
    let mut pen = None;
    let moved = |point: cranpose_ui_graphics::Point| (point.x + pad, point.y + pad);
    for segment in outline.segments() {
        let (start, end) = match segment {
            OutlineSegment::Line(a, b) | OutlineSegment::Quad(a, _, b) => (a, b),
            OutlineSegment::Cubic(a, _, _, b) => (a, b),
        };
        if pen != Some(start) {
            if pen.is_some() {
                builder.close();
            }
            let (x, y) = moved(start);
            builder.move_to(x, y);
        }
        match segment {
            OutlineSegment::Line(_, b) => {
                let (x, y) = moved(b);
                builder.line_to(x, y);
            }
            OutlineSegment::Quad(_, c, b) => {
                let ((cx, cy), (x, y)) = (moved(c), moved(b));
                builder.quad_to(cx, cy, x, y);
            }
            OutlineSegment::Cubic(_, c0, c1, b) => {
                let ((ax, ay), (bx, by), (x, y)) = (moved(c0), moved(c1), moved(b));
                builder.cubic_to(ax, ay, bx, by, x, y);
            }
        }
        pen = Some(end);
    }
    builder.close();
    builder.finish()
}

/// Counters of a [`GlyphMaskCache`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GlyphMaskCacheStats {
    /// Masks held.
    pub entries: usize,
    /// Lookups that found their mask.
    pub hits: u64,
    /// Lookups that drew their mask.
    pub misses: u64,
}

/// The masks glyphs were drawn with lately, so a glyph drawn again is not
/// rasterized again. A glyph that covers no pixel is kept as such.
pub struct GlyphMaskCache {
    masks: BoundedLruCache<GlyphMaskKey, Option<GlyphMask>>,
    rasterizer: Rasterizer,
    hits: u64,
    misses: u64,
}

impl GlyphMaskCache {
    /// A cache that holds up to `capacity` masks, and at least one.
    pub fn with_capacity_at_least_one(capacity: usize) -> Self {
        Self {
            masks: BoundedLruCache::with_capacity_at_least_one(capacity),
            rasterizer: Rasterizer::new(0, 0),
            hits: 0,
            misses: 0,
        }
    }

    /// Current counters.
    pub fn stats(&self) -> GlyphMaskCacheStats {
        GlyphMaskCacheStats {
            entries: self.masks.len(),
            hits: self.hits,
            misses: self.misses,
        }
    }

    /// The mask of `glyph` of `typeface` at `size` device pixels per em drawn
    /// as `raster` says, drawn now when the cache lacks it. `None` for a
    /// glyph that covers no pixel.
    pub fn mask(
        &mut self,
        typeface: &Typeface,
        glyph: GlyphId,
        size: f32,
        raster: GlyphRaster,
    ) -> Option<&GlyphMask> {
        let key = GlyphMaskKey::of(typeface, glyph, size, raster);
        if self.masks.contains(&key) {
            self.hits = self.hits.saturating_add(1);
        } else {
            self.draw(key, typeface, glyph, (size, raster));
        }
        self.masks.get(&key)?.as_ref()
    }

    // Draws the mask the cache lacks and keeps it under `key`. Out of line:
    // a frame mostly draws its glyphs from the cache, and a build that keeps
    // its drawing in fast memory keeps the lookup without this.
    #[inline(never)]
    fn draw(
        &mut self,
        key: GlyphMaskKey,
        typeface: &Typeface,
        glyph: GlyphId,
        (size, raster): (f32, GlyphRaster),
    ) {
        self.misses = self.misses.saturating_add(1);
        let mask = rasterize_glyph(typeface, glyph, size, raster, &mut self.rasterizer);
        self.masks.put(key, mask);
    }

    /// The mask under `key` if the cache holds it.
    pub fn cached(&mut self, key: &GlyphMaskKey) -> Option<&GlyphMask> {
        self.masks.get(key)?.as_ref()
    }
}

fn usize_of(value: u32) -> usize {
    usize::try_from(value).unwrap_or(0)
}

fn f32_of(value: i32) -> f32 {
    f32::from(i16::try_from(value).unwrap_or(i16::MAX))
}
