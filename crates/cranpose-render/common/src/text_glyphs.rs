//! The glyphs of a laid-out text placed on a renderer's pixels, each with
//! the paint of the span it draws.

use std::ops::Range;

use cranpose_text::{FontSynthesis, GlyphId, TextLayout, Typeface};
use cranpose_ui::{
    Brush, Color,
    text::{
        FontStyle, FontWeight, RangeStyle, RenderString, Shadow, SpanBoundaries, SpanStyle,
        TextDrawStyle, TextStyle,
    },
};
use cranpose_ui_graphics::{Point, whole};
use smallvec::SmallVec;

use crate::glyph_mask::GlyphRaster;

/// A text and the styles of its ranges.
#[derive(Clone, Copy, Debug)]
pub struct StyledTextRef<'a> {
    /// The text.
    pub text: &'a str,
    /// Styles of byte ranges of `text`.
    pub span_styles: &'a [RangeStyle<SpanStyle>],
}

impl StyledTextRef<'_> {
    pub(crate) fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    // The offsets where a span starts or ends, with the text's ends.
    fn span_boundaries(&self) -> SpanBoundaries {
        let mut boundaries: SpanBoundaries = smallvec::smallvec![0, self.text.len()];
        for span in self.span_styles {
            boundaries.push(span.range.start);
            boundaries.push(span.range.end);
        }
        boundaries.sort_unstable();
        boundaries.dedup();
        boundaries.retain(|at| *at <= self.text.len() && self.text.is_char_boundary(*at));
        boundaries
    }
}

impl<'a> From<&'a str> for StyledTextRef<'a> {
    fn from(text: &'a str) -> Self {
        Self {
            text,
            span_styles: &[],
        }
    }
}

impl<'a> From<&'a cranpose_ui::text::AnnotatedString> for StyledTextRef<'a> {
    fn from(text: &'a cranpose_ui::text::AnnotatedString) -> Self {
        Self {
            text: text.text.as_str(),
            span_styles: &text.span_styles,
        }
    }
}

impl<'a> From<&'a RenderString> for StyledTextRef<'a> {
    fn from(text: &'a RenderString) -> Self {
        Self {
            text: text.text(),
            span_styles: text.span_styles(),
        }
    }
}

/// Calls `visit` with each stretch of `text` between its span boundaries
/// and the span styles over it merged over `style`'s, once for an empty
/// text.
pub(crate) fn visit_span_styles(
    text: StyledTextRef<'_>,
    style: &TextStyle,
    mut visit: impl FnMut(Range<usize>, &SpanStyle),
) {
    if text.span_styles.is_empty() || text.text.is_empty() {
        visit(0..text.text.len(), &style.span_style);
        return;
    }
    for edges in text.span_boundaries().windows(2) {
        let range = edges[0]..edges[1];
        let merged = text
            .span_styles
            .iter()
            .filter(|span| span.range.start <= range.start && range.end <= span.range.end)
            .fold(style.span_style.clone(), |merged, span| {
                merged.merge(&span.item)
            });
        visit(range, &merged);
    }
}

/// What a span of a text paints its glyphs with.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SpanPaint {
    pub(crate) brush: Brush,
    // What the brush's alpha is multiplied by.
    pub(crate) alpha: f32,
    pub(crate) shadow: Option<Shadow>,
    // The stroke width of stroked text, in layout pixels.
    pub(crate) stroke: Option<f32>,
    weight: FontWeight,
    style: FontStyle,
    synthesis: FontSynthesis,
}

impl SpanPaint {
    // The paint of a span in `style`, glyphs whose style names no color in
    // `fallback`.
    pub(crate) fn of(style: &SpanStyle, fallback: Color) -> Self {
        let (brush, alpha) = match &style.brush {
            Some(brush) => (brush.clone(), style.alpha.unwrap_or(1.0).clamp(0.0, 1.0)),
            None => (Brush::solid(style.resolve_foreground_color(fallback)), 1.0),
        };
        Self {
            brush,
            alpha,
            shadow: style.shadow.filter(|shadow| shadow.color.3 > 0.0),
            stroke: match style.draw_style {
                Some(TextDrawStyle::Stroke { width }) if width.is_finite() && width > 0.0 => {
                    Some(width)
                }
                _ => None,
            },
            weight: style.font_weight.unwrap_or_default(),
            style: style.font_style.unwrap_or_default(),
            synthesis: style.font_synthesis.unwrap_or(FontSynthesis::All),
        }
    }

