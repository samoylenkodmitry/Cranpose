use std::ops::Range;

use cranpose_ui_graphics::whole;

use crate::{
    bidi_level::{BidiLevel, reorder_visually},
    cluster_widths::ClusterWidths,
    font_family_resolver::FontFamilyResolver,
    itemizer::{Itemizer, TextItem},
    layout_options::{TextLayoutOptions, TextOverflow},
    line_box::{FontExtent, paragraph_line_box},
    line_breaker::{
        BreakKind, BreakOpportunity, BrokenLine, LineBalancer, LineBreaker, LineEnd, WrapWidths,
        Wrapping, break_lines, hard_break, has_hard_break, starts_paragraph, visible_end,
    },
    paragraph::{LineBreak, ResolvedTextDirection},
    script::Script,
    shaped_item::{
        Piece, ShapedItem, face_index, hyphen, item_at, part_glyphs, scale, set_cluster,
        shape_inserted, spaced_width,
    },
    shaper::{ShapedGlyph, Shaper},
    shaping_span::ShapingSpan,
    style::ParagraphStyle,
    text_layout::{LayoutLine, LineRun, Sizing, TextLayout, WidthRange, font_units},
    typeface::Typeface,
};
#[cfg(feature = "hyphenation")]
use crate::{
    hyphenator::Hyphenator,
    paragraph::{Hyphens, TextAlign},
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
    /// The narrowest the paragraph may be, in pixels.
    pub min_width: f32,
    /// The widest the paragraph may be, in pixels; infinite for lines that
    /// end only at line breaks. As Compose sizes a paragraph, it takes its
    /// max intrinsic width rounded up to whole device pixels, within these
    /// two, and the widest only when they are equal; its lines wrap at that
    /// width when they soft wrap, and ellipses cut to it.
    pub max_width: f32,
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
    // The glyphs of a text measured for its intrinsic widths.
    measured: TextLayout,
    #[cfg(feature = "hyphenation")]
    hyphenator: Hyphenator,
    // The opportunities with the hyphenation points among them, swapped
    // with `breaks` once made, and a word's points.
    #[cfg(feature = "hyphenation")]
    hyphenated: Vec<BreakOpportunity>,
    #[cfg(feature = "hyphenation")]
    points: Vec<usize>,
}

/// The widths of a paragraph that do not depend on the width it is laid
/// out at: Compose's `ParagraphIntrinsics`. Each includes the indent of the
/// line it is measured on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParagraphIntrinsics {
    /// The widest word, the narrowest width no line breaks inside a word at.
    pub min_intrinsic_width: f32,
    /// The widest paragraph laid out on one line, the narrowest width no
    /// paragraph wraps at.
    pub max_intrinsic_width: f32,
}

// A line's text, the end of its visible text, the end of the text it draws,
// before the hard break that ends it, which draws nothing, and its indent.
struct LineText {
    range: Range<usize>,
    visible: usize,
    drawn: usize,
    indent: f32,
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
        layout.sizing = Sizing {
            content_width: whole::ceil(self.widest_paragraph(input) * lengths.grid) / lengths.grid,
            follows_max: options.soft_wrap || options.overflow.is_ellipsis(),
        };
        let width = layout.sizing.width(input.min_width, input.max_width);
        layout.width = width;
        let wrap = if options.soft_wrap {
            WrapWidths {
                first: width - lengths.first_line_indent,
                rest: width - lengths.rest_line_indent,
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
            let min = if options.soft_wrap {
                self.widths.width(0, visible_end(text, 0, text.len())) + lengths.first_line_indent
            } else {
                f32::NEG_INFINITY
            };
            layout.hold = Some(WidthRange {
                min,
                max: f32::INFINITY,
            });
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
            layout.hold = self.hold(input, &options, &wrapping);
        }
        layout.did_overflow_height = self.lines.len() > options.max_lines;
        if layout.did_overflow_height && ellipsis_at(&options, true).is_some() {
            take_rest_of_paragraph(&mut self.lines, text, options.max_lines - 1);
        }
        self.lines.truncate(options.max_lines);
        self.build_lines(input, &options, resolver, layout);
    }

