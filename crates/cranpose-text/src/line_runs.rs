use std::ops::Range;

use crate::{
    font_family_resolver::FontFamilyResolver,
    index_range::narrow,
    layout_body::LayoutBody,
    layout_input::LayoutInput,
    line_box::FontExtent,
    shaped_item::{Piece, scale},
    text_layout::LineRun,
    typeface::Typeface,
};

// Puts a line's pieces, already in visual order, into runs from its visible
// text's left edge; returns the runs and the visible width.
pub(crate) fn place_runs(
    pieces: &[Piece],
    visible: usize,
    layout: &mut LayoutBody,
) -> (Range<usize>, f32) {
    let first = layout.runs.len();
    let mut x = 0.0;
    let mut visible_left = None;
    let mut width = 0.0;
    for piece in pieces {
        if piece.text.start < visible || piece.text.is_empty() {
            visible_left.get_or_insert(x);
            width += piece.width;
        }
        layout.runs.push(LineRun {
            text: piece.text.clone().into(),
            glyphs: piece.glyphs.clone().into(),
            face: narrow(piece.face),
            size: piece.size,
            spacing: piece.spacing,
            spaces: 0,
            word_spacing: 0.0,
            baseline_shift: piece.baseline_shift,
            level: piece.level,
            x,
            width: piece.width,
            shift: 0.0,
        });
        x += piece.width;
    }
    for run in &mut layout.runs[first..] {
        run.x -= visible_left.unwrap_or(0.0);
    }
    (first..layout.runs.len(), width)
}

// The tallest ascent and deepest descent of the typefaces of a line's
// pieces, so a fallback font taller than the text's own font
// makes room for itself, as Android's fallback line spacing does. A line
// with no text takes the typeface of its span.
pub(crate) fn line_extent(
    pieces: &[Piece],
    input: &LayoutInput<'_>,
    resolver: &FontFamilyResolver,
    layout: &LayoutBody,
    start: usize,
) -> FontExtent {
    let extent = |face: &Typeface, scale: f32| {
        let metrics = face.line_metrics();
        FontExtent::new(
            metrics.ascent * scale,
            -metrics.descent * scale,
            metrics.line_gap * scale,
        )
    };
    if pieces.is_empty() {
        let span = input
            .spans
            .iter()
            .find(|span| span.range.contains(&start))
            .or_else(|| input.spans.last());
        return span
            .and_then(|span| {
                let face = resolver.resolve(span.font)?;
                Some(extent(face, scale(span, face)))
            })
            .unwrap_or_else(|| FontExtent::new(0.0, 0.0, 0.0));
    }
    // A span shifted up reaches higher above the line's baseline, and one
    // shifted down lower, as Android widens a run's metrics by its shift.
    pieces
        .iter()
        .fold(FontExtent::new(0.0, 0.0, 0.0), |line, piece| {
            let face = extent(&layout.faces[piece.face], piece.scale);
            let shift = piece.baseline_shift;
            FontExtent::new(
                line.ascent.max(face.ascent + shift.max(0.0)),
                line.descent.max(face.descent - shift.min(0.0)),
                line.line_gap.max(face.line_gap),
            )
        })
}
