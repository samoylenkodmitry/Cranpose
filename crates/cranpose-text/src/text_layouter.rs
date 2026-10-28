use std::ops::Range;

use crate::{
    bidi_level::{BidiLevel, reorder_visually},
    cluster_widths::ClusterWidths,
    font_family_resolver::FontFamilyResolver,
    itemizer::{Itemizer, TextItem},
    layout_options::{TextLayoutOptions, TextOverflow},
    line_box::{FontExtent, paragraph_line_box},
    line_breaker::{
        BreakOpportunity, LineBreaker, WrapWidths, break_lines, starts_paragraph, visible_end,
    },
    paragraph::{ResolvedTextDirection, TextAlign},
    script::Script,
    shaper::{ShapedGlyph, Shaper},
    shaping_span::ShapingSpan,
    style::ParagraphStyle,
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
    /// The paragraph settings: direction, alignment and line height style.
    pub paragraph: &'a ParagraphStyle,
    /// The paragraph's lengths in pixels.
    pub lengths: ParagraphLengths,
    /// The width lines wrap at and ellipses cut to, in pixels; infinite for
    /// lines that end only at line breaks.
    pub width: f32,
    /// Soft wrapping, the line limits and what shows where text overflows.
    pub options: TextLayoutOptions,
}

/// A paragraph style's lengths resolved to pixels.
#[derive(Clone, Copy, Debug)]
pub struct ParagraphLengths {
    /// The line height the style asks for, `None` for the fonts' own.
    pub line_height: Option<f32>,
    /// The indent of the first line of each paragraph.
    pub first_line_indent: f32,
    /// The indent of the other lines.
    pub rest_line_indent: f32,
    /// Device pixels per pixel of the layout: line boxes round to whole
    /// device pixels, as Android's do.
    pub grid: f32,
}

impl Default for ParagraphLengths {
    fn default() -> Self {
        Self {
            line_height: None,
            first_line_indent: 0.0,
            rest_line_indent: 0.0,
            grid: 1.0,
        }
    }
}

/// Lays paragraphs out. Keeps its itemizer, shaper, line breaker and scratch
/// storage from one layout to the next.
#[derive(Default)]
pub struct TextLayouter {
    itemizer: Itemizer,
    shaper: Shaper,
    breaker: LineBreaker,
    items: Vec<ShapedItem>,
    widths: ClusterWidths,
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
    // Extra width of each of its clusters.
    spacing: f32,
}

// The part of an item inside one line, at one bidi level, or an ellipsis.
#[derive(Clone, Debug)]
struct Piece {
    text: Range<usize>,
    glyphs: Range<usize>,
    face: usize,
    size: f32,
    scale: f32,
    spacing: f32,
    level: BidiLevel,
    width: f32,
}

// Where a line that does not fit shows an ellipsis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EllipsisAt {
    Start,
    Middle,
    End,
}

const ELLIPSIS: &str = "\u{2026}";

