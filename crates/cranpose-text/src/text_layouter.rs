use std::ops::Range;

use crate::{
    bidi_level::{BidiLevel, reorder_visually},
    font_family_resolver::FontFamilyResolver,
    itemizer::{Itemizer, TextItem},
    line_breaker::{BreakOpportunity, LineBreaker},
    paragraph::{ResolvedTextDirection, TextAlign, TextDirection},
    script::Script,
    shaper::{ShapedGlyph, Shaper},
    shaping_span::ShapingSpan,
    text_layout::{LayoutLine, LineRun, TextLayout, font_units},
    typeface::Typeface,
};

/// A paragraph to lay out and how.
#[derive(Clone, Debug)]
pub struct LayoutInput<'a> {
    /// The text.
    pub text: &'a str,
    /// The spans of the text, covering it in order with no gap.
    pub spans: &'a [ShapingSpan<'a>],
    /// The base direction of each paragraph of the text.
    pub direction: TextDirection,
    /// Where each line sits across the layout's width.
    pub align: TextAlign,
    /// The width lines wrap at, in pixels; infinite for lines that end only
    /// at line breaks.
    pub width: f32,
}

/// Lays paragraphs out. Keeps its itemizer, shaper, line breaker and scratch
/// storage from one layout to the next.
#[derive(Default)]
pub struct TextLayouter {
    itemizer: Itemizer,
    shaper: Shaper,
    breaker: LineBreaker,
    items: Vec<ShapedItem>,
    // The start of each cluster in text order, with the width of the text
    // before it; the last entry is the end of the text.
    clusters: Vec<(usize, f32)>,
    breaks: Vec<BreakOpportunity>,
    line_ranges: Vec<Range<usize>>,
    pieces: Vec<Piece>,
}

#[derive(Clone, Debug)]
struct ShapedItem {
    text: Range<usize>,
    glyphs: Range<usize>,
    face: usize,
    span: usize,
    level: BidiLevel,
    paragraph_level: BidiLevel,
    script: Script,
}

// The part of an item inside one line, at one bidi level.
#[derive(Clone, Debug)]
struct Piece {
    text: Range<usize>,
    glyphs: Range<usize>,
    face: usize,
    size: f32,
    scale: f32,
    level: BidiLevel,
    width: f32,
}

impl TextLayouter {
    /// Lays `input` out with the typefaces of `resolver`.
    pub fn layout(&mut self, input: &LayoutInput<'_>, resolver: &FontFamilyResolver) -> TextLayout {
        let mut layout = TextLayout {
            faces: Vec::new(),
            glyphs: Vec::new(),
            runs: Vec::new(),
            lines: Vec::new(),
            width: 0.0,
            height: 0.0,
        };
        self.shape(input, resolver, &mut layout);
        self.measure_clusters(input, &layout);
        self.breaker.opportunities(input.text, &mut self.breaks);
        self.break_lines(input.text, input.width);
        self.build_lines(input, resolver, &mut layout);
        layout
    }

    fn shape(
        &mut self,
        input: &LayoutInput<'_>,
        resolver: &FontFamilyResolver,
        layout: &mut TextLayout,
    ) {
        self.items.clear();
        let text = input.text;
        let (shaper, items) = (&mut self.shaper, &mut self.items);
        self.itemizer
            .itemize(text, input.direction, input.spans, resolver, |item| {
                let start = layout.glyphs.len();
                shaper.shape(
                    text,
                    &item,
                    &input.spans[item.span],
                    0..text.len(),
                    &mut layout.glyphs,
                );
                items.push(ShapedItem {
                    text: item.range,
                    glyphs: start..layout.glyphs.len(),
                    face: face_index(&mut layout.faces, item.typeface),
                    span: item.span,
                    level: item.level,
                    paragraph_level: item.paragraph_level,
                    script: item.script,
                });
            });
    }

    fn measure_clusters(&mut self, input: &LayoutInput<'_>, layout: &TextLayout) {
        self.clusters.clear();
        let mut total = 0.0;
        for item in &self.items {
            let scale = scale(&input.spans[item.span], &layout.faces[item.face]);
            let glyphs = &layout.glyphs[item.glyphs.clone()];
            let mut add = |glyph: &ShapedGlyph| {
                let start = glyph.text_offset();
                if self.clusters.last().is_none_or(|&(last, _)| last != start) {
                    self.clusters.push((start, total));
                }
                total += font_units(glyph.advance) * scale;
            };
            if item.level.is_rtl() {
                glyphs.iter().rev().for_each(&mut add);
            } else {
                glyphs.iter().for_each(&mut add);
            }
        }
        self.clusters.push((input.text.len(), total));
    }

    // The width of the text from `start` to `end`.
    fn width(&self, start: usize, end: usize) -> f32 {
        self.width_before(end) - self.width_before(start)
    }

    fn width_before(&self, offset: usize) -> f32 {
        let at = self.clusters.partition_point(|&(start, _)| start < offset);
        self.clusters.get(at).map_or(0.0, |&(_, width)| width)
    }

