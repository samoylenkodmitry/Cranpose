use std::ops::Range;

use cranpose_ui_graphics::{MemoryHeadroom, whole};
use smallvec::SmallVec;

use crate::{
    bidi_level::{BidiLevel, reorder_visually},
    ellipsis_at::{ELLIPSIS, EllipsisAt, ellipsis_at, take_rest_of_paragraph},
    font_family_resolver::FontFamilyResolver,
    index_range::narrow,
    itemizer::{Itemizer, TextItem},
    laid_glyphs::LaidGlyphs,
    layout_input::LayoutInput,
    layout_options::TextLayoutOptions,
    line_box::paragraph_line_box,
    line_breaker::{
        BreakKind, BreakOpportunity, BrokenLine, LineBalancer, LineBreaker, LineEnd, WrapWidths,
        Wrapping, break_lines, hard_break, has_hard_break, starts_paragraph, visible_end,
    },
    line_runs::{line_extent, place_runs},
    line_text::{LineEdges, LineText, before_hard_break, indent},
    paragraph::{LineBreak, ResolvedTextDirection},
    paragraph_intrinsics::ParagraphIntrinsics,
    paragraph_measures::ParagraphMeasures,
    script::Script,
    shaped_glyphs::ShapedGlyphs,
    shaped_item::{
        Piece, ShapedItem, face_index, hyphen, item_at, part_glyphs, scale, set_cluster,
        shape_inserted, spaced_width,
    },
    shaper::Shaper,
    text_layout::{LayoutBody, LayoutLine, TextLayout, WidthRange},
    typeface::Typeface,
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
    // The cluster widths and break opportunities of the text laid out last,
    // which keep their storage from one text to the next.
    measures: ParagraphMeasures,
    lines: Vec<BrokenLine>,
    pieces: Vec<Piece>,
    // The glyphs of the text being laid out, which keep their storage from
    // one text to the next; a layout takes them once they are all placed.
    glyphs: ShapedGlyphs,
    // The faces intrinsic widths are measured in, which keep their storage
    // from one text to the next.
    faces: SmallVec<[Typeface; 1]>,
    #[cfg(feature = "hyphenation")]
    hyphenator: Hyphenator,
    // The break opportunities with the hyphenation points among them, and
    // a word's points.
    #[cfg(feature = "hyphenation")]
    hyphenated: Vec<BreakOpportunity>,
    #[cfg(feature = "hyphenation")]
    points: Vec<usize>,
}

// The most glyphs the layouter keeps room for between texts: the storage of
// a longer text goes once the text is laid out.
const SCRATCH_GLYPHS: usize = 4096;
// How much the free width of a paragraph's last line counts in even breaking
// for `LineBreak.Paragraph`, which lets the last line stay short.
const PARAGRAPH_LAST_LINE: f32 = 0.16;

impl TextLayouter {
    /// Lays `input` out with the typefaces of `resolver`.
    pub fn layout(&mut self, input: &LayoutInput<'_>, resolver: &FontFamilyResolver) -> TextLayout {
        self.layout_taking(input, resolver, None)
    }

