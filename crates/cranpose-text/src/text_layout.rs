use std::{
    ops::Range,
    sync::atomic::{AtomicU32, Ordering},
};

use cranpose_ui_graphics::Rect;

use crate::{
    bidi_level::BidiLevel, glyph_id::GlyphId, paragraph::TextAlign, shaper::ShapedGlyph,
    typeface::Typeface,
};

/// A paragraph laid out in lines: glyph runs placed for drawing, and the
/// queries of Compose's `TextLayoutResult`. Distances are in pixels from the
/// layout's top left corner; offsets are byte offsets in the text. The
/// default is empty, ready for [`TextLayouter::layout_into`].
///
/// [`TextLayouter::layout_into`]: crate::TextLayouter::layout_into
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextLayout {
    pub(crate) faces: Vec<Typeface>,
    pub(crate) glyphs: Vec<ShapedGlyph>,
    pub(crate) runs: Vec<LineRun>,
    pub(crate) lines: Vec<LayoutLine>,
    pub(crate) width: f32,
    // The widest line with its indent.
    pub(crate) widest: f32,
    pub(crate) height: f32,
    pub(crate) did_overflow_height: bool,
    pub(crate) did_overflow_width: bool,
    // The widths at which the text breaks into the same lines; `None` when
    // only its own width is known to.
    pub(crate) hold: Option<WidthRange>,
    pub(crate) sizing: Sizing,
    id: u32,
}

// The id the next layout made or aligned takes.
static NEXT_ID: AtomicU32 = AtomicU32::new(1);

// What the paragraph's width follows: its max intrinsic width rounded up to
// whole device pixels, and whether the widest it may be counts, which it
// does when lines wrap or an ellipsis cuts them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Sizing {
    pub(crate) content_width: f32,
    pub(crate) follows_max: bool,
}

impl Sizing {
    // The width of the paragraph between `min_width` and `max_width`, as
    // Compose's `finalMaxWidth` gives it.
    pub(crate) fn width(self, min_width: f32, max_width: f32) -> f32 {
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
}

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
    pub(crate) text: Range<usize>,
    pub(crate) glyphs: Range<usize>,
    pub(crate) face: usize,
    pub(crate) size: f32,
    // Pixels per design unit.
    pub(crate) scale: f32,
    // Extra width of each cluster, half on each side.
    pub(crate) spacing: f32,
    // The face's space, how many of the run's glyphs are spaces before the
    // line's trailing whitespace, and the extra width each of them takes in
    // a justified line.
    pub(crate) space: Option<GlyphId>,
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

