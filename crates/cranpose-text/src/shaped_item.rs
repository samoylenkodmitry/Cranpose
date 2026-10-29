use std::ops::Range;

use smallvec::SmallVec;

use crate::{
    bidi_level::BidiLevel,
    itemizer::TextItem,
    layout_input::LayoutInput,
    script::Script,
    shaper::{ShapedGlyph, Shaper},
    shaping_span::ShapingSpan,
    text_layout::font_units,
    typeface::Typeface,
};

// An item of a paragraph shaped once for the paragraph, and the pieces its
// lines take of it.
#[derive(Clone, Debug)]
pub(crate) struct ShapedItem {
    pub(crate) text: Range<usize>,
    pub(crate) glyphs: Range<usize>,
    pub(crate) face: usize,
    pub(crate) span: usize,
    pub(crate) level: BidiLevel,
    pub(crate) paragraph_level: BidiLevel,
    pub(crate) script: Script,
    // Extra width of each of its clusters.
    pub(crate) spacing: f32,
}

impl ShapedItem {
    // The item that shapes `text`, which the layout draws after this item's
    // text in `face`, its typeface.
    pub(crate) fn inserted<'a>(&self, text: &str, face: &'a Typeface) -> TextItem<'a> {
        TextItem {
            range: 0..text.len(),
            level: self.level,
            paragraph_level: self.paragraph_level,
            script: self.script,
            typeface: face,
            span: self.span,
        }
    }
}

// The part of an item inside one line, at one bidi level, or an ellipsis.
#[derive(Clone, Debug)]
pub(crate) struct Piece {
    pub(crate) text: Range<usize>,
    pub(crate) glyphs: Range<usize>,
    pub(crate) face: usize,
    pub(crate) size: f32,
    pub(crate) scale: f32,
    pub(crate) spacing: f32,
    pub(crate) baseline_shift: f32,
    pub(crate) level: BidiLevel,
    pub(crate) width: f32,
}

// The index of the item that holds `offset`.
pub(crate) fn item_at(items: &[ShapedItem], offset: usize) -> usize {
    items.partition_point(|item| item.text.end <= offset)
}

// The hyphen Android draws after a hyphenated line in `face`: U+2010, or
// the hyphen-minus in a face without it.
pub(crate) fn hyphen(face: &Typeface) -> &'static str {
    if face.covers('\u{2010}') {
        "\u{2010}"
    } else {
        "-"
    }
}

// Shapes `inserted`, text the layout draws that the text does not hold,
// such as an ellipsis or a hyphen, as `item` says, appending its glyphs to
// `glyphs`; returns its width.
pub(crate) fn shape_inserted(
    shaper: &mut Shaper,
    input: &LayoutInput<'_>,
    item: &TextItem<'_>,
    inserted: &str,
    glyphs: &mut Vec<ShapedGlyph>,
) -> f32 {
    let start = glyphs.len();
    let span = &input.spans[item.span];
    shaper.shape(inserted, item, span, 0..inserted.len(), glyphs);
    spaced_width(&glyphs[start..], scale(span, item.typeface), 0.0)
}

// Gives the glyphs of an ellipsis or a hyphen the text offset it stands at.
pub(crate) fn set_cluster(glyphs: &mut [ShapedGlyph], offset: usize) {
    let cluster = u32::try_from(offset).unwrap_or(u32::MAX);
    for glyph in glyphs {
        glyph.cluster = cluster;
    }
}

// The glyphs of `sub` of `item` inside `part` of a line, among `glyphs` in
// `faces`: a slice of the item's glyphs, or `sub` shaped again without the
// text outside `part`, added to `glyphs`, when the item's shapes change
// across an edge of `part` inside it.
pub(crate) fn part_glyphs(
    shaper: &mut Shaper,
    input: &LayoutInput<'_>,
    item: &ShapedItem,
    sub: Range<usize>,
    part: Range<usize>,
    faces: &[Typeface],
    glyphs: &mut Vec<ShapedGlyph>,
) -> Range<usize> {
    let own = &glyphs[item.glyphs.clone()];
    let cut_unsafe = |cut: usize| {
        let at = cluster_index(own, cut, item.level);
        own.get(at)
            .is_none_or(|glyph| glyph.unsafe_to_break || glyph.text_offset() != cut)
    };
    let cut_inside =
        (sub.start == part.start && part.start > item.text.start && cut_unsafe(part.start))
            || (sub.end == part.end && part.end < item.text.end && cut_unsafe(part.end));
    if !cut_inside {
        let (low, high) = if item.level.is_rtl() {
            (
                own.partition_point(|glyph| glyph.text_offset() >= sub.end),
                own.partition_point(|glyph| glyph.text_offset() >= sub.start),
            )
        } else {
            (
                own.partition_point(|glyph| glyph.text_offset() < sub.start),
                own.partition_point(|glyph| glyph.text_offset() < sub.end),
            )
        };
        return item.glyphs.start + low..item.glyphs.start + high;
    }
    let start = glyphs.len();
    let reshaped = TextItem {
        range: sub,
        level: item.level,
        paragraph_level: item.paragraph_level,
        script: item.script,
        typeface: &faces[item.face],
        span: item.span,
    };
    shaper.shape(input.text, &reshaped, &input.spans[item.span], part, glyphs);
    start..glyphs.len()
}

// The index of `face` in `faces`, added when missing.
pub(crate) fn face_index(faces: &mut SmallVec<[Typeface; 1]>, face: &Typeface) -> usize {
    faces
        .iter()
        .position(|known| known.same(face))
        .unwrap_or_else(|| {
            faces.push(face.clone());
            faces.len() - 1
        })
}

// The width of `glyphs` at `scale`, each of their clusters `spacing` wider.
pub(crate) fn spaced_width(glyphs: &[ShapedGlyph], scale: f32, spacing: f32) -> f32 {
    let width = glyphs.iter().fold(0.0, |width, glyph| {
        width + font_units(glyph.advance) * scale
    });
    // Most text has no spacing, which adds nothing whatever the clusters.
    if spacing == 0.0 {
        return width;
    }
    let starts = glyphs
        .windows(2)
        .filter(|pair| pair[0].cluster != pair[1].cluster)
        .count()
        + usize::from(!glyphs.is_empty());
    width + spacing * f32::from(u16::try_from(starts).unwrap_or(u16::MAX))
}

// Pixels per design unit of `face` at the size of `span`.
pub(crate) fn scale(span: &ShapingSpan<'_>, face: &Typeface) -> f32 {
    span.size / f32::from(face.units_per_em())
}

// The index of the first glyph of the cluster at `offset` in an item's
// glyphs, which run right to left at an odd `level`.
pub(crate) fn cluster_index(glyphs: &[ShapedGlyph], offset: usize, level: BidiLevel) -> usize {
    if level.is_rtl() {
        glyphs.partition_point(|glyph| glyph.text_offset() > offset)
    } else {
        glyphs.partition_point(|glyph| glyph.text_offset() < offset)
    }
}