    // The color of a solid paint, `None` for a gradient.
    pub(crate) fn solid_color(&self) -> Option<Color> {
        match self.brush {
            Brush::Solid(color) => Some(Color(color.0, color.1, color.2, color.3 * self.alpha)),
            _ => None,
        }
    }

    // How a glyph of `typeface` at `size` device pixels per em paints with
    // this paint at `scale` device pixels a layout pixel: stroked, and with
    // the bold and italic Compose synthesizes for a face that lacks them.
    pub(crate) fn raster(&self, typeface: &Typeface, size: f32, scale: f32) -> GlyphRaster {
        let synthesis = typeface.synthesis(self.weight, self.style, self.synthesis);
        GlyphRaster {
            stroke: self.stroke.map(|width| width * scale),
            embolden: if synthesis.bold {
                size * fake_bold_ratio(size)
            } else {
                0.0
            },
            skew: if synthesis.italic {
                FAKE_ITALIC_SKEW
            } else {
                0.0
            },
        }
    }
}

// Skia's fake italic: Android's `setTextSkewX(-0.25)`.
const FAKE_ITALIC_SKEW: f32 = 0.25;

// How wide Skia's fake bold strokes a glyph, as a share of its size: 1/24
// at 9 pixels to 1/32 at 36, constant past either end.
fn fake_bold_ratio(size: f32) -> f32 {
    let along = ((size - 9.0) / 27.0).clamp(0.0, 1.0);
    1.0 / 24.0 + along * (1.0 / 32.0 - 1.0 / 24.0)
}

/// The paints of a text's spans, by the text they cover.
pub(crate) struct SpanPaints {
    paints: SmallVec<[(Range<usize>, SpanPaint); 2]>,
}

impl SpanPaints {
    // The paints of `text` in `style`, a span's style over the text's.
    pub(crate) fn of(text: StyledTextRef<'_>, style: &TextStyle, fallback: Color) -> Self {
        let mut paints: SmallVec<[(Range<usize>, SpanPaint); 2]> = SmallVec::new();
        visit_span_styles(text, style, |range, merged| {
            let paint = SpanPaint::of(merged, fallback);
            match paints.last_mut() {
                Some((last, last_paint)) if *last_paint == paint => last.end = range.end,
                _ => paints.push((range, paint)),
            }
        });
        Self { paints }
    }

    // The paint of the character at `offset`.
    pub(crate) fn at(&self, offset: usize) -> &SpanPaint {
        let at = self
            .paints
            .partition_point(|(range, _)| range.end <= offset)
            .min(self.paints.len() - 1);
        &self.paints[at].1
    }

    // Whether every span paints with `test` true.
    pub(crate) fn all(&self, test: impl Fn(&SpanPaint) -> bool) -> bool {
        self.paints.iter().all(|(_, paint)| test(paint))
    }
}

/// A glyph of a text placed on the device's pixels.
pub(crate) struct PlacedGlyph<'a> {
    pub(crate) typeface: &'a Typeface,
    pub(crate) glyph: GlyphId,
    // Device pixels per em.
    pub(crate) size: f32,
    pub(crate) raster: GlyphRaster,
    // The glyph's origin on the device's pixels: whole pixels for a static
    // text, whole pixels from `origin` for one in motion.
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) paint: &'a SpanPaint,
}

/// Calls `visit` with each glyph `layout` draws, its top left corner at
/// `origin` in device pixels, `scale` device pixels a layout pixel, in the
/// paint `paints` give it.
pub(crate) fn visit_placed_glyphs<'a>(
    layout: &'a TextLayout,
    paints: &'a SpanPaints,
    (origin, scale): (Point, f32),
    static_motion: bool,
    mut visit: impl FnMut(PlacedGlyph<'a>),
) {
    let place = |value: f32, from: f32| {
        if static_motion {
            whole::round(from + value * scale)
        } else {
            from + whole::round(value * scale)
        }
    };
    for line in 0..layout.line_count() {
        for run in layout.glyph_runs(line) {
            let size = run.size * scale;
            for glyph in run.positions() {
                let paint = paints.at(glyph.offset);
                visit(PlacedGlyph {
                    typeface: run.typeface,
                    glyph: glyph.id,
                    size,
                    raster: paint.raster(run.typeface, size, scale),
                    x: place(glyph.x, origin.x),
                    y: place(glyph.y, origin.y),
                    paint,
                });
            }
        }
    }
}
