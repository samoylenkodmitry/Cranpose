use std::{
    ops::Range,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
};

use cranpose_ui_graphics::Rect;
use smallvec::SmallVec;

use crate::{
    bidi_level::BidiLevel,
    glyph_id::GlyphId,
    index_range::{IndexRange, widen},
    laid_glyphs::LaidGlyphs,
    paragraph::TextAlign,
    shaped_glyphs::{GlyphOffset, ShapedGlyph},
    typeface::Typeface,
};

/// A paragraph laid out in lines: glyph runs placed for drawing, and the
/// queries of Compose's `TextLayoutResult`. Distances are in pixels from the
/// layout's top left corner; offsets are byte offsets in the text. The
/// default is empty, ready for [`TextLayouter::layout_into`].
///
/// [`TextLayouter::layout_into`]: crate::TextLayouter::layout_into
#[derive(Clone, Debug, Default)]
pub struct TextLayout {
    // The lines and glyphs, shared by the copies aligned at other widths
    // while their lines keep their places.
    body: Arc<LayoutBody>,
    width: f32,
    // The width the lines wrap at.
    wrap: f32,
    did_overflow_width: bool,
    id: u32,
}

impl PartialEq for TextLayout {
    fn eq(&self, other: &Self) -> bool {
        // Copies that share a body hold the same lines and glyphs. A text
        // laid out again into the same lines draws the same, though it
        // takes a new id.
        (Arc::ptr_eq(&self.body, &other.body) || self.body == other.body)
            && self.width == other.width
            && self.did_overflow_width == other.did_overflow_width
    }
}

// What a layout holds at every width it is aligned at.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct LayoutBody {
    // Most text is one line in one run of one typeface, which these hold
    // without allocating.
    pub(crate) faces: SmallVec<[Typeface; 1]>,
    // Shared with the bodies copied to place the lines anew.
    pub(crate) glyphs: LaidGlyphs,
    pub(crate) runs: SmallVec<[LineRun; 1]>,
    pub(crate) lines: SmallVec<[LayoutLine; 1]>,
    // The widest line with its indent.
    pub(crate) widest: f32,
    pub(crate) height: f32,
    pub(crate) did_overflow_height: bool,
    // The widths at which the text breaks into the same lines; `None` when
    // only its own width is known to.
    pub(crate) hold: Option<WidthRange>,
    // What the width the lines wrap at follows: the max intrinsic width
    // rounded up to whole device pixels, and whether the widest the
    // paragraph may be counts, which it does when lines wrap or an ellipsis
    // cuts them.
    pub(crate) content_width: f32,
    pub(crate) follows_max: bool,
    // The widest line rounded up to whole device pixels: the width of a
    // paragraph whose lines wrap.
    pub(crate) line_width: f32,
    // How many glyphs, from the first, shaping placed: an ellipsis's and a
    // hyphen's follow them.
    pub(crate) shaped: usize,
}

// The id the next layout made or aligned takes.
static NEXT_ID: AtomicU32 = AtomicU32::new(1);

// Widths from `min` up to but not including `max`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WidthRange {
    pub(crate) min: f32,
    pub(crate) max: f32,
}

impl WidthRange {
    pub(crate) const ALL: Self = Self {
        min: f32::NEG_INFINITY,
        max: f32::INFINITY,
    };

    // Narrows the range to the widths from `min` up to `max`.
    pub(crate) fn narrow(&mut self, min: f32, max: f32) {
        self.min = self.min.max(min);
        self.max = self.max.min(max);
    }
}

// How far inside a hold's ends a width must be for the layout to stand for
// it, so that the order a sum is taken in cannot decide.
const HOLD_MARGIN: f32 = 1e-3;

// A run of one typeface, size and bidi level inside a line, in visual order.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LineRun {
    pub(crate) text: IndexRange,
    pub(crate) glyphs: IndexRange,
    pub(crate) face: u32,
    pub(crate) size: f32,
    // Extra width of each cluster, half on each side.
    pub(crate) spacing: f32,
    // In a justified line, how many of the run's glyphs are spaces before
    // the line's trailing whitespace and the extra width each of them takes.
    pub(crate) spaces: u16,
    pub(crate) word_spacing: f32,
    // How far the run's baseline sits above the line's.
    pub(crate) baseline_shift: f32,
    pub(crate) level: BidiLevel,
    // The left edge from the line's left edge and the width, before
    // justification, and how far justification moves the run right.
    pub(crate) x: f32,
    pub(crate) width: f32,
    pub(crate) shift: f32,
}