    // The width of the text from `start` to `end` without its trailing
    // whitespace, which hangs past the line's end.
    fn visible_width(&self, text: &str, start: usize, end: usize) -> f32 {
        self.width(start, visible_end(text, start, end))
    }

    // Greedy line breaking: each line takes as many break opportunities as
    // fit `width`, and a word wider than `width` breaks between clusters.
    fn break_lines(&mut self, text: &str, width: f32) {
        self.line_ranges.clear();
        let mut start = 0;
        let mut fit = None;
        let mut index = 0;
        while let Some(&opportunity) = self.breaks.get(index) {
            if self.visible_width(text, start, opportunity.offset) <= width {
                if opportunity.mandatory {
                    self.line_ranges.push(start..opportunity.offset);
                    start = opportunity.offset;
                    fit = None;
                } else {
                    fit = Some(opportunity.offset);
                }
                index += 1;
            } else {
                let end = fit
                    .take()
                    .unwrap_or_else(|| self.cluster_break(start, opportunity.offset, width));
                self.line_ranges.push(start..end);
                start = end;
            }
        }
        let ends_with_break = self
            .breaks
            .last()
            .is_some_and(|opportunity| opportunity.mandatory);
        if start < text.len() || self.line_ranges.is_empty() || ends_with_break {
            self.line_ranges.push(start..text.len());
        }
    }

    // Where a line from `start` that cannot reach the opportunity at `end`
    // breaks: after the clusters that fit `width`, and after one at least.
    fn cluster_break(&self, start: usize, end: usize, width: f32) -> usize {
        let first = self
            .clusters
            .partition_point(|&(cluster, _)| cluster <= start);
        let mut cut = self
            .clusters
            .get(first)
            .map_or(end, |&(cluster, _)| cluster.min(end));
        for &(cluster, _) in &self.clusters[first..] {
            if cluster >= end || self.width(start, cluster) > width {
                break;
            }
            cut = cut.max(cluster);
        }
        cut
    }

    fn build_lines(
        &mut self,
        input: &LayoutInput<'_>,
        resolver: &FontFamilyResolver,
        layout: &mut TextLayout,
    ) {
        let mut top = 0.0;
        let mut widest: f32 = 0.0;
        for index in 0..self.line_ranges.len() {
            let range = self.line_ranges[index].clone();
            let visible = visible_end(input.text, range.start, range.end);
            self.collect_pieces(input, range.clone(), visible, layout);
            reorder_visually(&mut self.pieces, |piece| piece.level);
            let first_run = layout.runs.len();
            let mut x = 0.0;
            let mut visible_left = None;
            let mut width = 0.0;
            for piece in &self.pieces {
                if piece.text.start < visible {
                    visible_left.get_or_insert(x);
                    width += piece.width;
                }
                layout.runs.push(LineRun {
                    text: piece.text.clone(),
                    glyphs: piece.glyphs.clone(),
                    face: piece.face,
                    size: piece.size,
                    scale: piece.scale,
                    level: piece.level,
                    x,
                    width: piece.width,
                });
                x += piece.width;
            }
            for run in &mut layout.runs[first_run..] {
                run.x -= visible_left.unwrap_or(0.0);
            }
            let (ascent, descent) = self.line_extent(input, resolver, layout, range.start);
            let baseline = top + ascent;
            layout.lines.push(LayoutLine {
                level: self.paragraph_level(input, range.start),
                text: range,
                visible_end: visible,
                runs: first_run..layout.runs.len(),
                top,
                baseline,
                bottom: baseline + descent,
                left: 0.0,
                width,
            });
            top = baseline + descent;
            widest = widest.max(width);
        }
        layout.width = if input.width.is_finite() {
            input.width
        } else {
            widest
        };
        layout.height = top;
        for line in &mut layout.lines {
            line.left = align_fraction(input.align, line.level) * (layout.width - line.width);
        }
    }

    // The parts of the items inside the line `range`, in text order, the
    // trailing whitespace from `visible` on at the paragraph's level (rule L1
    // of UAX #9).
    fn collect_pieces(
        &mut self,
        input: &LayoutInput<'_>,
        range: Range<usize>,
        visible: usize,
        layout: &mut TextLayout,
    ) {
        let Self {
            items,
            shaper,
            pieces,
            ..
        } = self;
        pieces.clear();
        let first = items.partition_point(|item| item.text.end <= range.start);
        for item in &items[first..] {
            if item.text.start >= range.end {
                break;
            }
            let start = item.text.start.max(range.start);
            let end = item.text.end.min(range.end);
            let middle = visible.clamp(start, end);
            for (part, level) in [
                (start..middle, item.level),
                (middle..end, item.paragraph_level),
            ] {
                if part.is_empty() {
                    continue;
                }
                let glyphs = part_glyphs(shaper, input, item, part.clone(), range.clone(), layout);
                let scale = scale(&input.spans[item.span], &layout.faces[item.face]);
                pieces.push(Piece {
                    width: layout.glyphs[glyphs.clone()]
                        .iter()
                        .map(|glyph| font_units(glyph.advance) * scale)
                        .sum(),
                    text: part,
                    glyphs,
                    face: item.face,
                    size: input.spans[item.span].size,
                    scale,
                    level,
                });
            }
        }
    }

