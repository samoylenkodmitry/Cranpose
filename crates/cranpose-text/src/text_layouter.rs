use std::ops::Range;

use cranpose_ui_graphics::{MemoryHeadroom, whole};

use crate::{
    bidi_level::{BidiLevel, reorder_visually},
    cluster_widths::ClusterWidths,
    font_family_resolver::FontFamilyResolver,
    itemizer::{Itemizer, TextItem},
    layout_input::LayoutInput,
    layout_options::{TextLayoutOptions, TextOverflow},
    line_box::paragraph_line_box,
    line_breaker::{
        BreakKind, BreakOpportunity, BrokenLine, LineBalancer, LineBreaker, LineEnd, WrapWidths,
        Wrapping, break_lines, hard_break, has_hard_break, starts_paragraph, visible_end,
    },
    line_runs::{line_extent, place_runs},
    paragraph::{LineBreak, ResolvedTextDirection},
    script::Script,
    shaped_item::{
        Piece, ShapedItem, face_index, hyphen, item_at, part_glyphs, scale, set_cluster,
        shape_inserted, spaced_width,
    },
    shaper::{ShapedGlyph, Shaper},
    text_layout::{LayoutLine, Sizing, TextLayout, WidthRange, font_units},
};
#[cfg(feature = "hyphenation")]
use crate::{
    hyphenator::Hyphenator,
    paragraph::{Hyphens, TextAlign},
};

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
    // The layout intrinsic widths are measured in and new layouts are built
    // in, which keeps its storage from one text to the next.
    scratch: TextLayout,
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

// Where a line stands among a layout's lines: the bottom of the line above,
// whether it is the first and the last, and whether it shows an ellipsis.
#[derive(Clone, Copy)]
struct LineEdges {
    top: f32,
    first: bool,
    last: bool,
    ellipsized: bool,
}

// Where a line that does not fit shows an ellipsis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EllipsisAt {
    Start,
    Middle,
    End,
}

const ELLIPSIS: &str = "\u{2026}";
// The most glyphs the scratch layout keeps room for between texts: the
// storage of a longer text goes once the text is laid out.
const SCRATCH_GLYPHS: usize = 4096;
// How much the free width of a paragraph's last line counts in even breaking
// for `LineBreak.Paragraph`, which lets the last line stay short.
const PARAGRAPH_LAST_LINE: f32 = 0.16;

impl TextLayouter {
    /// Lays `input` out with the typefaces of `resolver`.
    pub fn layout(&mut self, input: &LayoutInput<'_>, resolver: &FontFamilyResolver) -> TextLayout {
        self.layout_taking(input, resolver, None)
    }

    /// Lays `input` out as [`TextLayouter::layout`] does, with the glyphs of
    /// `shaped`, a layout of the same text in the same spans and typefaces
    /// at other widths, in place of shaping the text again: Compose's
    /// `Paragraph` made from the `ParagraphIntrinsics` of another.
    pub fn relayout(
        &mut self,
        input: &LayoutInput<'_>,
        resolver: &FontFamilyResolver,
        shaped: &TextLayout,
    ) -> TextLayout {
        self.layout_taking(input, resolver, Some(shaped))
    }

    fn layout_taking(
        &mut self,
        input: &LayoutInput<'_>,
        resolver: &FontFamilyResolver,
        shaped: Option<&TextLayout>,
    ) -> TextLayout {
        let mut scratch = std::mem::take(&mut self.scratch);
        self.fill(input, resolver, shaped, &mut scratch);
        // A clone holds each part in storage of its own length: a label's
        // layout keeps one run and one line, not the room a vector grows by.
        let layout = scratch.clone();
        self.keep_scratch(scratch);
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
        self.fill(input, resolver, None, layout);
    }