impl TextLayouter {
    /// Lays `input` out with the typefaces of `resolver`.
    pub fn layout(&mut self, input: &LayoutInput<'_>, resolver: &FontFamilyResolver) -> TextLayout {
        let options = input.options.normalized();
        let mut layout = TextLayout {
            faces: Vec::new(),
            glyphs: Vec::new(),
            runs: Vec::new(),
            lines: Vec::new(),
            width: 0.0,
            height: 0.0,
            did_overflow_height: false,
            did_overflow_width: false,
        };
        self.shape(input, resolver, &mut layout);
        self.measure_clusters(input, &layout);
        self.breaker.opportunities(input.text, &mut self.breaks);
        let lengths = input.lengths;
        let wrap = if options.soft_wrap {
            WrapWidths {
                first: input.width - lengths.first_line_indent,
                rest: input.width - lengths.rest_line_indent,
            }
        } else {
            WrapWidths {
                first: f32::INFINITY,
                rest: f32::INFINITY,
            }
        };
        break_lines(
            input.text,
            &self.widths,
            &self.breaks,
            wrap,
            &mut self.line_ranges,
        );
        layout.did_overflow_height = self.line_ranges.len() > options.max_lines;
        self.line_ranges.truncate(options.max_lines.max(1));
        self.build_lines(input, &options, resolver, &mut layout);
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
        let direction = input.paragraph.text_direction;
        self.itemizer
            .itemize(text, direction, input.spans, resolver, |item| {
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
                    spacing: if item.script.takes_letter_spacing() {
                        input.spans[item.span].letter_spacing
                    } else {
                        0.0
                    },
                });
            });
    }

    fn measure_clusters(&mut self, input: &LayoutInput<'_>, layout: &TextLayout) {
        self.widths.clear();
        for item in &self.items {
            let scale = scale(&input.spans[item.span], &layout.faces[item.face]);
            let glyphs = &layout.glyphs[item.glyphs.clone()];
            let mut add = |glyph: &ShapedGlyph| {
                self.widths.push(
                    glyph.text_offset(),
                    font_units(glyph.advance) * scale,
                    item.spacing,
                );
            };
            if item.level.is_rtl() {
                glyphs.iter().rev().for_each(&mut add);
            } else {
                glyphs.iter().for_each(&mut add);
            }
        }
        self.widths.finish(input.text.len());
    }

    fn build_lines(
        &mut self,
        input: &LayoutInput<'_>,
        options: &TextLayoutOptions,
        resolver: &FontFamilyResolver,
        layout: &mut TextLayout,
    ) {
        let mut top = 0.0;
        let mut widest: f32 = 0.0;
        let last = self.line_ranges.len() - 1;
        for index in 0..=last {
            let range = self.line_ranges[index].clone();
            let visible = visible_end(input.text, range.start, range.end);
            let overflows = self.widths.width(range.start, visible) > input.width;
            let ellipsis = ellipsis_at(options, index == last)
                .filter(|_| overflows || (index == last && layout.did_overflow_height));
            self.pieces.clear();
            let ellipsized = match ellipsis {
                Some(at) => self.ellipsize(input, resolver, at, range.clone(), visible, layout),
                None => false,
            };
            if !ellipsized {
                self.collect_pieces(input, range.clone(), visible, layout);
            }
            reorder_visually(&mut self.pieces, |piece| piece.level);
            let (runs, width) = self.place_runs(visible, layout);
            let extent = self.line_extent(input, resolver, layout, range.start);
            let asked = input.lengths.line_height.unwrap_or(f32::NAN);
            let line_box = paragraph_line_box(input.paragraph, extent, asked, input.lengths.grid);
            let trim_top = if index == 0 { line_box.trim_top } else { 0.0 };
            let trim_bottom = if index == last {
                line_box.trim_bottom
            } else {
                0.0
            };
            let indent = if starts_paragraph(input.text, range.start) {
                input.lengths.first_line_indent
            } else {
                input.lengths.rest_line_indent
            };
            let bottom = top + line_box.height - trim_top - trim_bottom;
            layout.lines.push(LayoutLine {
                level: self.paragraph_level(input, range.start),
                text: range,
                visible_end: visible,
                runs,
                top,
                baseline: top + line_box.baseline - trim_top,
                bottom,
                left: indent,
                width,
                ellipsized,
            });
            top = bottom;
            widest = widest.max(width + indent);
        }
        if layout.lines.len() < options.min_lines {
            let first = &layout.lines[0];
            let missing = options.min_lines - layout.lines.len();
            top += (first.bottom - first.top) * lines_f32(missing);
        }
        layout.width = if input.width.is_finite() {
            input.width
        } else {
            widest
        };
        layout.height = top;
        layout.did_overflow_width = widest > layout.width;
        let align = input.paragraph.text_align;
        for line in &mut layout.lines {
            let indent = line.left;
            let free = layout.width - indent - line.width;
            line.left = align_fraction(align, line.level) * free;
            if !line.level.is_rtl() {
                line.left += indent;
            }
        }
    }

    // Puts the line's pieces, already in visual order, into runs from its
    // visible text's left edge; returns the runs and the visible width.
    fn place_runs(&self, visible: usize, layout: &mut TextLayout) -> (Range<usize>, f32) {
        let first = layout.runs.len();
        let mut x = 0.0;
        let mut visible_left = None;
        let mut width = 0.0;
        for piece in &self.pieces {
            if piece.text.start < visible || piece.text.is_empty() {
                visible_left.get_or_insert(x);
                width += piece.width;
            }
            layout.runs.push(LineRun {
                text: piece.text.clone(),
                glyphs: piece.glyphs.clone(),
                face: piece.face,
                size: piece.size,
                scale: piece.scale,
                spacing: piece.spacing,
                level: piece.level,
                x,
                width: piece.width,
            });
            x += piece.width;
        }
        for run in &mut layout.runs[first..] {
            run.x -= visible_left.unwrap_or(0.0);
        }
        (first..layout.runs.len(), width)
    }

    // Collects the pieces of a line that does not fit with an ellipsis where
    // `at` says, cutting its text at cluster boundaries so the rest and the
    // ellipsis fit the width. False when no typeface draws an ellipsis.
    fn ellipsize(
        &mut self,
        input: &LayoutInput<'_>,
        resolver: &FontFamilyResolver,
        at: EllipsisAt,
        range: Range<usize>,
        visible: usize,
        layout: &mut TextLayout,
    ) -> bool {
        let span = input
            .spans
            .iter()
            .position(|span| {
                span.range
                    .contains(&visible.saturating_sub(1).max(range.start))
            })
            .unwrap_or(0);
        let level = self.paragraph_level(input, range.start);
        let Some(mut ellipsis) = self.ellipsis_piece(input, resolver, span, level, layout) else {
            return false;
        };
        let room = (input.width - ellipsis.width).max(0.0);
        let text = input.text;
        let (head, tail) = match at {
            EllipsisAt::End => {
                let cut = self.widths.last_fitting(range.start, visible, room);
                (visible_end(text, range.start, cut), range.end)
            }
            EllipsisAt::Start => (
                range.start,
                self.widths.first_fitting(range.start, visible, room),
            ),
            EllipsisAt::Middle => {
                let head = self.widths.last_fitting(range.start, visible, room / 2.0);
                let used = self.widths.width(range.start, head);
                (head, self.widths.first_fitting(head, visible, room - used))
            }
        };
        set_cluster(&mut layout.glyphs[ellipsis.glyphs.clone()], head);
        ellipsis.text = head..head;
        self.collect_pieces(input, range.start..head, visible, layout);
        self.pieces.push(ellipsis);
        if tail < range.end {
            self.collect_pieces(input, tail..range.end, visible, layout);
        }
        true
    }

    // The ellipsis in the typeface that `span` resolves it to, at `level`.
    fn ellipsis_piece(
        &mut self,
        input: &LayoutInput<'_>,
        resolver: &FontFamilyResolver,
        span: usize,
        level: BidiLevel,
        layout: &mut TextLayout,
    ) -> Option<Piece> {
        let style = &input.spans[span];
        let mut typeface = None;
        resolver.typeface_runs(ELLIPSIS, style.font, |_, face| {
            typeface.get_or_insert(face);
        });
        let typeface = &resolver.typefaces()[typeface?];
        let start = layout.glyphs.len();
        let item = TextItem {
            range: 0..ELLIPSIS.len(),
            level,
            paragraph_level: level,
            script: Script::COMMON,
            typeface,
            span,
        };
        self.shaper.shape(
            ELLIPSIS,
            &item,
            style,
            0..ELLIPSIS.len(),
            &mut layout.glyphs,
        );
        let scale = scale(style, typeface);
        let glyphs = start..layout.glyphs.len();
        Some(Piece {
            width: spaced_width(&layout.glyphs[glyphs.clone()], scale, 0.0),
            text: 0..0,
            glyphs,
            face: face_index(&mut layout.faces, typeface),
            size: style.size,
            scale,
            spacing: 0.0,
            level,
        })
    }

    // Adds the parts of the items inside `part` of a line, in text order,
    // the trailing whitespace from `visible` on at the paragraph's level
    // (rule L1 of UAX #9).
    fn collect_pieces(
        &mut self,
        input: &LayoutInput<'_>,
        part: Range<usize>,
        visible: usize,
        layout: &mut TextLayout,
    ) {
        let Self {
            items,
            shaper,
            pieces,
            ..
        } = self;
        let first = items.partition_point(|item| item.text.end <= part.start);
        for item in &items[first..] {
            if item.text.start >= part.end {
                break;
            }
            let start = item.text.start.max(part.start);
            let end = item.text.end.min(part.end);
            let middle = visible.clamp(start, end);
            for (sub, level) in [
                (start..middle, item.level),
                (middle..end, item.paragraph_level),
            ] {
                if sub.is_empty() {
                    continue;
                }
                let glyphs = part_glyphs(shaper, input, item, sub.clone(), part.clone(), layout);
                let scale = scale(&input.spans[item.span], &layout.faces[item.face]);
                pieces.push(Piece {
                    width: spaced_width(&layout.glyphs[glyphs.clone()], scale, item.spacing),
                    text: sub,
                    glyphs,
                    face: item.face,
                    size: input.spans[item.span].size,
                    scale,
                    spacing: item.spacing,
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
    ) -> FontExtent {
        let extent = |face: &Typeface, scale: f32| {
            let metrics = face.line_metrics();
            FontExtent::new(
                metrics.ascent * scale,
                -metrics.descent * scale,
                metrics.line_gap * scale,
            )
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
                .unwrap_or_else(|| FontExtent::new(0.0, 0.0, 0.0));
        }
        self.pieces
            .iter()
            .fold(FontExtent::new(0.0, 0.0, 0.0), |line, piece| {
                let face = extent(&layout.faces[piece.face], piece.scale);
                FontExtent::new(
                    line.ascent.max(face.ascent),
                    line.descent.max(face.descent),
                    line.line_gap.max(face.line_gap),
                )
            })
    }

    // The base level of the paragraph that holds `offset`.
    fn paragraph_level(&self, input: &LayoutInput<'_>, offset: usize) -> BidiLevel {
        let at = self.items.partition_point(|item| item.text.end <= offset);
        self.items
            .get(at)
            .or_else(|| self.items.last())
            .map_or_else(
                || match input.paragraph.text_direction.resolve("") {
                    ResolvedTextDirection::Ltr => BidiLevel::LTR,
                    ResolvedTextDirection::Rtl => BidiLevel::RTL,
                },
                |item| item.paragraph_level,
            )
    }
}

// Where the last line, or a line on its own, shows an ellipsis: a start or
// middle ellipsis only on a single line, as Android lays them out.
fn ellipsis_at(options: &TextLayoutOptions, last: bool) -> Option<EllipsisAt> {
    match options.overflow {
        TextOverflow::Ellipsis if last => Some(EllipsisAt::End),
        TextOverflow::StartEllipsis if last && options.max_lines == 1 => Some(EllipsisAt::Start),
        TextOverflow::MiddleEllipsis if last && options.max_lines == 1 => Some(EllipsisAt::Middle),
        TextOverflow::StartEllipsis | TextOverflow::MiddleEllipsis if last => Some(EllipsisAt::End),
        _ => None,
    }
}

// Gives the glyphs of an ellipsis the text offset it stands at.
fn set_cluster(glyphs: &mut [ShapedGlyph], offset: usize) {
    let cluster = u32::try_from(offset).unwrap_or(u32::MAX);
    for glyph in glyphs {
        glyph.cluster = cluster;
    }
}

// The glyphs of `sub` of `item` inside `part` of a line: a slice of the
// item's glyphs, or `sub` shaped again without the text outside `part` when
// the item's shapes change across an edge of `part` inside it.
fn part_glyphs(
    shaper: &mut Shaper,
    input: &LayoutInput<'_>,
    item: &ShapedItem,
    sub: Range<usize>,
    part: Range<usize>,
    layout: &mut TextLayout,
) -> Range<usize> {
    let glyphs = &layout.glyphs[item.glyphs.clone()];
    let cut_unsafe = |cut: usize| {
        let at = cluster_index(glyphs, cut, item.level);
        glyphs
            .get(at)
            .is_none_or(|glyph| glyph.unsafe_to_break || glyph.text_offset() != cut)
    };
    let cut_inside =
        (sub.start == part.start && part.start > item.text.start && cut_unsafe(part.start))
            || (sub.end == part.end && part.end < item.text.end && cut_unsafe(part.end));
    if !cut_inside {
        let (low, high) = if item.level.is_rtl() {
            (
                glyphs.partition_point(|glyph| glyph.text_offset() >= sub.end),
                glyphs.partition_point(|glyph| glyph.text_offset() >= sub.start),
            )
        } else {
            (
                glyphs.partition_point(|glyph| glyph.text_offset() < sub.start),
                glyphs.partition_point(|glyph| glyph.text_offset() < sub.end),
            )
        };
        return item.glyphs.start + low..item.glyphs.start + high;
    }
    let start = layout.glyphs.len();
    let reshaped = TextItem {
        range: sub,
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
        part,
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

// The width of `glyphs` at `scale`, each of their clusters `spacing` wider.
fn spaced_width(glyphs: &[ShapedGlyph], scale: f32, spacing: f32) -> f32 {
    let mut clusters = 0_u16;
    let mut width = 0.0;
    for (at, glyph) in glyphs.iter().enumerate() {
        if at == 0 || glyphs[at - 1].cluster != glyph.cluster {
            clusters = clusters.saturating_add(1);
        }
        width += font_units(glyph.advance) * scale;
    }
    width + spacing * f32::from(clusters)
}

// Pixels per design unit of `face` at the size of `span`.
fn scale(span: &ShapingSpan<'_>, face: &Typeface) -> f32 {
    span.size / f32::from(face.units_per_em())
}

// A line count as a float; counts beyond `u16` saturate.
fn lines_f32(count: usize) -> f32 {
    f32::from(u16::try_from(count).unwrap_or(u16::MAX))
}

// The index of the first glyph of the cluster at `offset` in an item's
// glyphs, which run right to left at an odd `level`.
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