    /// The intrinsic widths of `input`, laid out with the typefaces of
    /// `resolver`; its width and options do not count.
    pub fn intrinsics(
        &mut self,
        input: &LayoutInput<'_>,
        resolver: &FontFamilyResolver,
    ) -> ParagraphIntrinsics {
        let mut measured = std::mem::take(&mut self.measured);
        measured.clear();
        self.shape(input, resolver, &mut measured);
        self.measure_clusters(input, &measured);
        self.measured = measured;
        let text = input.text;
        self.breaker
            .opportunities(text, input.paragraph.line_break, &mut self.breaks);
        let max_intrinsic_width = self.widest_paragraph(input);
        // An empty text, or one that ends with a line break, ends with an
        // empty line.
        let empty_last = self
            .breaks
            .last()
            .is_none_or(|opportunity| opportunity.kind == BreakKind::Hard);
        let mut min_intrinsic_width = if empty_last {
            input.lengths.first_line_indent
        } else {
            0.0
        };
        let mut word = 0;
        for opportunity in &self.breaks {
            let width = self
                .widths
                .width(word, visible_end(text, word, opportunity.offset));
            min_intrinsic_width = min_intrinsic_width.max(width + indent(input, word));
            word = opportunity.offset;
        }
        ParagraphIntrinsics {
            min_intrinsic_width,
            max_intrinsic_width,
        }
    }