// One cluster of a run, left to right: the offset its text starts at, its
// glyphs within the run and its width with the run's spacing.
struct Cluster {
    offset: usize,
    glyphs: Range<usize>,
    width: f32,
}

impl LineRun {
    // The left edge from the line's left edge.
    fn left(&self) -> f32 {
        self.x + self.shift
    }

    // The right edge from the line's left edge.
    fn right(&self) -> f32 {
        self.left() + self.width + self.word_spacing * f32::from(self.spaces)
    }

    // Pixels per design unit of `face`, the run's typeface.
    fn scale(&self, face: &Typeface) -> f32 {
        self.size / f32::from(face.units_per_em())
    }

    // The run's clusters left to right, of its `glyphs` in its typeface
    // `face`.
    fn clusters<'a>(
        &'a self,
        glyphs: &'a [ShapedGlyph],
        face: &Typeface,
    ) -> impl Iterator<Item = Cluster> + 'a {
        let scale = self.scale(face);
        let space = face.space();
        let mut at = 0;
        std::iter::from_fn(move || {
            let first = glyphs.get(at)?;
            let start = at;
            let mut width = self.spacing;
            while let Some(glyph) = glyphs
                .get(at)
                .filter(|glyph| glyph.cluster == first.cluster)
            {
                width += font_units(glyph.advance) * scale;
                if space == Some(glyph.id()) {
                    width += self.word_spacing;
                }
                at += 1;
            }
            Some(Cluster {
                offset: first.text_offset(),
                glyphs: start..at,
                width,
            })
        })
    }

    // The run's clusters left to right, each with its left edge from the
    // run's left edge and the offset where its text ends: where the next
    // cluster in text order starts.
    fn cluster_spans<'a>(
        &'a self,
        (glyphs, face): (&'a [ShapedGlyph], &Typeface),
    ) -> impl Iterator<Item = (f32, Cluster, usize)> + 'a {
        let mut clusters = self.clusters(glyphs, face).peekable();
        let mut left = 0.0;
        let mut previous = None;
        std::iter::from_fn(move || {
            let cluster = clusters.next()?;
            let end = if self.level.is_rtl() {
                previous.unwrap_or_else(|| self.text.end())
            } else {
                clusters
                    .peek()
                    .map_or_else(|| self.text.end(), |next| next.offset)
            };
            previous = Some(cluster.offset);
            let at = left;
            left += cluster.width;
            Some((at, cluster, end))
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LayoutLine {
    pub(crate) text: IndexRange,
    // The end of the text before the line's trailing whitespace.
    pub(crate) visible_end: u32,
    pub(crate) runs: IndexRange,
    pub(crate) top: f32,
    pub(crate) baseline: f32,
    pub(crate) bottom: f32,
    // The left edge of the line's visible text, its indent, its width
    // before justification and what justification adds to it.
    pub(crate) left: f32,
    pub(crate) indent: f32,
    pub(crate) width: f32,
    pub(crate) stretch: f32,
    // The level of the paragraph the line is in.
    pub(crate) level: BidiLevel,
    pub(crate) ellipsized: bool,
    pub(crate) ends_paragraph: bool,
}

impl LayoutLine {
    // The right edge of the visible text.
    fn right(&self) -> f32 {
        self.left + self.width + self.stretch
    }
}

/// Which of its two places a caret takes where the text before and after it
/// runs in different directions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaretDirection {
    /// The place of the paragraph's direction: Compose's
    /// `usePrimaryDirection = true`.
    Primary,
    /// The other place.
    Secondary,
}

/// The glyphs of one run of a line, ready to draw.
pub struct GlyphRun<'a> {
    /// The typeface the glyphs come from.
    pub typeface: &'a Typeface,
    /// The font size in pixels.
    pub size: f32,
    glyphs: &'a [ShapedGlyph],
    offsets: Option<&'a [GlyphOffset]>,
    run: &'a LineRun,
    pen: f32,
    baseline: f32,
}

/// A glyph placed in a layout.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PositionedGlyph {
    /// The glyph.
    pub id: GlyphId,
    /// Where its origin sits, from the layout's left edge.
    pub x: f32,
    /// Where its origin sits, from the layout's top edge.
    pub y: f32,
    /// The offset in the text of the character it draws, the first of its
    /// cluster.
    pub offset: usize,
}

