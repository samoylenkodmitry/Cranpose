//! Where a laid-out text's glyphs draw: across the visible text of its
//! lines, and wherever the ink of a glyph reaches past them.

use cranpose_text::{FontSynthesis, TextLayout};
use cranpose_ui::text::{FontStyle, FontWeight, RangeStyle, SpanStyle, TextDrawStyle, TextStyle};
use cranpose_ui_graphics::{Rect, whole};

use crate::{
    glyph_mask::STROKE_MITER_LIMIT,
    text_glyphs::{FAKE_BOLD_MAX_RATIO, FAKE_ITALIC_SKEW},
};

/// What a text's style and spans set that puts ink past the outlines of its
/// glyphs: the widest stroke, the heaviest weight and an italic, which a face
/// that lacks them may synthesize.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct InkStyle {
    // In layout pixels.
    stroke: f32,
    weight: FontWeight,
    italic: bool,
}

impl InkStyle {
    /// What `style` and `spans` set.
    pub fn of(style: &TextStyle, spans: &[RangeStyle<SpanStyle>]) -> Self {
        std::iter::once(&style.span_style)
            .chain(spans.iter().map(|span| &span.item))
            .fold(Self::default(), |ink, span| Self {
                stroke: match span.draw_style {
                    Some(TextDrawStyle::Stroke { width }) if width.is_finite() => {
                        ink.stroke.max(width)
                    }
                    _ => ink.stroke,
                },
                weight: ink.weight.max(span.font_weight.unwrap_or_default()),
                italic: ink.italic | (span.font_style == Some(FontStyle::Italic)),
            })
    }

    /// How far the glyphs of a text in this style, laid out as `layout`, put
    /// ink past their outlines: a stroke's outer half, and a bold or an
    /// italic that a face of the layout lacks and may synthesize.
    pub fn spread(self, layout: &TextLayout) -> InkSpread {
        let mut spread = InkSpread {
            around: self.stroke * 0.5 * STROKE_MITER_LIMIT,
            skew: 0.0,
        };
        if self.weight < FontWeight::SEMI_BOLD && !self.italic {
            return spread;
        }
        let style = if self.italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        };
        let synthesis =
            |face: &cranpose_text::Typeface| face.synthesis(self.weight, style, FontSynthesis::All);
        // Bold and italic text mostly has faces of its own.
        if !layout.typefaces().iter().any(|face| {
            let synthesis = synthesis(face);
            synthesis.bold || synthesis.italic
        }) {
            return spread;
        }
        for line in 0..layout.line_count() {
            for run in layout.glyph_runs(line) {
                let synthesis = synthesis(run.typeface);
                if synthesis.bold {
                    let embolden = run.size * FAKE_BOLD_MAX_RATIO;
                    spread.around = spread.around.max(embolden * 0.5 * STROKE_MITER_LIMIT);
                }
                if synthesis.italic {
                    spread.skew = FAKE_ITALIC_SKEW;
                }
            }
        }
        spread
    }
}

/// How far past their outlines a text's glyphs put ink, in layout pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct InkSpread {
    // Around every outline: a stroke's outer half or a synthesized bold's,
    // as far as a miter at the limit reaches.
    around: f32,
    // How far right a pixel of height above the baseline moves the ink.
    skew: f32,
}

/// The part of `rect` in which the glyphs of `layout` draw, `layout` drawn
/// from `rect`'s top left corner at `scale` units of `rect` a layout pixel:
/// across the visible text of its lines, and past them or past `rect`
/// wherever the ink of a glyph, spread by `spread`, reaches. Its edges lie on
/// whole units from that corner, so that the part rasterizes on the grid the
/// whole text does. A renderer draws a text's glyphs in this part, unless
/// the text [casts a shadow](crate::text_glyphs::casts_shadow).
pub fn glyph_part(rect: Rect, layout: &TextLayout, scale: f32, spread: InkSpread) -> Rect {
    let (left, right) = layout.lines_reach();
    let left = whole::floor(left * scale);
    let lines = Rect {
        x: rect.x + left,
        width: whole::ceil(right * scale) - left,
        ..rect
    };
    if !layout.inks_past_lines() && spread == InkSpread::default() {
        return lines;
    }
    with_ink(lines, (rect, layout, scale), spread)
}

// `lines`, the part of `rect` across the visible text of `layout`'s lines,
// grown by the ink its glyphs put past it, spread by `spread`. Out of line:
// most text keeps its ink within its lines and spreads none.
#[inline(never)]
fn with_ink(
    lines: Rect,
    (rect, layout, scale): (Rect, &TextLayout, f32),
    spread: InkSpread,
) -> Rect {
    let past = layout.ink_past_lines();
    let ink = if spread == InkSpread::default() {
        let Some(past) = past else {
            return lines;
        };
        past
    } else {
        let (left, right) = layout.lines_reach();
        let within = Rect {
            x: left,
            y: 0.0,
            width: right - left,
            height: layout.height(),
        };
        past.map_or(within, |past| within.union(past))
    };
    // A slant moves each point of a glyph by its height from the glyph's
    // baseline, which lies within the ink's height. Half a unit more on each
    // side holds a glyph whose origin a renderer rounds to a whole pixel.
    let InkSpread { around, skew } = spread;
    let slant = skew * ink.height;
    let left = whole::floor((ink.x - around - slant) * scale - 0.5);
    let top = whole::floor((ink.y - around) * scale - 0.5);
    let right = whole::ceil((ink.x + ink.width + around + slant) * scale + 0.5);
    let bottom = whole::ceil((ink.y + ink.height + around) * scale + 0.5);
    lines.union(Rect {
        x: rect.x + left,
        y: rect.y + top,
        width: right - left,
        height: bottom - top,
    })
}