    // The run's clusters left to right.
    fn clusters<'a>(&'a self, glyphs: &'a [ShapedGlyph]) -> impl Iterator<Item = Cluster> + 'a {
        let mut at = 0;
        std::iter::from_fn(move || {
            let first = glyphs.get(at)?;
            let start = at;
            let mut width = self.spacing;
            while let Some(glyph) = glyphs
                .get(at)
                .filter(|glyph| glyph.cluster == first.cluster)
            {
                width += font_units(glyph.advance) * self.scale;
                if self.space == Some(glyph.id) {
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
        glyphs: &'a [ShapedGlyph],
    ) -> impl Iterator<Item = (f32, Cluster, usize)> + 'a {
        let mut clusters = self.clusters(glyphs).peekable();
        let mut left = 0.0;
        let mut previous = None;
        std::iter::from_fn(move || {
            let cluster = clusters.next()?;
            let end = if self.level.is_rtl() {
                previous.unwrap_or(self.text.end)
            } else {
                clusters.peek().map_or(self.text.end, |next| next.offset)
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
    pub(crate) text: Range<usize>,
    // The end of the text before the line's trailing whitespace.
    pub(crate) visible_end: usize,
    pub(crate) runs: Range<usize>,
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
        let mut pen = self.pen;
        run.clusters(self.glyphs).flat_map(move |cluster| {
            let mut x = pen + run.spacing / 2.0;
            pen += cluster.width;
            self.glyphs[cluster.glyphs].iter().map(move |glyph| {
                let at = x + font_units(glyph.x_offset) * run.scale;
                x += font_units(glyph.advance) * run.scale;
                PositionedGlyph {
                    id: glyph.id,
                    x: at,
                    y: baseline - font_units(glyph.y_offset) * run.scale,
                    offset: cluster.offset,
                }
            })
        })
    }

    /// The text the run draws.
    pub fn text(&self) -> Range<usize> {
        self.run.text.clone()
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

impl TextLayout {
    // Empties the layout, keeping its storage.
    pub(crate) fn clear(&mut self) {
        self.faces.clear();
        self.glyphs.clear();
        self.runs.clear();
        self.lines.clear();
        self.width = 0.0;
        self.widest = 0.0;
        self.height = 0.0;
        self.did_overflow_height = false;
        self.did_overflow_width = false;
        self.hold = None;
        self.sizing = Sizing::default();
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
        let width = self.sizing.width(min_width, max_width);
        (width - self.width).abs() < HOLD_MARGIN
            || self.hold.is_some_and(|hold| {
                hold.min + HOLD_MARGIN <= width && width + HOLD_MARGIN < hold.max
            })
    }

    /// The width the paragraph takes laid out between `min_width` and
    /// `max_width`: its max intrinsic width rounded up to whole device
    /// pixels within them, the max when they are equal.
    pub fn width_between(&self, min_width: f32, max_width: f32) -> f32 {
        self.sizing.width(min_width, max_width)
    }

    /// The max widths, from `.0` up to and with `.1`, at which the paragraph
    /// laid out from `min_width` keeps its width and its lines: every width
    /// when its width follows no max, every width from its own when its text
    /// fits, else only its own width.
    pub fn max_widths_kept(&self, min_width: f32) -> (f32, f32) {
        if !self.sizing.follows_max {
            (f32::NEG_INFINITY, f32::INFINITY)
        } else if self.width >= self.sizing.content_width.max(min_width) {
            (self.width, f32::INFINITY)
        } else {
            (self.width, self.width)
        }
    }

    /// Places the lines across the width the paragraph takes between
    /// `min_width` and `max_width` as `align` says, for widths the layout
    /// [holds](TextLayout::holds).
    pub fn align(&mut self, min_width: f32, max_width: f32, align: TextAlign) {
        self.width = self.sizing.width(min_width, max_width);
        self.did_overflow_width = self.widest > self.width || self.width > max_width;
        // A glyph that keeps its place keeps the id what a renderer drew of
        // the layout goes by.
        if self.align_lines(align) {
            self.renew_id();
        }
    }

    // Places each line across the layout's width as `align` says,
    // stretching justified lines to the width. True when a line moved or
    // stretched, moving its glyphs.
    pub(crate) fn align_lines(&mut self, align: TextAlign) -> bool {
        let mut moved = false;
        for index in 0..self.lines.len() {
            let before = (self.lines[index].left, self.lines[index].stretch);
            self.unjustify(index);
            let line = &self.lines[index];
            let free = self.width - line.indent - line.width;
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
                align_fraction(align, line.level) * free
            };
            if !line.level.is_rtl() {
                line.left += line.indent;
            }
            moved |= line.left.to_bits() != before.0.to_bits()
                || line.stretch.to_bits() != before.1.to_bits();
        }
        moved
    }

    // Takes the stretch of justification off line `index`.
    fn unjustify(&mut self, index: usize) {
        let line = &mut self.lines[index];
        line.stretch = 0.0;
        for run in &mut self.runs[line.runs.clone()] {
            run.word_spacing = 0.0;
            run.shift = 0.0;
        }
    }

    // Spreads `free` width over the spaces of the visible text of line
    // `index`; false when the line has none.
    fn justify(&mut self, index: usize, free: f32) -> bool {
        let line = &mut self.lines[index];
        let runs = &mut self.runs[line.runs.clone()];
        let total: u32 = runs.iter().map(|run| u32::from(run.spaces)).sum();
        if total == 0 {
            return false;
        }
        let per_space = free / f32::from(u16::try_from(total).unwrap_or(u16::MAX));
        let mut shift = 0.0;
        for run in runs {
            run.shift = shift;
            if run.spaces > 0 {
                run.word_spacing = per_space;
                shift += per_space * f32::from(run.spaces);
            }
        }
        line.stretch = free;
        true
    }

    /// The paragraph's width: its max intrinsic width rounded up to whole
    /// device pixels, within the widths it was laid out between.
    pub fn width(&self) -> f32 {
        self.width
    }

    /// The height of all lines.
    pub fn height(&self) -> f32 {
        self.height
    }

    /// The number of lines, at least one.
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Whether the text had more lines than the line limit: Compose's
    /// `didOverflowHeight` of a layout the line limit cut.
    pub fn did_overflow_height(&self) -> bool {
        self.did_overflow_height
    }

    /// Whether the paragraph is wider than the widest it was laid out at, or
    /// a line wider than the paragraph: Compose's `didOverflowWidth`.
    pub fn did_overflow_width(&self) -> bool {
        self.did_overflow_width
    }

    /// Whether `line` shows an ellipsis in place of text.
    pub fn is_line_ellipsized(&self, line: usize) -> bool {
        self.lines[line].ellipsized
    }

    /// The offset where `line` starts. Panics when `line` is out of range, as
    /// Compose throws.
    pub fn line_start(&self, line: usize) -> usize {
        self.lines[line].text.start
    }

    /// The offset where `line` ends, after its line break and trailing
    /// whitespace.
    pub fn line_end(&self, line: usize) -> usize {
        self.lines[line].text.end
    }

    /// The offset where the visible text of `line` ends, before its line
    /// break and trailing whitespace: Compose's `getLineEnd(line, true)`.
    pub fn line_visible_end(&self, line: usize) -> usize {
        self.lines[line].visible_end
    }

    /// The top of `line`.
    pub fn line_top(&self, line: usize) -> f32 {
        self.lines[line].top
    }

    /// The bottom of `line`.
    pub fn line_bottom(&self, line: usize) -> f32 {
        self.lines[line].bottom
    }

    /// The baseline of `line`.
    pub fn line_baseline(&self, line: usize) -> f32 {
        self.lines[line].baseline
    }

    /// The left edge of the visible text of `line`.
    pub fn line_left(&self, line: usize) -> f32 {
        self.lines[line].left
    }

    /// The right edge of the visible text of `line`.
    pub fn line_right(&self, line: usize) -> f32 {
        self.lines[line].right()
    }

    /// The line that holds `offset`; the last line for the end of the text.
    pub fn line_for_offset(&self, offset: usize) -> usize {
        self.lines
            .partition_point(|line| line.text.end <= offset)
            .min(self.lines.len() - 1)
    }

    /// The line at height `y`, the first above the text and the last below it.
    pub fn line_for_vertical_position(&self, y: f32) -> usize {
        self.lines
            .partition_point(|line| line.bottom <= y)
            .min(self.lines.len() - 1)
    }

    /// The x of a caret at `offset`. Where the text before and after `offset`
    /// runs in different directions the caret has two places, and `direction`
    /// picks one.
    pub fn horizontal_position(&self, offset: usize, direction: CaretDirection) -> f32 {
        let line = &self.lines[self.line_for_offset(offset)];
        let runs = &self.runs[line.runs.clone()];
        let after = runs
            .iter()
            .find(|run| run.text.start <= offset && offset < run.text.end);
        let before = runs
            .iter()
            .find(|run| run.text.start < offset && offset <= run.text.end);
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
        let line = &self.lines[self.line_for_vertical_position(y)];
        let runs = &self.runs[line.runs.clone()];
        let local = x - line.left;
        let Some(run) = runs
            .iter()
            .find(|run| local < run.right())
            .or_else(|| runs.last())
        else {
            return line.text.start;
        };
        self.offset_in_run(run, local - run.left())
    }

    /// The glyph runs of `line`, left to right.
    pub fn glyph_runs(&self, line: usize) -> impl Iterator<Item = GlyphRun<'_>> + '_ {
        let line = &self.lines[line];
        self.runs[line.runs.clone()]
            .iter()
            .map(move |run| GlyphRun {
                typeface: &self.faces[run.face],
                size: run.size,
                glyphs: &self.glyphs[run.glyphs.clone()],
                run,
                pen: line.left + run.left(),
                baseline: line.baseline,
            })
    }

    /// The box of the character at `offset`, across its line: Compose's
    /// `getBoundingBox`. A zero-width box at the caret for an offset with no
    /// character, such as the end of the text.
    pub fn bounding_box(&self, offset: usize) -> Rect {
        let line = &self.lines[self.line_for_offset(offset)];
        let found = self.runs[line.runs.clone()].iter().find_map(|run| {
            run.cluster_spans(&self.glyphs[run.glyphs.clone()])
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
        let line = &self.lines[self.line_for_offset(offset)];
        Rect {
            x: self.horizontal_position(offset, CaretDirection::Primary),
            y: line.top,
            width: 0.0,
            height: line.bottom - line.top,
        }
    }

    /// Pushes onto `rects` the rectangles that cover the text from `start`
    /// to `end` line by line, one for each stretch of it that runs in one
    /// direction: Compose's `getPathForRange` as rectangles.
    pub fn selection_rects(&self, start: usize, end: usize, rects: &mut Vec<Rect>) {
        for line in &self.lines {
            if line.text.end <= start || line.text.start >= end {
                continue;
            }
            let mut open: Option<(f32, f32)> = None;
            for run in &self.runs[line.runs.clone()] {
                for (left, cluster, cluster_end) in
                    run.cluster_spans(&self.glyphs[run.glyphs.clone()])
                {
                    let x = run.left() + left;
                    let covered = cluster.offset < end && cluster_end > start;
                    match (covered, open) {
                        (true, Some((from, _))) => open = Some((from, x + cluster.width)),
                        (true, None) => open = Some((x, x + cluster.width)),
                        (false, Some(span)) => {
                            push_span(rects, line, span);
                            open = None;
                        }
                        (false, None) => {}
                    }
                }
            }
            if let Some(span) = open {
                push_span(rects, line, span);
            }
        }
    }

    // The caret x of `offset` inside `run`, from the line's left edge.
    fn caret_in_run(&self, run: &LineRun, offset: usize) -> f32 {
        let before: f32 = run
            .clusters(&self.glyphs[run.glyphs.clone()])
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
        for (left, cluster, end) in run.cluster_spans(&self.glyphs[run.glyphs.clone()]) {
            if x < left + cluster.width / 2.0 {
                return if run.level.is_rtl() {
                    end
                } else {
                    cluster.offset
                };
            }
        }
        if run.level.is_rtl() {
            run.text.start
        } else {
            run.text.end
        }
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
fn push_span(rects: &mut Vec<Rect>, line: &LayoutLine, (left, right): (f32, f32)) {
    rects.push(Rect {
        x: line.left + left,
        y: line.top,
        width: right - left,
        height: line.bottom - line.top,
    });
}

// A distance in font design units as a float. Shaped values lie far inside
// the range of an `i16` for any font, whose em is at most 16,384 units.
pub(crate) fn font_units(value: i32) -> f32 {
    f32::from(i16::try_from(value).unwrap_or(if value < 0 { i16::MIN } else { i16::MAX }))
}