impl GlyphRun<'_> {
    /// Each glyph with the position of its origin in the layout, left to
    /// right.
    pub fn positions(&self) -> impl Iterator<Item = PositionedGlyph> + '_ {
        let run = self.run;
        let baseline = self.baseline - run.baseline_shift;
        let scale = run.scale(self.typeface);
        let mut pen = self.pen;
        run.clusters(self.glyphs, self.typeface)
            .flat_map(move |cluster| {
                let mut x = pen + run.spacing / 2.0;
                pen += cluster.width;
                cluster.glyphs.map(move |index| {
                    let glyph = self.glyphs[index];
                    let drawn = self
                        .offsets
                        .and_then(|offsets| offsets.get(index))
                        .copied()
                        .unwrap_or_default();
                    let at = x + font_units(drawn.x) * scale;
                    x += font_units(glyph.advance) * scale;
                    PositionedGlyph {
                        id: glyph.id(),
                        x: at,
                        y: baseline - font_units(drawn.y) * scale,
                        offset: cluster.offset,
                    }
                })
            })
    }

    /// The text the run draws.
    pub fn text(&self) -> Range<usize> {
        self.run.text.range()
    }

    /// Where the run's baseline sits, from the layout's top edge.
    pub fn baseline(&self) -> f32 {
        self.baseline - self.run.baseline_shift
    }

    /// The run's left and right edges, from the layout's left edge.
    pub fn edges(&self) -> (f32, f32) {
        (self.pen, self.pen + self.run.right() - self.run.left())
    }
}

impl LayoutBody {
    // The width the lines wrap at between `min_width` and `max_width`, as
    // Compose's `finalMaxWidth` gives it.
    pub(crate) fn wrap_width(&self, min_width: f32, max_width: f32) -> f32 {
        let max_width = if self.follows_max {
            max_width
        } else {
            f32::INFINITY
        };
        // Equal ends fix the width; a min past the max leaves the max.
        if min_width >= max_width {
            max_width
        } else {
            self.content_width.max(min_width).min(max_width)
        }
    }

    // The width of the paragraph between `min_width` and `max_width`: the
    // width its lines wrap at, save that lines that wrap take the widest of
    // them, as a label shows its lines and not the room around them.
    pub(crate) fn width_between(&self, min_width: f32, max_width: f32) -> f32 {
        self.width_at(self.wrap_width(min_width, max_width), min_width)
    }

    // The width of the paragraph whose lines wrap at `wrap`, at least
    // `min_width` wide.
    fn width_at(&self, wrap: f32, min_width: f32) -> f32 {
        if self.content_width <= wrap {
            wrap
        } else {
            self.line_width.max(min_width).min(wrap)
        }
    }

    // The glyphs when shaping placed them all, which a relayout that adds
    // none keeps.
    pub(crate) fn shaped_glyphs(&self) -> Option<&LaidGlyphs> {
        (self.glyphs.len() == self.shaped).then_some(&self.glyphs)
    }

    // The glyphs of `run` and their typeface.
    fn run_glyphs(&self, run: &LineRun) -> (&[ShapedGlyph], &Typeface) {
        (
            &self.glyphs[run.glyphs.range()],
            &self.faces[widen(run.face)],
        )
    }

    // Empties the body, keeping the storage of all but its shared glyphs.
    pub(crate) fn clear(&mut self) {
        self.faces.clear();
        self.glyphs = LaidGlyphs::default();
        self.runs.clear();
        self.lines.clear();
        self.widest = 0.0;
        self.height = 0.0;
        self.did_overflow_height = false;
        self.hold = None;
        self.content_width = 0.0;
        self.follows_max = false;
        self.line_width = 0.0;
        self.shaped = 0;
    }

    // Gives back the room each part has past its length.
    pub(crate) fn shrink_to_fit(&mut self) {
        self.faces.shrink_to_fit();
        self.runs.shrink_to_fit();
        self.lines.shrink_to_fit();
    }

    // Whether placing the lines across `width` as `align` says moves one:
    // every justified text does, and other text when a line's place across
    // the free width changes.
    fn lines_move(&self, width: f32, align: TextAlign) -> bool {
        align == TextAlign::Justify
            || self.lines.iter().any(|line| {
                line.stretch != 0.0
                    || line_left(line, width, align).to_bits() != line.left.to_bits()
            })
    }