    /// Lays `input` out as [`TextLayouter::layout`] does, with the glyphs
    /// `shaped` placed for the same text in the same typefaces in place of
    /// shaping it again.
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
        let mut faces = SmallVec::new();
        self.shape(input, resolver, shaped, &mut faces);
        self.place(input, resolver, shaped, faces)
    }

    // Places the items just shaped in `faces` into a new layout. Out of
    // line, so the layout and the placing stay off the stack of the
    // shaping, which runs deepest.
    #[inline(never)]
    fn place(
        &mut self,
        input: &LayoutInput<'_>,
        resolver: &FontFamilyResolver,
        shaped: Option<&TextLayout>,
        faces: SmallVec<[Typeface; 1]>,
    ) -> TextLayout {
        let mut body = LayoutBody {
            faces,
            ..LayoutBody::default()
        };
        let width = self.fill(input, resolver, shaped, &mut body);
        // A layout kept holds each part in storage of its own length.
        body.shrink_to_fit();
        TextLayout::new(body, width, input.max_width)
    }

    /// Lays `input` out into `layout` in place of what it held, in the
    /// storage of its lines and runs when no copy of it shares them.
    pub fn layout_into(
        &mut self,
        input: &LayoutInput<'_>,
        resolver: &FontFamilyResolver,
        layout: &mut TextLayout,
    ) {
        layout.refill(input.max_width, |body| {
            body.clear();
            self.shape(input, resolver, None, &mut body.faces);
            self.fill(input, resolver, None, body)
        });
    }

    // Lays the items just shaped of `input` out into `layout`, with what
    // `shaped` holds when given, and returns the width its lines are placed
    // across. Out of line, as `place` is.
    #[inline(never)]
    fn fill(
        &mut self,
        input: &LayoutInput<'_>,
        resolver: &FontFamilyResolver,
        shaped: Option<&TextLayout>,
        layout: &mut LayoutBody,
    ) -> f32 {
        let options = input.options.normalized();
        layout.shaped = self.glyphs.len();
        layout.glyphs = shaped
            .and_then(|shaped| shaped.body().shaped_glyphs())
            .cloned()
            .unwrap_or_default();
        let lengths = input.lengths;
        let text = input.text;
        // The piece of a text of one item with no line break and no
        // whitespace at its end, which is its line when it fits.
        let mut lone = (self.items.len() == 1
            && !has_hard_break(text)
            && visible_end(text, 0, text.len()) == text.len())
        .then(|| self.lone_piece(input, layout));
        // The width up to the trailing whitespace of a text with no line
        // break: its one paragraph.
        let unbroken = if let Some(piece) = &lone {
            Some(piece.width)
        } else {
            self.measure_widths(input, &layout.faces);
            (!has_hard_break(text)).then(|| {
                self.measures
                    .widths
                    .width(0, visible_end(text, 0, text.len()))
            })
        };
        let widest = unbroken.map_or_else(
            || self.widest_paragraph(input),
            |width| width.max(0.0) + lengths.first_line_indent,
        );
        layout.content_width = whole::ceil(widest * lengths.grid) / lengths.grid;
        layout.follows_max = options.soft_wrap || options.overflow.is_ellipsis();
        let width = layout.width_between(input.min_width, input.max_width);
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
        let fits = unbroken.filter(|&line| line <= wrap.first);
        // A lone item that fits with no ellipsis is the line, and needs no
        // cluster widths.
        if let Some(piece) =
            lone.take_if(|piece| fits.is_some() && piece.width <= width - lengths.first_line_indent)
        {
            let line = piece.width + lengths.first_line_indent;
            let mut hold = fitting_hold(&options, line);
            if ellipsis_at(&options).is_some() {
                hold.narrow(line, f32::INFINITY);
            }
            layout.hold = Some(hold);
            self.lay_out_one_line(input, &options, resolver, piece, width, layout);
            return width;
        }
        if lone.is_some() {
            self.measure_widths(input, &layout.faces);
        }
        if let Some(line) = fits {
            layout.hold = Some(fitting_hold(&options, line + lengths.first_line_indent));
            self.lines.clear();
            self.lines.push(BrokenLine::plain(0..text.len()));
        } else {
            self.wrap_lines(input, &options, wrap, layout);
        }
        layout.did_overflow_height = self.lines.len() > options.max_lines;
        if layout.did_overflow_height && ellipsis_at(&options).is_some() {
            take_rest_of_paragraph(&mut self.lines, text, options.max_lines - 1);
        }
        self.lines.truncate(options.max_lines);
        self.build_lines(input, &options, resolver, width, layout);
        width
    }

    // Breaks `input` into lines of the `wrap` widths, at the opportunities
    // its line break style takes, and sets the layout's hold.
    fn wrap_lines(
        &mut self,
        input: &LayoutInput<'_>,
        options: &TextLayoutOptions,
        wrap: WrapWidths,
        layout: &mut LayoutBody,
    ) {
        let text = input.text;
        let line_break = input.paragraph.line_break;
        self.breaker
            .opportunities(text, line_break, &mut self.measures.breaks);
        #[cfg(feature = "hyphenation")]
        let breaks =
            if input.paragraph.hyphens == Hyphens::Auto && self.hyphenate(input, wrap, layout) {
                &self.hyphenated
            } else {
                &self.measures.breaks
            };
        #[cfg(not(feature = "hyphenation"))]
        let breaks = &self.measures.breaks;
        let wrapping = Wrapping {
            text,
            widths: &self.measures.widths,
            wrap,
        };
        match line_break {
            LineBreak::Paragraph => {
                self.balancer
                    .balance(&wrapping, breaks, PARAGRAPH_LAST_LINE, &mut self.lines);
            }
            LineBreak::Heading => {
                self.balancer
                    .balance(&wrapping, breaks, 1.0, &mut self.lines);
            }
            LineBreak::Simple | LineBreak::Unspecified => {
                break_lines(&wrapping, breaks, &mut self.lines);
            }
        }
        layout.hold = self.hold(input, options, &wrapping, breaks);
    }

    /// The intrinsic widths of `input`, laid out with the typefaces of
    /// `resolver`; its width and options do not count.
    pub fn intrinsics(
        &mut self,
        input: &LayoutInput<'_>,
        resolver: &FontFamilyResolver,
    ) -> ParagraphIntrinsics {
        let mut faces = std::mem::take(&mut self.faces);
        faces.clear();
        self.shape(input, resolver, None, &mut faces);
        self.measure_widths(input, &faces);
        self.faces = faces;
        self.keep_glyph_room();
        let text = input.text;
        self.breaker
            .opportunities(text, input.paragraph.line_break, &mut self.measures.breaks);
        let max_intrinsic_width = self.widest_paragraph(input);
        // An empty text, or one that ends with a line break, ends with an
        // empty line.
        let empty_last = self
            .measures
            .breaks
            .last()
            .is_none_or(|opportunity| opportunity.kind == BreakKind::Hard);
        let mut min_intrinsic_width = if empty_last {
            input.lengths.first_line_indent
        } else {
            0.0
        };
        let mut word = 0;
        for opportunity in &self.measures.breaks {
            let width = self
                .measures
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

    // Measures the widths of the clusters shaped last, in storage of the
    // layouter's own.
    fn measure_widths(&mut self, input: &LayoutInput<'_>, faces: &[Typeface]) {
        self.measures
            .widths
            .measure(&self.items, input, faces, &self.glyphs);
    }

    // Gives back the storage of the glyphs when it grew past what a label or
    // a paragraph needs.
    fn keep_glyph_room(&mut self) {
        if self.glyphs.capacity() > SCRATCH_GLYPHS {
            self.glyphs = ShapedGlyphs::default();
        }
    }

    // Ends a layout whose lines reach down to `top`: the layout takes the
    // glyphs, a text short of its minimum lines takes the height of that
    // many of its first line, and its lines align across `width`.
    fn finish_lines(
        &mut self,
        input: &LayoutInput<'_>,
        options: &TextLayoutOptions,
        (mut top, width): (f32, f32),
        layout: &mut LayoutBody,
    ) {
        // A relayout that added no glyphs keeps those it took.
        if layout.glyphs.len() != self.glyphs.len() {
            layout.glyphs = LaidGlyphs::new(&self.glyphs);
        }
        self.keep_glyph_room();
        if layout.lines.len() < options.min_lines {
            let first = &layout.lines[0];
            let missing = options.min_lines - layout.lines.len();
            top += (first.bottom - first.top) * lines_f32(missing);
        }
        layout.height = top;
        layout.align_lines(width, input.paragraph.text_align);
    }

    // The widest paragraph of the text measured last on one line, with its
    // first line's indent.
    fn widest_paragraph(&self, input: &LayoutInput<'_>) -> f32 {
        let text = input.text;
        let mut widest: f32 = 0.0;
        let mut start = 0;
        for (at, ch) in text.char_indices() {
            if hard_break(ch) {
                widest = widest.max(
                    self.measures
                        .widths
                        .width(start, visible_end(text, start, at)),
                );
                start = at + ch.len_utf8();
            }
        }
        let last = self
            .measures
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

    // Gathers in `hyphenated` the break opportunities with the hyphenation
    // points of the words a line may break inside: every word for even
    // breaking, and for greedy breaking the words wider than a line, the
    // only ones it hyphenates. False when lines take any width, so none
    // breaks inside a word.
    #[cfg(feature = "hyphenation")]
    fn hyphenate(
        &mut self,
        input: &LayoutInput<'_>,
        wrap: WrapWidths,
        layout: &mut LayoutBody,
    ) -> bool {
        if !wrap.first.is_finite() {
            return false;
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
        self.hyphenated.clear();
        let mut word_start = 0;
        for index in 0..self.measures.breaks.len() {
            let opportunity = self.measures.breaks[index];
            let word_end = visible_end(input.text, word_start, opportunity.offset);
            if self.measures.widths.width(word_start, word_end) > wider_than {
                self.hyphenate_word(input, word_start..word_end, penalty_per_size, layout);
            }
            self.hyphenated.push(opportunity);
            word_start = opportunity.offset;
        }
        true
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
        layout: &mut LayoutBody,
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
            if self.measures.widths.next_boundary(at.saturating_sub(1)) != Some(at) {
                continue;
            }
            let index = item_at(&self.items, at.saturating_sub(1));
            let item = &self.items[index];
            let width = match hyphen_width {
                Some((known, width)) if known == index => width,
                _ => {
                    let face = &layout.faces[item.face];
                    let shaped = self.glyphs.len();
                    let text = hyphen(face);
                    let item = item.inserted(text, face);
                    let width =
                        shape_inserted(&mut self.shaper, input, &item, text, &mut self.glyphs);
                    self.glyphs.truncate(shaped);
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
        breaks: &[BreakOpportunity],
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
            let (min, max) = wrapping.greedy_limits(line, breaks);
            let indent = indent(input, line.text.start);
            hold.narrow(min + indent, max + indent);
        }
        Some(hold)
    }

    // Shapes the items of `input` with the typefaces it adds to `faces`, or
    // takes the glyphs `shaped` placed for them, whose clusters fall in each
    // item's text in item order.
    fn shape(
        &mut self,
        input: &LayoutInput<'_>,
        resolver: &FontFamilyResolver,
        shaped: Option<&TextLayout>,
        faces: &mut SmallVec<[Typeface; 1]>,
    ) {
        self.items.clear();
        self.glyphs.clear();
        // A glyph takes at least a byte of text, so the glyphs grow once.
        self.glyphs.reserve(input.text.len());
        let text = input.text;
        let (shaper, items, glyphs) = (&mut self.shaper, &mut self.items, &mut self.glyphs);
        let direction = input.paragraph.text_direction;
        let mut taken = shaped.map(|shaped| (shaped.body(), 0));
        self.itemizer
            .itemize(text, direction, input.spans, resolver, |item| {
                let start = glyphs.len();
                match &mut taken {
                    Some((body, from)) => {
                        *from = body.glyphs.take(*from..body.shaped, &item.range, glyphs);
                    }
                    None => {
                        shaper.shape(text, &item, &input.spans[item.span], 0..text.len(), glyphs);
                    }
                }
                items.push(ShapedItem {
                    text: item.range,
                    glyphs: start..glyphs.len(),
                    face: face_index(faces, item.typeface),
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

    fn build_lines(
        &mut self,
        input: &LayoutInput<'_>,
        options: &TextLayoutOptions,
        resolver: &FontFamilyResolver,
        width: f32,
        layout: &mut LayoutBody,
    ) {
        let mut top = 0.0;
        let last = self.lines.len() - 1;
        // Most lines are one run.
        layout.lines.reserve_exact(self.lines.len());
        layout.runs.reserve_exact(self.lines.len());
        for index in 0..=last {
            let range = self.lines[index].text.clone();
            let indent = indent(input, range.start);
            let line = LineText {
                visible: visible_end(input.text, range.start, range.end),
                drawn: before_hard_break(input.text, range.start, range.end),
                indent,
                room: width - indent,
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
            let starts = starts_paragraph(input.text, layout.lines[index].text.start());
            layout.lines[index - 1].ends_paragraph = starts;
        }
        self.finish_lines(input, options, (top, width), layout);
    }

    // The piece of the one item of a text: the whole item.
    fn lone_piece(&self, input: &LayoutInput<'_>, layout: &LayoutBody) -> Piece {
        let item = &self.items[0];
        let span = &input.spans[item.span];
        let scale = scale(span, &layout.faces[item.face]);
        Piece {
            width: spaced_width(&self.glyphs[item.glyphs.clone()], scale, item.spacing),
            text: item.text.clone(),
            glyphs: item.glyphs.clone(),
            face: item.face,
            size: span.size,
            scale,
            spacing: item.spacing,
            baseline_shift: span.baseline_shift,
            level: item.level,
        }
    }

    // Lays out the one line of a text whose one item fits it to its end, as
    // `build_lines` lays it out: the item's `piece` is the line's one piece.
    fn lay_out_one_line(
        &mut self,
        input: &LayoutInput<'_>,
        options: &TextLayoutOptions,
        resolver: &FontFamilyResolver,
        piece: Piece,
        width: f32,
        layout: &mut LayoutBody,
    ) {
        self.pieces.clear();
        self.pieces.push(piece);
        let end = input.text.len();
        let indent = input.lengths.first_line_indent;
        let line = LineText {
            range: 0..end,
            visible: end,
            drawn: end,
            indent,
            room: width - indent,
        };
        let edges = LineEdges {
            top: 0.0,
            first: true,
            last: true,
            ellipsized: false,
        };
        let top = self.push_line(input, resolver, line, edges, layout);
        self.finish_lines(input, options, (top, width), layout);
    }

    // Places the pieces just collected for `line` as its runs, adds the
    // line below `edges.top` and returns its bottom.
    fn push_line(
        &self,
        input: &LayoutInput<'_>,
        resolver: &FontFamilyResolver,
        line: LineText,
        edges: LineEdges,
        layout: &mut LayoutBody,
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
            text: line.range.into(),
            visible_end: narrow(line.visible),
            runs: runs.into(),
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
        layout: &mut LayoutBody,
    ) -> bool {
        self.pieces.clear();
        let last = index + 1 == self.lines.len();
        let visible_width = self.measures.widths.width(line.range.start, line.visible);
        let overflows = visible_width > line.room;
        let ellipsis = ellipsis_at(options).filter(|_| last);
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
        layout: &mut LayoutBody,
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
        let room = (line.room - ellipsis.width).max(0.0);
        let text = input.text;
        let (head, tail) = match at {
            EllipsisAt::End => {
                let cut = self
                    .measures
                    .widths
                    .last_fitting(range.start, visible, room);
                if let Some(hold) = &mut layout.hold {
                    // The cut stays while its text fits and one more cluster
                    // does not.
                    let edge = line.indent + ellipsis.width;
                    let min = if cut > range.start {
                        self.measures.widths.width(range.start, cut) + edge
                    } else {
                        f32::NEG_INFINITY
                    };
                    let max = match self.measures.widths.next_boundary(cut) {
                        Some(next) if cut < visible => {
                            self.measures.widths.width(range.start, next) + edge
                        }
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
                    self.measures
                        .widths
                        .first_fitting(range.start, visible, room),
                )
            }
            EllipsisAt::Middle => {
                // The end takes up to half the room and the start the rest,
                // as Android cuts a middle ellipsis.
                layout.hold = None;
                let tail = self
                    .measures
                    .widths
                    .first_fitting(range.start, visible, room / 2.0);
                let used = self.measures.widths.width(tail, visible);
                (
                    self.measures
                        .widths
                        .last_fitting(range.start, tail, room - used),
                    tail,
                )
            }
        };
        set_cluster(&mut self.glyphs.glyphs_mut()[ellipsis.glyphs.clone()], head);
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
        layout: &mut LayoutBody,
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
        let start = self.glyphs.len();
        let width = shape_inserted(&mut self.shaper, input, &item, ELLIPSIS, &mut self.glyphs);
        Some(Piece {
            width,
            text: 0..0,
            glyphs: start..self.glyphs.len(),
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
    fn push_hyphen(&mut self, input: &LayoutInput<'_>, at: usize, layout: &mut LayoutBody) {
        let Some(item) = self.items.get(item_at(&self.items, at.saturating_sub(1))) else {
            return;
        };
        let span = &input.spans[item.span];
        let face = &layout.faces[item.face];
        let hyphen = hyphen(face);
        let start = self.glyphs.len();
        let width = shape_inserted(
            &mut self.shaper,
            input,
            &item.inserted(hyphen, face),
            hyphen,
            &mut self.glyphs,
        );
        set_cluster(&mut self.glyphs.glyphs_mut()[start..], at);
        self.pieces.push(Piece {
            width,
            text: at..at,
            glyphs: start..self.glyphs.len(),
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
        layout: &mut LayoutBody,
    ) {
        let Self {
            items,
            shaper,
            pieces,
            glyphs: all,
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
                let glyphs = part_glyphs(
                    shaper,
                    input,
                    item,
                    sub.clone(),
                    part.clone(),
                    &layout.faces,
                    all,
                );
                let scale = scale(&input.spans[item.span], &layout.faces[item.face]);
                pieces.push(Piece {
                    width: spaced_width(&all[glyphs.clone()], scale, item.spacing),
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

// The hold of a text that fits on one line `line` wide with its indent:
// any width that keeps it on one line, when it wraps.
fn fitting_hold(options: &TextLayoutOptions, line: f32) -> WidthRange {
    WidthRange {
        min: if options.soft_wrap {
            line
        } else {
            f32::NEG_INFINITY
        },
        max: f32::INFINITY,
    }
}

// A line count as a float; counts beyond `u16` saturate.
fn lines_f32(count: usize) -> f32 {
    f32::from(u16::try_from(count).unwrap_or(u16::MAX))
}
