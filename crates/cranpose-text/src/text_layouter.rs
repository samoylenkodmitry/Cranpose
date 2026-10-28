use std::ops::Range;

use crate::{
    bidi_level::{BidiLevel, reorder_visually},
    cluster_widths::ClusterWidths,
    font_family_resolver::FontFamilyResolver,
    glyph_id::GlyphId,
    itemizer::{Itemizer, TextItem},
    layout_options::{TextLayoutOptions, TextOverflow},
    line_box::{FontExtent, paragraph_line_box},
    line_breaker::{
        BreakOpportunity, BrokenLine, LineBalancer, LineBreaker, LineEnd, WrapWidths, Wrapping,
        break_lines, has_hard_break, starts_paragraph, visible_end,
    },
    paragraph::{LineBreak, ResolvedTextDirection, TextAlign},
    script::Script,
    shaper::{ShapedGlyph, Shaper},
    shaping_span::ShapingSpan,
    style::ParagraphStyle,
    text_layout::{LayoutLine, LineRun, TextLayout, font_units},
    typeface::Typeface,
};
#[cfg(feature = "hyphenation")]
use crate::{hyphenator::Hyphenator, line_breaker::BreakKind, paragraph::Hyphens};

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
    balancer: LineBalancer,
    items: Vec<ShapedItem>,
    widths: ClusterWidths,
    breaks: Vec<BreakOpportunity>,
    lines: Vec<BrokenLine>,
    pieces: Vec<Piece>,
    #[cfg(feature = "hyphenation")]
    hyphenator: Hyphenator,
    // The opportunities with the hyphenation points among them, swapped
    // with `breaks` once made, and a word's points.
    #[cfg(feature = "hyphenation")]
    hyphenated: Vec<BreakOpportunity>,
    #[cfg(feature = "hyphenation")]
    points: Vec<usize>,
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

impl ShapedItem {
    // The item that shapes `text`, which the layout draws after this item's
    // text in `face`, its typeface.
    fn inserted<'a>(&self, text: &str, face: &'a Typeface) -> TextItem<'a> {
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
struct Piece {
    text: Range<usize>,
    glyphs: Range<usize>,
    face: usize,
    size: f32,
    scale: f32,
    spacing: f32,
    baseline_shift: f32,
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
// How much the free width of a paragraph's last line counts in even breaking
// for `LineBreak.Paragraph`, which lets the last line stay short.
const PARAGRAPH_LAST_LINE: f32 = 0.16;

impl TextLayouter {
    /// Lays `input` out with the typefaces of `resolver`.
    pub fn layout(&mut self, input: &LayoutInput<'_>, resolver: &FontFamilyResolver) -> TextLayout {
        let mut layout = TextLayout::default();
        self.layout_into(input, resolver, &mut layout);
        layout
    }

    /// Lays `input` out into `layout` in place of what it held, reusing its
    /// storage: text that changes every frame lays out without allocating
    /// once the storage has grown to fit it.
    pub fn layout_into(
        &mut self,
        input: &LayoutInput<'_>,
        resolver: &FontFamilyResolver,
        layout: &mut TextLayout,
    ) {
        let options = input.options.normalized();
        layout.clear();
        // A glyph takes at least a byte of text, so the glyphs grow once.
        layout.glyphs.reserve(input.text.len());
        self.shape(input, resolver, layout);
        self.measure_clusters(input, layout);
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
        let text = input.text;
        // Most text is one line that fits; it needs no break opportunities.
        if !has_hard_break(text)
            && self.widths.width(0, visible_end(text, 0, text.len())) <= wrap.first
        {
            self.lines.clear();
            self.lines.push(BrokenLine::plain(0..text.len()));
        } else {
            let line_break = input.paragraph.line_break;
            self.breaker
                .opportunities(text, line_break, &mut self.breaks);
            #[cfg(feature = "hyphenation")]
            if input.paragraph.hyphens == Hyphens::Auto {
                self.hyphenate(input, wrap, layout);
            }
            let wrapping = Wrapping {
                text,
                widths: &self.widths,
                wrap,
            };
            match line_break {
                LineBreak::Paragraph => self.balancer.balance(
                    &wrapping,
                    &self.breaks,
                    PARAGRAPH_LAST_LINE,
                    &mut self.lines,
                ),
                LineBreak::Heading => {
                    self.balancer
                        .balance(&wrapping, &self.breaks, 1.0, &mut self.lines);
                }
                LineBreak::Simple | LineBreak::Unspecified => {
                    break_lines(&wrapping, &self.breaks, &mut self.lines);
                }
            }
        }
        layout.did_overflow_height = self.lines.len() > options.max_lines;
        self.lines.truncate(options.max_lines.max(1));
        self.build_lines(input, &options, resolver, layout);
    }