    // Places each line across `width` as `align` says, stretching justified
    // lines to the width. True when a line moved or stretched, moving its
    // glyphs.
    pub(crate) fn align_lines(&mut self, width: f32, align: TextAlign) -> bool {
        let mut moved = false;
        for index in 0..self.lines.len() {
            let before = (self.lines[index].left, self.lines[index].stretch);
            self.unjustify(index);
            let line = &self.lines[index];
            let free = width - line.indent - line.width;
            // Justified text stretches the spaces of each line but the last
            // of its paragraph, as Android's inter-word justification does.
            let justified = align == TextAlign::Justify
                && free > 0.0
                && !line.ellipsized
                && !line.ends_paragraph;
            let stretched = justified && self.justify(index, free);
            let line = &mut self.lines[index];
            line.left = if stretched {
                0.0
            } else {
                line_left(line, width, align)
            };
            moved |= line.left.to_bits() != before.0.to_bits()
                || line.stretch.to_bits() != before.1.to_bits();
        }
        moved
    }

    // Takes the stretch of justification off line `index`.
    fn unjustify(&mut self, index: usize) {
        let line = &mut self.lines[index];
        line.stretch = 0.0;
        for run in &mut self.runs[line.runs.range()] {
            run.word_spacing = 0.0;
            run.shift = 0.0;
        }
    }

    // Spreads `free` width over the spaces of the visible text of line
    // `index`; false when the line has none.
    fn justify(&mut self, index: usize, free: f32) -> bool {
        let Self {
            faces,
            lines,
            runs,
            glyphs,
            ..
        } = self;
        let line = &mut lines[index];
        let runs = &mut runs[line.runs.range()];
        let visible_end = widen(line.visible_end);
        // How many of a run's glyphs are spaces before the line's trailing
        // whitespace.
        let spaces = |run: &LineRun| {
            if run.text.start() >= visible_end {
                return 0;
            }
            let count = glyphs[run.glyphs.range()]
                .iter()
                .filter(|glyph| Some(glyph.id()) == faces[widen(run.face)].space())
                .count();
            u16::try_from(count).unwrap_or(u16::MAX)
        };
        let total: u32 = runs.iter().map(|run| u32::from(spaces(run))).sum();
        if total == 0 {
            return false;
        }
        let per_space = free / f32::from(u16::try_from(total).unwrap_or(u16::MAX));
        let mut shift = 0.0;
        for run in runs {
            run.shift = shift;
            run.spaces = spaces(run);
            if run.spaces > 0 {
                run.word_spacing = per_space;
                shift += per_space * f32::from(run.spaces);
            }
        }
        line.stretch = free;
        true
    }
}

impl TextLayout {
    // The layout of `body`, whose lines wrap at `wrap`, laid out between
    // `widths`, the narrowest and the widest it may be.
    pub(crate) fn new(body: LayoutBody, wrap: f32, widths: (f32, f32)) -> Self {
        let width = body.width_between(widths.0, widths.1);
        let mut layout = Self {
            did_overflow_width: body.widest > width || width > widths.1,
            body: Arc::new(body),
            width,
            wrap,
            id: 0,
        };
        layout.renew_id();
        layout
    }

    // Lays the layout out again between `widths`: `fill` fills its body, in
    // the body's own storage when no copy shares it, and gives the width its
    // lines wrap at.
    pub(crate) fn refill(&mut self, widths: (f32, f32), fill: impl FnOnce(&mut LayoutBody) -> f32) {
        if Arc::get_mut(&mut self.body).is_none() {
            self.body = Arc::default();
        }
        let body = Arc::make_mut(&mut self.body);
        self.wrap = fill(body);
        self.width = body.width_between(widths.0, widths.1);
        self.did_overflow_width = body.widest > self.width || self.width > widths.1;
        self.renew_id();
    }

    // The body, which the layouter reads glyphs shaped before from.
    pub(crate) fn body(&self) -> &LayoutBody {
        &self.body
    }

    /// A number this layout's glyphs and positions go by: a layout laid out
    /// or aligned again takes a new one, so a renderer keys what it drew of
    /// a layout by it; a clone keeps it.
    pub fn id(&self) -> u32 {
        self.id
    }

    // Gives the layout a new id, its glyphs or positions changed.
    pub(crate) fn renew_id(&mut self) {
        self.id = NEXT_ID.fetch_add(1, Ordering::Relaxed).max(1);
    }

