//! Where a laid-out text's glyphs draw: across the visible text of its
//! lines, and wherever the ink of a glyph reaches past them.

use cranpose_text::{SynthesizedStyle, TextLayout};
use cranpose_ui::text::{RangeStyle, SpanStyle, TextDrawStyle, TextStyle};
use cranpose_ui_graphics::{Rect, whole};

use crate::{
    glyph_mask::STROKE_MITER_LIMIT,
    text_glyphs::{FAKE_BOLD_MAX_RATIO, FAKE_ITALIC_SKEW},
};

/// How far past their outlines a text's glyphs put ink, in layout pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct InkSpread {
    // Around every outline: a stroke's outer half or a synthesized bold's,
    // as far as a miter at the limit reaches.
    around: f32,
    // How far right a pixel of height above the baseline moves the ink.
    skew: f32,
}

impl InkSpread {
    /// How far past their outlines glyphs put ink when stroked `stroke`
    /// layout pixels wide, zero for a fill, and drawn with the bold and the
    /// italic `synthesized` names: a stroke's outer half, or a synthesized
    /// bold's, and the slant of an italic.
    pub fn of(stroke: f32, synthesized: SynthesizedStyle) -> Self {
        // A glyph is stroked or emboldened, never both.
        let embolden = synthesized
            .bold_size
            .map_or(0.0, |size| size * FAKE_BOLD_MAX_RATIO);
        Self {
            around: stroke.max(embolden) * 0.5 * STROKE_MITER_LIMIT,
            skew: if synthesized.italic {
                FAKE_ITALIC_SKEW
            } else {
                0.0
            },
        }
    }

    /// How far past their outlines the glyphs of a text in `style` with
    /// `spans`, laid out as `layout`, put ink.
    pub fn of_text(
        style: &TextStyle,
        spans: &[RangeStyle<SpanStyle>],
        layout: &TextLayout,
    ) -> Self {
        let stroke = spans
            .iter()
            .fold(stroke_width(&style.span_style), |stroke, span| {
                stroke.max(stroke_width(&span.item))
            });
        Self::of(stroke, layout.synthesized_style())
    }
}

// How wide a stroke `style` draws its glyphs with, in layout pixels; zero
// for filled glyphs.
fn stroke_width(style: &SpanStyle) -> f32 {
    match style.draw_style {
        Some(TextDrawStyle::Stroke { width }) if width.is_finite() && width > 0.0 => width,
        _ => 0.0,
    }
}

/// The part of `rect` in which the glyphs of `layout` draw, `layout` drawn
/// from `rect`'s top left corner at `scale` units of `rect` a layout pixel:
/// across the visible text of its lines, and past them or past `rect`
/// wherever the ink of a glyph, spread by `spread`, reaches. Its left and
/// right edges lie on whole units from that corner, so that the part
/// rasterizes on the grid the whole text does. A renderer draws a text's
/// glyphs in this part, unless
/// the text [casts a shadow](crate::text_glyphs::casts_shadow).
#[inline]
pub fn glyph_part(rect: Rect, layout: &TextLayout, scale: f32, spread: InkSpread) -> Rect {
    let reach = reach_from_corner((rect, layout, scale), spread);
    let left = whole::floor(reach.x);
    Rect {
        x: rect.x + left,
        y: rect.y + reach.y,
        width: whole::ceil(reach.x + reach.width) - left,
        height: reach.height,
    }
}

// The box around `rect` and the ink the glyphs of `layout`, drawn from its
// top left corner and spread by `spread`, put past it, in layout pixels:
// `glyph_part` at a scale of one, unrounded. A reader that tests pixels
// against it allows for the antialiased edge, as the containment slack and
// the rounded clip's margin do.
pub(crate) fn glyph_bounds(rect: Rect, layout: &TextLayout, spread: InkSpread) -> Rect {
    let reach = reach_from_corner((rect, layout, 1.0), spread);
    rect.union(Rect {
        x: rect.x + reach.x,
        y: rect.y + reach.y,
        ..reach
    })
}

// Where the glyphs of `layout`, drawn from the top left corner of `rect` at
// `scale` units of `rect` a layout pixel and spread by `spread`, put ink,
// in units of `rect` from that corner, unrounded: down at least to the
// bottom of `rect`. A slant moves each point of a glyph by its height from
// the glyph's baseline, which lies within that height.
#[inline]
fn reach_from_corner((rect, layout, scale): (Rect, &TextLayout, f32), spread: InkSpread) -> Rect {
    let InkSpread { around, skew } = spread;
    let ink = layout.glyph_reach();
    let around = around * scale;
    let top = ink.y * scale;
    let bottom = ((ink.y + ink.height) * scale).max(rect.height);
    let across = around + skew * (bottom - top);
    Rect {
        x: ink.x * scale - across,
        y: top - around,
        width: ink.width * scale + 2.0 * across,
        height: bottom - top + 2.0 * around,
    }
}