    /// The dictionaries `Hyphens.Auto` hyphenates words by.
    #[cfg(feature = "hyphenation")]
    pub fn hyphenator_mut(&mut self) -> &mut Hyphenator {
        &mut self.hyphenator
    }

    // Adds to the break opportunities the hyphenation points of the words a
    // line may break inside: every word for even breaking, and for greedy
    // breaking the words wider than a line, the only ones it hyphenates.
    #[cfg(feature = "hyphenation")]
    fn hyphenate(&mut self, input: &LayoutInput<'_>, wrap: WrapWidths, layout: &mut TextLayout) {
        if !wrap.first.is_finite() {
            return;
        }
        let wider_than = match input.paragraph.line_break {
            LineBreak::Paragraph | LineBreak::Heading => f32::NEG_INFINITY,
            LineBreak::Simple | LineBreak::Unspecified => wrap.first.min(wrap.rest),
        };
        // Android's penalty for a hyphen in even breaking: half the font size
        // times the line width, a quarter of it in justified text.
        let justified = if input.paragraph.text_align == TextAlign::Justify {
            0.25
        } else {
            1.0
        };
        let penalty_per_size = 0.5 * wrap.first * justified;
        let breaks = std::mem::take(&mut self.breaks);
        self.hyphenated.clear();
        let mut word_start = 0;
        for &opportunity in &breaks {
            let word_end = visible_end(input.text, word_start, opportunity.offset);
            if self.widths.width(word_start, word_end) > wider_than {
                self.hyphenate_word(input, word_start..word_end, penalty_per_size, layout);
            }
            self.hyphenated.push(opportunity);
            word_start = opportunity.offset;
        }
        self.breaks = breaks;
        std::mem::swap(&mut self.breaks, &mut self.hyphenated);
    }