    /// Whether the text laid out again between `min_width` and `max_width`,
    /// with the rest of its input the same, breaks into the same lines:
    /// [`TextLayout::align`] then gives that layout without laying the text
    /// out again.
    pub fn holds(&self, min_width: f32, max_width: f32) -> bool {
        self.holds_wrap(self.body.wrap_width(min_width, max_width))
    }

    /// Whether laying the text out between `min_width` and `max_width`
    /// gives this layout: its lines [hold](TextLayout::holds) there and its
    /// width stays.
    pub fn stays_between(&self, min_width: f32, max_width: f32) -> bool {
        let wrap = self.body.wrap_width(min_width, max_width);
        self.body.width_at(wrap, min_width).to_bits() == self.width.to_bits()
            && self.holds_wrap(wrap)
    }

    // Whether lines that wrap at `wrap` are this layout's.
    fn holds_wrap(&self, wrap: f32) -> bool {
        (wrap - self.wrap).abs() < HOLD_MARGIN
            || self
                .body
                .hold
                .is_some_and(|hold| hold.min + HOLD_MARGIN <= wrap && wrap + HOLD_MARGIN < hold.max)
    }

    /// The width the paragraph takes laid out between `min_width` and
    /// `max_width`: its max intrinsic width rounded up to whole device
    /// pixels within them, the max when they are equal, and its widest line
    /// rounded up to whole device pixels within them when its lines wrap.
    pub fn width_between(&self, min_width: f32, max_width: f32) -> f32 {
        self.body.width_between(min_width, max_width)
    }

    /// The max widths, from `.0` up to and with `.1`, at which the paragraph
    /// laid out from `min_width` keeps its width and its lines: every width
    /// when its width follows no max, every width from its own when its text
    /// fits, else the widths its lines wrap the same at below the width it
    /// fits at, or only the one it wraps at when those are not known.
    pub fn max_widths_kept(&self, min_width: f32) -> (f32, f32) {
        let body = &self.body;
        if !body.follows_max {
            (f32::NEG_INFINITY, f32::INFINITY)
        } else if self.width >= body.content_width.max(min_width) {
            (self.width, f32::INFINITY)
        } else if let Some(hold) = body.hold {
            let low = (hold.min + HOLD_MARGIN).max(self.width);
            let high = (hold.max - HOLD_MARGIN).min(body.content_width).next_down();
            if low <= high {
                (low, high)
            } else {
                (self.wrap, self.wrap)
            }
        } else {
            (self.wrap, self.wrap)
        }
    }

    /// Places the lines across the width the paragraph takes between
    /// `min_width` and `max_width` as `align` says, for widths the layout
    /// [holds](TextLayout::holds).
    pub fn align(&mut self, min_width: f32, max_width: f32, align: TextAlign) {
        self.wrap = self.body.wrap_width(min_width, max_width);
        self.width = self.body.width_between(min_width, max_width);
        self.did_overflow_width = self.body.widest > self.width || self.width > max_width;
        // Lines that keep their places keep the body the copies share, and
        // the id what a renderer drew of the layout goes by.
        if self.body.lines_move(self.width, align)
            && Arc::make_mut(&mut self.body).align_lines(self.width, align)
        {
            self.renew_id();
        }
    }

    /// The paragraph's width: its max intrinsic width rounded up to whole
    /// device pixels, within the widths it was laid out between.
    pub fn width(&self) -> f32 {
        self.width
    }

    /// The height of all lines.
    pub fn height(&self) -> f32 {
        self.body.height
    }

    /// The number of lines, at least one.
    pub fn line_count(&self) -> usize {
        self.body.lines.len()
    }

    /// Whether the text had more lines than the line limit: Compose's
    /// `didOverflowHeight` of a layout the line limit cut.
    pub fn did_overflow_height(&self) -> bool {
        self.body.did_overflow_height
    }

    /// Whether the paragraph is wider than the widest it was laid out at, or
    /// a line wider than the paragraph: Compose's `didOverflowWidth`.
    pub fn did_overflow_width(&self) -> bool {
        self.did_overflow_width
    }

    /// Whether `line` shows an ellipsis in place of text.
    pub fn is_line_ellipsized(&self, line: usize) -> bool {
        self.body.lines[line].ellipsized
    }

    /// The offset where `line` starts. Panics when `line` is out of range, as
    /// Compose throws.
    pub fn line_start(&self, line: usize) -> usize {
        self.body.lines[line].text.start()
    }

