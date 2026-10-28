//! What a text draws besides its glyphs: the background of its spans and
//! the lines under and through them, line by line from its layout.

use cranpose_text::TextLayout;
use cranpose_ui::{
    Brush, Color,
    text::{TextDecoration, TextStyle},
};
use cranpose_ui_graphics::Rect;

use crate::text_glyphs::{SpanPaint, StyledTextRef, visit_span_styles};

/// Calls `visit` with each rectangle a span's background covers behind
/// `layout`'s glyphs, in layout pixels from its top left corner, with its
/// brush: the stretch of each line the span's text takes, top to bottom of
/// the line.
pub fn visit_text_backgrounds(
    layout: &TextLayout,
    text: StyledTextRef<'_>,
    style: &TextStyle,
    mut visit: impl FnMut(Rect, &Brush),
) {
    if text.is_empty()
        || !(style.span_style.background.is_some()
            || text.span_styles.iter().any(|span| span.item.background.is_some()))
    {
        return;
    }
    let mut rects = Vec::new();
    visit_span_styles(text, style, |range, merged| {
        if let Some(background) = merged.background {
            rects.clear();
            layout.selection_rects(range.start, range.end, &mut rects);
            let brush = Brush::solid(background);
            for rect in &rects {
                visit(*rect, &brush);
            }
        }
    });
}

/// Calls `visit` with each line `layout`'s text draws under or through its
/// glyphs, in layout pixels from its top left corner, with its brush: where
/// the font of the run under it places its underline and its line through
/// the text. Glyphs whose style names no color draw their lines in
/// `fallback`.
pub fn visit_text_lines(
    layout: &TextLayout,
    text: StyledTextRef<'_>,
    style: &TextStyle,
    fallback: Color,
    mut visit: impl FnMut(Rect, &Brush),
) {
    let decorated = |span: &cranpose_ui::text::SpanStyle| {
        span.text_decoration
            .is_some_and(|decoration| decoration != TextDecoration::NONE)
    };
    if text.is_empty()
        || !(decorated(&style.span_style)
            || text.span_styles.iter().any(|span| decorated(&span.item)))
    {
        return;
    }
    let mut rects = Vec::new();
    visit_span_styles(text, style, |range, merged| {
        let Some(decoration) = merged.text_decoration.filter(|line| *line != TextDecoration::NONE)
        else {
            return;
        };
        let brush = SpanPaint::of(merged, fallback).brush;
        rects.clear();
        layout.selection_rects(range.start, range.end, &mut rects);
        for rect in &rects {
            let line = layout.line_for_vertical_position(rect.y + rect.height / 2.0);
            let Some(run) = layout
                .glyph_runs(line)
                .find(|run| run.text().start < range.end && range.start < run.text().end)
            else {
                continue;
            };
            let metrics = run.typeface.decoration_metrics();
            let scale = run.size / f32::from(run.typeface.units_per_em());
            let baseline = run.baseline();
            let mut line_rect = |top: f32, thickness: f32| {
                let decoration_rect = Rect {
                    x: rect.x,
                    y: baseline + top * scale,
                    width: rect.width,
                    height: thickness * scale,
                };
                visit(decoration_rect, &brush);
            };
            if decoration.contains(TextDecoration::UNDERLINE) {
                line_rect(metrics.underline_top, metrics.underline_thickness);
            }
            if decoration.contains(TextDecoration::LINE_THROUGH) {
                line_rect(metrics.strike_through_top, metrics.strike_through_thickness);
            }
        }
    });
}