    // Adds the hyphenation points of the letters of `word`, without the
    // punctuation around them, that fall between clusters, each with the
    // width of its hyphen.
    #[cfg(feature = "hyphenation")]
    fn hyphenate_word(
        &mut self,
        input: &LayoutInput<'_>,
        word: Range<usize>,
        penalty_per_size: f32,
        layout: &mut TextLayout,
    ) {
        let Some(text) = input.text.get(word.clone()) else {
            return;
        };
        let letters = text.trim_start_matches(|ch: char| !ch.is_alphabetic());
        let start = word.start + (text.len() - letters.len());
        let letters = letters.trim_end_matches(|ch: char| !ch.is_alphabetic());
        let Some(first) = self.items.get(item_at(&self.items, start)) else {
            return;
        };
        if letters.is_empty() || first.level.is_rtl() {
            return;
        }
        self.points.clear();
        self.hyphenator
            .break_points(input.spans[first.span].language, letters, &mut self.points);
        let mut hyphen_width: Option<(usize, f32)> = None;
        for &point in &self.points {
            let at = start + point;
            if self.widths.next_boundary(at.saturating_sub(1)) != Some(at) {
                continue;
            }
            let index = item_at(&self.items, at.saturating_sub(1));
            let item = &self.items[index];
            let width = match hyphen_width {
                Some((known, width)) if known == index => width,
                _ => {
                    let face = &layout.faces[item.face];
                    let shaped = layout.glyphs.len();
                    let text = hyphen(face);
                    let item = item.inserted(text, face);
                    let width =
                        shape_inserted(&mut self.shaper, input, &item, text, &mut layout.glyphs);
                    layout.glyphs.truncate(shaped);
                    hyphen_width = Some((index, width));
                    width
                }
            };
            self.hyphenated.push(BreakOpportunity {
                offset: at,
                kind: BreakKind::Hyphen {
                    width,
                    penalty: penalty_per_size * input.spans[item.span].size,
                },
            });
        }
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
        let last = self.lines.len() - 1;
        for index in 0..=last {
            let range = self.lines[index].text.clone();
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
                if self.lines[index].end == LineEnd::Hyphen {
                    self.push_hyphen(input, range.end, layout);
                }
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
        align_lines(input, layout);
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
                word_spacing: 0.0,
                space: None,
                baseline_shift: piece.baseline_shift,
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
        let item = TextItem {
            range: 0..ELLIPSIS.len(),
            level,
            paragraph_level: level,
            script: Script::COMMON,
            typeface,
            span,
        };
        let start = layout.glyphs.len();
        let width = shape_inserted(&mut self.shaper, input, &item, ELLIPSIS, &mut layout.glyphs);
        Some(Piece {
            width,
            text: 0..0,
            glyphs: start..layout.glyphs.len(),
            face: face_index(&mut layout.faces, typeface),
            size: style.size,
            scale: scale(style, typeface),
            spacing: 0.0,
            baseline_shift: style.baseline_shift,
            level,
        })
    }

    // Adds the hyphen of a line hyphenated at `at`, in the typeface and
    // settings of the text before it, as Android draws it.
    fn push_hyphen(&mut self, input: &LayoutInput<'_>, at: usize, layout: &mut TextLayout) {
        let Some(item) = self.items.get(item_at(&self.items, at.saturating_sub(1))) else {
            return;
        };
        let span = &input.spans[item.span];
        let face = &layout.faces[item.face];
        let hyphen = hyphen(face);
        let start = layout.glyphs.len();
        let width = shape_inserted(
            &mut self.shaper,
            input,
            &item.inserted(hyphen, face),
            hyphen,
            &mut layout.glyphs,
        );
        set_cluster(&mut layout.glyphs[start..], at);
        self.pieces.push(Piece {
            width,
            text: at..at,
            glyphs: start..layout.glyphs.len(),
            face: item.face,
            size: span.size,
            scale: scale(span, face),
            spacing: 0.0,
            baseline_shift: span.baseline_shift,
            level: item.level,
        });
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
                    baseline_shift: input.spans[item.span].baseline_shift,
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
        self.items
            .get(item_at(&self.items, offset))
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

// The index of the item that holds `offset`.
fn item_at(items: &[ShapedItem], offset: usize) -> usize {
    items.partition_point(|item| item.text.end <= offset)
}

// The hyphen Android draws after a hyphenated line in `face`: U+2010, or
// the hyphen-minus in a face without it.
fn hyphen(face: &Typeface) -> &'static str {
    if face.covers('\u{2010}') {
        "\u{2010}"
    } else {
        "-"
    }
}

// Shapes `inserted`, text the layout draws that the text does not hold,
// such as an ellipsis or a hyphen, as `item` says, appending its glyphs to
// `glyphs`; returns its width.
fn shape_inserted(
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

// Places each line across the layout's width as the paragraph aligns it,
// stretching justified lines to the width.
fn align_lines(input: &LayoutInput<'_>, layout: &mut TextLayout) {
    let align = input.paragraph.text_align;
    for index in 0..layout.lines.len() {
        let line = &layout.lines[index];
        let indent = line.left;
        let free = layout.width - indent - line.width;
        // Justified text stretches the spaces of each line but the last
        // of its paragraph, as Android's inter-word justification does.
        let justified = align == TextAlign::Justify
            && free > 0.0
            && !line.ellipsized
            && layout
                .lines
                .get(index + 1)
                .is_some_and(|next| !starts_paragraph(input.text, next.text.start));
        let stretched = justified && justify(layout, index, free);
        let line = &mut layout.lines[index];
        line.left = if stretched {
            0.0
        } else {
            align_fraction(align, line.level) * free
        };
        if !line.level.is_rtl() {
            line.left += indent;
        }
    }
}

// Spreads `free` width over the spaces of the visible text of line `index`;
// false when the line has none.
fn justify(layout: &mut TextLayout, index: usize, free: f32) -> bool {
    let line = &layout.lines[index];
    let visible = line.visible_end;
    let runs = line.runs.clone();
    let spaces_of = |layout: &TextLayout, run: &LineRun| -> (Option<GlyphId>, u16) {
        let space = layout.faces[run.face].glyph(' ');
        let count = layout.glyphs[run.glyphs.clone()]
            .iter()
            .filter(|glyph| Some(glyph.id) == space && glyph.text_offset() < visible)
            .count();
        (space, u16::try_from(count).unwrap_or(u16::MAX))
    };
    let total: u32 = layout.runs[runs.clone()]
        .iter()
        .map(|run| u32::from(spaces_of(layout, run).1))
        .sum();
    if total == 0 {
        return false;
    }
    let per_space = free / f32::from(u16::try_from(total).unwrap_or(u16::MAX));
    let mut x = layout.runs[runs.start].x;
    for at in runs {
        let (space, count) = spaces_of(layout, &layout.runs[at]);
        let run = &mut layout.runs[at];
        run.x = x;
        if count > 0 {
            run.word_spacing = per_space;
            run.space = space;
            run.width += per_space * f32::from(count);
        }
        x += run.width;
    }
    layout.lines[index].width += free;
    true
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