    /// The offset where `line` ends, after its line break and trailing
    /// whitespace.
    pub fn line_end(&self, line: usize) -> usize {
        self.body.lines[line].text.end()
    }

    /// The offset where the visible text of `line` ends, before its line
    /// break and trailing whitespace: Compose's `getLineEnd(line, true)`.
    pub fn line_visible_end(&self, line: usize) -> usize {
        widen(self.body.lines[line].visible_end)
    }

    /// The top of `line`.
    pub fn line_top(&self, line: usize) -> f32 {
        self.body.lines[line].top
    }

    /// The bottom of `line`.
    pub fn line_bottom(&self, line: usize) -> f32 {
        self.body.lines[line].bottom
    }

    /// The baseline of `line`.
    pub fn line_baseline(&self, line: usize) -> f32 {
        self.body.lines[line].baseline
    }

    /// The left edge of the visible text of `line`.
    pub fn line_left(&self, line: usize) -> f32 {
        self.body.lines[line].left
    }

    /// The right edge of the visible text of `line`.
    pub fn line_right(&self, line: usize) -> f32 {
        self.body.lines[line].right()
    }

    /// The line that holds `offset`; the last line for the end of the text.
    pub fn line_for_offset(&self, offset: usize) -> usize {
        self.body
            .lines
            .partition_point(|line| line.text.end() <= offset)
            .min(self.body.lines.len() - 1)
    }

    /// The line at height `y`, the first above the text and the last below it.
    pub fn line_for_vertical_position(&self, y: f32) -> usize {
        self.body
            .lines
            .partition_point(|line| line.bottom <= y)
            .min(self.body.lines.len() - 1)
    }

    /// The x of a caret at `offset`. Where the text before and after `offset`
    /// runs in different directions the caret has two places, and `direction`
    /// picks one.
    pub fn horizontal_position(&self, offset: usize, direction: CaretDirection) -> f32 {
        let line = &self.body.lines[self.line_for_offset(offset)];
        let runs = &self.body.runs[line.runs.range()];
        let after = runs
            .iter()
            .find(|run| run.text.start() <= offset && offset < run.text.end());
        let before = runs
            .iter()
            .find(|run| run.text.start() < offset && offset <= run.text.end());
        let run = match (before, after) {
            (Some(before), Some(after)) if before.level != after.level => {
                if (before.level == line.level) == (direction == CaretDirection::Primary) {
                    before
                } else {
                    after
                }
            }
            (_, Some(run)) | (Some(run), None) => run,
            (None, None) => {
                return if line.level.is_rtl() {
                    line.right()
                } else {
                    line.left
                };
            }
        };
        line.left + self.caret_in_run(run, offset)
    }

    /// The offset of the caret nearest to the point `x`, `y`.
    pub fn offset_for_position(&self, x: f32, y: f32) -> usize {
        let line = &self.body.lines[self.line_for_vertical_position(y)];
        let runs = &self.body.runs[line.runs.range()];
        let local = x - line.left;
        let Some(run) = runs
            .iter()
            .find(|run| local < run.right())
            .or_else(|| runs.last())
        else {
            return line.text.start();
        };
        self.offset_in_run(run, local - run.left())
    }

    /// The glyph runs of `line`, left to right.
    pub fn glyph_runs(&self, line: usize) -> impl Iterator<Item = GlyphRun<'_>> + '_ {
        let line = &self.body.lines[line];
        self.body.runs[line.runs.range()]
            .iter()
            .map(move |run| GlyphRun {
                typeface: &self.body.faces[widen(run.face)],
                size: run.size,
                glyphs: &self.body.glyphs[run.glyphs.range()],
                offsets: self.body.glyphs.offsets(run.glyphs.range()),
                run,
                pen: line.left + run.left(),
                baseline: line.baseline,
            })
    }

    /// The box of the character at `offset`, across its line: Compose's
    /// `getBoundingBox`. A zero-width box at the caret for an offset with no
    /// character, such as the end of the text.
    pub fn bounding_box(&self, offset: usize) -> Rect {
        let line = &self.body.lines[self.line_for_offset(offset)];
        let found = self.body.runs[line.runs.range()].iter().find_map(|run| {
            run.cluster_spans(self.body.run_glyphs(run))
                .find(|(_, cluster, end)| (cluster.offset..*end).contains(&offset))
                .map(|(left, cluster, _)| (run.left() + left, cluster.width))
        });
        let (x, width) = found.unwrap_or_else(|| {
            (
                self.horizontal_position(offset, CaretDirection::Primary) - line.left,
                0.0,
            )
        });
        Rect {
            x: line.left + x,
            y: line.top,
            width,
            height: line.bottom - line.top,
        }
    }