    // The widest paragraph of the text measured last on one line, with its
    // first line's indent.
    fn widest_paragraph(&self, input: &LayoutInput<'_>) -> f32 {
        let text = input.text;
        let mut widest: f32 = 0.0;
        let mut start = 0;
        for (at, ch) in text.char_indices() {
            if hard_break(ch) {
                widest = widest.max(self.widths.width(start, visible_end(text, start, at)));
                start = at + ch.len_utf8();
            }
        }
        let last = self
            .widths
            .width(start, visible_end(text, start, text.len()));
        widest.max(last) + input.lengths.first_line_indent
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

    // The widths at which breaking gives the same lines: every width when
    // lines do not wrap, for greedy breaking the limits each line keeps its
    // end over moved by its indent, and none for even breaking, whose lines
    // can move with any width.
    fn hold(
        &self,
        input: &LayoutInput<'_>,
        options: &TextLayoutOptions,
        wrapping: &Wrapping<'_>,
    ) -> Option<WidthRange> {
        if !options.soft_wrap {
            return Some(WidthRange::ALL);
        }
        if !matches!(
            input.paragraph.line_break,
            LineBreak::Simple | LineBreak::Unspecified
        ) {
            return None;
        }
        let mut hold = WidthRange::ALL;
        for line in &self.lines {
            let (min, max) = wrapping.greedy_limits(line, &self.breaks);
            let indent = indent(input, line.text.start);
            hold.narrow(min + indent, max + indent);
        }
        Some(hold)
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
                        let span = &input.spans[item.span];
                        span.letter_spacing
                            .unwrap_or_else(|| item.typeface.tracking(span.size))
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
        let last = self.lines.len() - 1;
        for index in 0..=last {
            let range = self.lines[index].text.clone();
            let line = LineText {
                visible: visible_end(input.text, range.start, range.end),
                drawn: before_hard_break(input.text, range.start, range.end),
                indent: indent(input, range.start),
                range,
            };
            let ellipsized = self.collect_line(input, options, resolver, (index, &line), layout);
            reorder_visually(&mut self.pieces, |piece| piece.level);
            let (runs, width) = self.place_runs(line.visible, layout);
            let extent = self.line_extent(input, resolver, layout, line.range.start);
            let asked = input.lengths.line_height.unwrap_or(f32::NAN);
            let line_box = paragraph_line_box(input.paragraph, extent, asked, input.lengths.grid);
            let trim_top = if index == 0 { line_box.trim_top } else { 0.0 };
            let trim_bottom = if index == last {
                line_box.trim_bottom
            } else {
                0.0
            };
            let bottom = top + line_box.height - trim_top - trim_bottom;
            layout.lines.push(LayoutLine {
                level: self.paragraph_level(input, line.range.start),
                text: line.range,
                visible_end: line.visible,
                runs,
                top,
                baseline: top + line_box.baseline - trim_top,
                bottom,
                left: line.indent,
                indent: line.indent,
                width,
                stretch: 0.0,
                ellipsized,
                ends_paragraph: true,
            });
            top = bottom;
            layout.widest = layout.widest.max(width + line.indent);
        }
        for index in 1..layout.lines.len() {
            let starts = starts_paragraph(input.text, layout.lines[index].text.start);
            layout.lines[index - 1].ends_paragraph = starts;
        }
        if layout.lines.len() < options.min_lines {
            let first = &layout.lines[0];
            let missing = options.min_lines - layout.lines.len();
            top += (first.bottom - first.top) * lines_f32(missing);
        }
        layout.height = top;
        layout.align_lines(input.paragraph.text_align);
        layout.did_overflow_width = layout.widest > layout.width || layout.width > input.max_width;
        layout.renew_id();
    }

    // Collects the pieces of line `index`, with an ellipsis where it does
    // not fit and the options ask for one, or the hyphen of a hyphenated
    // line, and narrows the layout's hold to the widths that keep the
    // ellipsis where it is. True when the line shows an ellipsis.
    fn collect_line(
        &mut self,
        input: &LayoutInput<'_>,
        options: &TextLayoutOptions,
        resolver: &FontFamilyResolver,
        (index, line): (usize, &LineText),
        layout: &mut TextLayout,
    ) -> bool {
        self.pieces.clear();
        let last = index + 1 == self.lines.len();
        let visible_width = self.widths.width(line.range.start, line.visible);
        let overflows = visible_width > layout.width - line.indent;
        let ellipsis = ellipsis_at(options, last);
        let shown = ellipsis.filter(|_| overflows || (last && layout.did_overflow_height));
        if let Some(hold) = &mut layout.hold {
            if ellipsis.is_some() && shown.is_none() {
                hold.narrow(visible_width + line.indent, f32::INFINITY);
            }
            if shown.is_some() && overflows {
                hold.narrow(f32::NEG_INFINITY, visible_width + line.indent);
            }
        }
        let ellipsized = match shown {
            Some(at) => self.ellipsize(input, resolver, at, line, layout),
            None => false,
        };
        if !ellipsized {
            self.collect_pieces(input, line.range.start..line.drawn, line.visible, layout);
            if self.lines[index].end == LineEnd::Hyphen {
                self.push_hyphen(input, line.range.end, layout);
            }
        }
        ellipsized
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
            let space = layout.faces[piece.face].glyph(' ');
            let spaces = if piece.text.start < visible {
                layout.glyphs[piece.glyphs.clone()]
                    .iter()
                    .filter(|glyph| Some(glyph.id) == space)
                    .count()
            } else {
                0
            };
            layout.runs.push(LineRun {
                text: piece.text.clone(),
                glyphs: piece.glyphs.clone(),
                face: piece.face,
                size: piece.size,
                scale: piece.scale,
                spacing: piece.spacing,
                space,
                spaces: u16::try_from(spaces).unwrap_or(u16::MAX),
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

    // Collects the pieces of a line that does not fit with an ellipsis where
    // `at` says, cutting its text at cluster boundaries so the rest and the
    // ellipsis fit the width. False when no typeface draws an ellipsis.
    fn ellipsize(
        &mut self,
        input: &LayoutInput<'_>,
        resolver: &FontFamilyResolver,
        at: EllipsisAt,
        line: &LineText,
        layout: &mut TextLayout,
    ) -> bool {
        let (range, visible) = (line.range.clone(), line.visible);
        // The ellipsis takes the style of the text its line starts with.
        let span = input
            .spans
            .iter()
            .position(|span| span.range.contains(&range.start))
            .unwrap_or(0);
        let level = self.paragraph_level(input, range.start);
        let Some(mut ellipsis) = self.ellipsis_piece(input, resolver, span, level, layout) else {
            return false;
        };
        let room = (layout.width - line.indent - ellipsis.width).max(0.0);
        let text = input.text;
        let (head, tail) = match at {
            EllipsisAt::End => {
                let cut = self.widths.last_fitting(range.start, visible, room);
                if let Some(hold) = &mut layout.hold {
                    // The cut stays while its text fits and one more cluster
                    // does not.
                    let edge = line.indent + ellipsis.width;
                    let min = if cut > range.start {
                        self.widths.width(range.start, cut) + edge
                    } else {
                        f32::NEG_INFINITY
                    };
                    let max = match self.widths.next_boundary(cut) {
                        Some(next) if cut < visible => self.widths.width(range.start, next) + edge,
                        _ => f32::INFINITY,
                    };
                    hold.narrow(min, max);
                }
                (visible_end(text, range.start, cut), range.end)
            }
            EllipsisAt::Start => {
                layout.hold = None;
                (
                    range.start,
                    self.widths.first_fitting(range.start, visible, room),
                )
            }
            EllipsisAt::Middle => {
                // The end takes up to half the room and the start the rest,
                // as Android cuts a middle ellipsis.
                layout.hold = None;
                let tail = self.widths.first_fitting(range.start, visible, room / 2.0);
                let used = self.widths.width(tail, visible);
                (
                    self.widths.last_fitting(range.start, tail, room - used),
                    tail,
                )
            }
        };
        set_cluster(&mut layout.glyphs[ellipsis.glyphs.clone()], head);
        ellipsis.text = head..head;
        self.collect_pieces(input, range.start..head, visible, layout);
        self.pieces.push(ellipsis);
        if tail < line.drawn {
            self.collect_pieces(input, tail..line.drawn, visible, layout);
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
        // A span shifted up reaches higher above the line's baseline, and one
        // shifted down lower, as Android widens a run's metrics by its shift.
        self.pieces
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
// middle ellipsis only on a single line, as Android lays them out; text of
// more lines clips.
fn ellipsis_at(options: &TextLayoutOptions, last: bool) -> Option<EllipsisAt> {
    match options.overflow {
        TextOverflow::Ellipsis if last => Some(EllipsisAt::End),
        TextOverflow::StartEllipsis if last && options.max_lines == 1 => Some(EllipsisAt::Start),
        TextOverflow::MiddleEllipsis if last && options.max_lines == 1 => Some(EllipsisAt::Middle),
        _ => None,
    }
}

// Makes line `last` run to the end of its paragraph over the lines after it
// a line limit drops, so its ellipsis cuts the paragraph where it stops
// fitting, as Android's StaticLayout treats them as one line.
fn take_rest_of_paragraph(lines: &mut [BrokenLine], text: &str, last: usize) {
    let end = lines[last + 1..]
        .iter()
        .take_while(|line| !starts_paragraph(text, line.text.start))
        .last()
        .map_or(lines[last].text.end, |line| line.text.end);
    lines[last] = BrokenLine::plain(lines[last].text.start..end);
}

// The end of the text from `start` to `end` before the hard break that ends
// it, if one does.
fn before_hard_break(text: &str, start: usize, end: usize) -> usize {
    text.get(start..end)
        .map_or(end, |part| start + part.trim_end_matches(hard_break).len())
}

// The indent of the line starting at `offset`.
fn indent(input: &LayoutInput<'_>, offset: usize) -> f32 {
    if starts_paragraph(input.text, offset) {
        input.lengths.first_line_indent
    } else {
        input.lengths.rest_line_indent
    }
}

// A line count as a float; counts beyond `u16` saturate.
fn lines_f32(count: usize) -> f32 {
    f32::from(u16::try_from(count).unwrap_or(u16::MAX))
}