    // The tallest ascent and deepest descent of the typefaces on the line
    // just collected, so a fallback font taller than the text's own font
    // makes room for itself, as Android's fallback line spacing does. A line
    // with no text takes the typeface of its span.
    fn line_extent(
        &self,
        input: &LayoutInput<'_>,
        resolver: &FontFamilyResolver,
        layout: &TextLayout,
        start: usize,
    ) -> (f32, f32) {
        let extent = |face: &Typeface, scale: f32| {
            let metrics = face.line_metrics();
            (metrics.ascent * scale, -metrics.descent * scale)
        };
        if self.pieces.is_empty() {
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
                .unwrap_or_default();
        }
        self.pieces
            .iter()
            .fold((0.0_f32, 0.0_f32), |(up, down), piece| {
                let (ascent, descent) = extent(&layout.faces[piece.face], piece.scale);
                (up.max(ascent), down.max(descent))
            })
    }

    // The base level of the paragraph that holds `offset`.
    fn paragraph_level(&self, input: &LayoutInput<'_>, offset: usize) -> BidiLevel {
        let at = self.items.partition_point(|item| item.text.end <= offset);
        self.items
            .get(at)
            .or_else(|| self.items.last())
            .map_or_else(
                || match input.direction.resolve("") {
                    ResolvedTextDirection::Ltr => BidiLevel::LTR,
                    ResolvedTextDirection::Rtl => BidiLevel::RTL,
                },
                |item| item.paragraph_level,
            )
    }
}

// The glyphs of `part` of `item` in the line `line`: a slice of the item's
// glyphs, or the part shaped again without the text across the line's
// edge when the item's shapes change at a break there.
fn part_glyphs(
    shaper: &mut Shaper,
    input: &LayoutInput<'_>,
    item: &ShapedItem,
    part: Range<usize>,
    line: Range<usize>,
    layout: &mut TextLayout,
) -> Range<usize> {
    let rtl = item.level.is_rtl();
    let glyphs = &layout.glyphs[item.glyphs.clone()];
    let cut_unsafe = |cut: usize| {
        let at = cluster_index(glyphs, cut, item.level);
        glyphs
            .get(at)
            .is_none_or(|glyph| glyph.unsafe_to_break || glyph.text_offset() != cut)
    };
    let breaks_inside =
        (part.start == line.start && line.start > item.text.start && cut_unsafe(line.start))
            || (part.end == line.end && line.end < item.text.end && cut_unsafe(line.end));
    if !breaks_inside {
        let (low, high) = if rtl {
            (
                glyphs.partition_point(|glyph| glyph.text_offset() >= part.end),
                glyphs.partition_point(|glyph| glyph.text_offset() >= part.start),
            )
        } else {
            (
                glyphs.partition_point(|glyph| glyph.text_offset() < part.start),
                glyphs.partition_point(|glyph| glyph.text_offset() < part.end),
            )
        };
        return item.glyphs.start + low..item.glyphs.start + high;
    }
    let start = layout.glyphs.len();
    let reshaped = TextItem {
        range: part,
        level: item.level,
        paragraph_level: item.paragraph_level,
        script: item.script,
        typeface: &layout.faces[item.face],
        span: item.span,
    };
    shaper.shape(
        input.text,
        &reshaped,
        &input.spans[item.span],
        line,
        &mut layout.glyphs,
    );
    start..layout.glyphs.len()
}

// The index of `face` in `faces`, added when missing.
fn face_index(faces: &mut Vec<Typeface>, face: &Typeface) -> usize {
    faces
        .iter()
        .position(|known| known.same(face))
        .unwrap_or_else(|| {
            faces.push(face.clone());
            faces.len() - 1
        })
}

// Pixels per design unit of `face` at the size of `span`.
fn scale(span: &ShapingSpan<'_>, face: &Typeface) -> f32 {
    span.size / f32::from(face.units_per_em())
}

// The end of the text from `start` to `end` before its trailing whitespace.
fn visible_end(text: &str, start: usize, end: usize) -> usize {
    text.get(start..end)
        .map_or(end, |part| start + part.trim_end().len())
}

// The index of the first glyph of the cluster at `offset` in an item's glyphs,
// drawn right to left when `rtl`.
fn cluster_index(glyphs: &[ShapedGlyph], offset: usize, level: BidiLevel) -> usize {
    if level.is_rtl() {
        glyphs.partition_point(|glyph| glyph.text_offset() > offset)
    } else {
        glyphs.partition_point(|glyph| glyph.text_offset() < offset)
    }
}

// How far across the free width a line of paragraph `level` sits.
fn align_fraction(align: TextAlign, level: BidiLevel) -> f32 {
    let start = if level.is_rtl() { 1.0 } else { 0.0 };
    match align {
        TextAlign::Left => 0.0,
        TextAlign::Right => 1.0,
        TextAlign::Center => 0.5,
        TextAlign::Start | TextAlign::Justify | TextAlign::Unspecified => start,
        TextAlign::End => 1.0 - start,
    }
}