    /// The caret at `offset` as a zero-width rectangle across its line:
    /// Compose's `getCursorRect`.
    pub fn cursor_rect(&self, offset: usize) -> Rect {
        let line = &self.body.lines[self.line_for_offset(offset)];
        Rect {
            x: self.horizontal_position(offset, CaretDirection::Primary),
            y: line.top,
            width: 0.0,
            height: line.bottom - line.top,
        }
    }

    /// Pushes onto `rects` the rectangles that cover the text from `start`
    /// to `end` line by line, one for each stretch of it that runs in one
    /// direction, within the paragraph's width or its visible text: Compose's
    /// `getPathForRange` as rectangles. The spaces a wrapped line ends with
    /// hang past the widest line, which they do not widen, and cover nothing
    /// there.
    pub fn selection_rects(&self, start: usize, end: usize, rects: &mut Vec<Rect>) {
        let width = self.width;
        for line in &self.body.lines {
            if line.text.end() <= start || line.text.start() >= end {
                continue;
            }
            let mut open: Option<(f32, f32)> = None;
            for run in &self.body.runs[line.runs.range()] {
                for (left, cluster, cluster_end) in run.cluster_spans(self.body.run_glyphs(run)) {
                    let x = run.left() + left;
                    let covered = cluster.offset < end && cluster_end > start;
                    match (covered, open) {
                        (true, Some((from, _))) => open = Some((from, x + cluster.width)),
                        (true, None) => open = Some((x, x + cluster.width)),
                        (false, Some(span)) => {
                            push_span(rects, (line, width), span);
                            open = None;
                        }
                        (false, None) => {}
                    }
                }
            }
            if let Some(span) = open {
                push_span(rects, (line, width), span);
            }
        }
    }

    // The caret x of `offset` inside `run`, from the line's left edge.
    fn caret_in_run(&self, run: &LineRun, offset: usize) -> f32 {
        let (glyphs, face) = self.body.run_glyphs(run);
        let before: f32 = run
            .clusters(glyphs, face)
            .filter(|cluster| cluster.offset < offset)
            .map(|cluster| cluster.width)
            .sum();
        if run.level.is_rtl() {
            run.right() - before
        } else {
            run.left() + before
        }
    }

    // The offset nearest to `x` pixels from the left edge of `run`: each
    // cluster's left half gives the offset on its left.
    fn offset_in_run(&self, run: &LineRun, x: f32) -> usize {
        for (left, cluster, end) in run.cluster_spans(self.body.run_glyphs(run)) {
            if x < left + cluster.width / 2.0 {
                return if run.level.is_rtl() {
                    end
                } else {
                    cluster.offset
                };
            }
        }
        if run.level.is_rtl() {
            run.text.start()
        } else {
            run.text.end()
        }
    }
}

// Where `align` places the left edge of `line` across `width`, unjustified.
fn line_left(line: &LayoutLine, width: f32, align: TextAlign) -> f32 {
    let free = width - line.indent - line.width;
    let left = align_fraction(align, line.level) * free;
    if line.level.is_rtl() {
        left
    } else {
        left + line.indent
    }
}

// How far across the free width of a line `align` places it, in a
// paragraph at `level`.
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

// Adds the rectangle of `line` from `left` to `right`, from the line's left
// edge.
// Pushes the rectangle of `line` from `left` to `right` within the
// paragraph's `width`, or past it as far as the line's visible text reaches.
fn push_span(rects: &mut Vec<Rect>, (line, width): (&LayoutLine, f32), (left, right): (f32, f32)) {
    let (low, high) = (line.left.min(0.0), width.max(line.left + line.width));
    let (left, right) = (
        (line.left + left).clamp(low, high),
        (line.left + right).clamp(low, high),
    );
    if left < right {
        rects.push(Rect {
            x: left,
            y: line.top,
            width: right - left,
            height: line.bottom - line.top,
        });
    }
}

// A distance in font design units as a float, exact for every value under
// 2^24: shaped values lie far inside it for any font, whose em is at most
// 16,384 units.
pub(crate) fn font_units(value: i32) -> f32 {
    value as f32
}