    // Lays `input` out into `layout`, with the glyphs of `shaped` when given.
    fn fill(
        &mut self,
        input: &LayoutInput<'_>,
        resolver: &FontFamilyResolver,
        shaped: Option<&TextLayout>,
        layout: &mut TextLayout,
    ) {
        let options = input.options.normalized();
        layout.clear();
        // A glyph takes at least a byte of text, so the glyphs grow once.
        layout.glyphs.reserve(input.text.len());
        self.shape(input, resolver, shaped, layout);
        self.measure_clusters(input, layout);
        let lengths = input.lengths;
        let text = input.text;
        // The width up to the trailing whitespace of a text with no line
        // break: its one paragraph.
        let unbroken =
            (!has_hard_break(text)).then(|| self.widths.width(0, visible_end(text, 0, text.len())));
        let widest = unbroken.map_or_else(
            || self.widest_paragraph(input),
            |width| width.max(0.0) + lengths.first_line_indent,
        );
        layout.sizing = Sizing {
            content_width: whole::ceil(widest * lengths.grid) / lengths.grid,
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
        // Most text is one line that fits; it needs no break opportunities.
        if let Some(line) = unbroken.filter(|&line| line <= wrap.first) {
            let min = if options.soft_wrap {
                line + lengths.first_line_indent
            } else {
                f32::NEG_INFINITY
            };
            let mut hold = WidthRange {
                min,
                max: f32::INFINITY,
            };
            // A line of one item that fits with no ellipsis and has no
            // whitespace after its text is that item.
            if self.items.len() == 1
                && line <= width - lengths.first_line_indent
                && visible_end(text, 0, text.len()) == text.len()
            {
                if ellipsis_at(&options, true).is_some() {
                    hold.narrow(line + lengths.first_line_indent, f32::INFINITY);
                }
                layout.hold = Some(hold);
                self.lay_out_one_line(input, &options, resolver, layout);
                return;
            }
            layout.hold = Some(hold);
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
        let mut measured = std::mem::take(&mut self.scratch);
        measured.clear();
        self.shape(input, resolver, None, &mut measured);
        self.measure_clusters(input, &measured);
        self.keep_scratch(measured);
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

    // Keeps `scratch` for the next text, unless its storage grew past what a
    // label or a paragraph needs.
    fn keep_scratch(&mut self, scratch: TextLayout) {
        if scratch.glyphs.capacity() <= SCRATCH_GLYPHS {
            self.scratch = scratch;
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

    /// Sizes the caches the layouter keeps by how much memory the device
    /// has left: tight memory takes them back to their least.
    pub fn set_memory_headroom(&mut self, headroom: MemoryHeadroom) {
        self.shaper.words.set_headroom(headroom);
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

    // Shapes the items of `input` into `layout`, or takes the glyphs
    // `shaped` placed for them, whose clusters fall in each item's text in
    // item order.
    fn shape(
        &mut self,
        input: &LayoutInput<'_>,
        resolver: &FontFamilyResolver,
        shaped: Option<&TextLayout>,
        layout: &mut TextLayout,
    ) {
        self.items.clear();
        let text = input.text;
        let (shaper, items) = (&mut self.shaper, &mut self.items);
        let direction = input.paragraph.text_direction;
        let mut taken = shaped.map(|shaped| &shaped.glyphs[..shaped.shaped]);
        self.itemizer
            .itemize(text, direction, input.spans, resolver, |item| {
                let start = layout.glyphs.len();
                match &mut taken {
                    Some(glyphs) => {
                        let count = glyphs
                            .iter()
                            .take_while(|glyph| item.range.contains(&glyph.text_offset()))
                            .count();
                        let (own, rest) = glyphs.split_at(count);
                        layout.glyphs.extend_from_slice(own);
                        *glyphs = rest;
                    }
                    None => shaper.shape(
                        text,
                        &item,
                        &input.spans[item.span],
                        0..text.len(),
                        &mut layout.glyphs,
                    ),
                }
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
        layout.shaped = layout.glyphs.len();
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
            let edges = LineEdges {
                top,
                first: index == 0,
                last: index == last,
                ellipsized,
            };
            top = self.push_line(input, resolver, line, edges, layout);
        }
        for index in 1..layout.lines.len() {
            let starts = starts_paragraph(input.text, layout.lines[index].text.start);
            layout.lines[index - 1].ends_paragraph = starts;
        }
        finish_lines(input, options, top, layout);
    }

    // Lays out the one line of a text whose one item fits it to its end, as
    // `build_lines` lays it out: the item is the line's one piece.
    fn lay_out_one_line(
        &mut self,
        input: &LayoutInput<'_>,
        options: &TextLayoutOptions,
        resolver: &FontFamilyResolver,
        layout: &mut TextLayout,
    ) {
        let item = &self.items[0];
        let span = &input.spans[item.span];
        let scale = scale(span, &layout.faces[item.face]);
        self.pieces.clear();
        self.pieces.push(Piece {
            width: spaced_width(&layout.glyphs[item.glyphs.clone()], scale, item.spacing),
            text: item.text.clone(),
            glyphs: item.glyphs.clone(),
            face: item.face,
            size: span.size,
            scale,
            spacing: item.spacing,
            baseline_shift: span.baseline_shift,
            level: item.level,
        });
        let end = input.text.len();
        let line = LineText {
            range: 0..end,
            visible: end,
            drawn: end,
            indent: input.lengths.first_line_indent,
        };
        let edges = LineEdges {
            top: 0.0,
            first: true,
            last: true,
            ellipsized: false,
        };
        let top = self.push_line(input, resolver, line, edges, layout);
        finish_lines(input, options, top, layout);
    }

    // Places the pieces just collected for `line` as its runs, adds the
    // line below `edges.top` and returns its bottom.
    fn push_line(
        &self,
        input: &LayoutInput<'_>,
        resolver: &FontFamilyResolver,
        line: LineText,
        edges: LineEdges,
        layout: &mut TextLayout,
    ) -> f32 {
        let (runs, width) = place_runs(&self.pieces, line.visible, layout);
        let extent = line_extent(&self.pieces, input, resolver, layout, line.range.start);
        let asked = input.lengths.line_height.unwrap_or(f32::NAN);
        let line_box = paragraph_line_box(input.paragraph, extent, asked, input.lengths.grid);
        let trim_top = if edges.first { line_box.trim_top } else { 0.0 };
        let trim_bottom = if edges.last {
            line_box.trim_bottom
        } else {
            0.0
        };
        let top = edges.top;
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
            ellipsized: edges.ellipsized,
            ends_paragraph: true,
        });
        layout.widest = layout.widest.max(width + line.indent);
        bottom
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

// Ends a layout whose lines reach down to `top`: a text short of its
// minimum lines takes the height of that many of its first line, and its
// lines align.
fn finish_lines(
    input: &LayoutInput<'_>,
    options: &TextLayoutOptions,
    mut top: f32,
    layout: &mut TextLayout,
) {
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
