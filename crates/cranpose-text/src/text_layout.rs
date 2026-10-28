use std::ops::Range;

use cranpose_ui_graphics::Rect;

use crate::{bidi_level::BidiLevel, glyph_id::GlyphId, shaper::ShapedGlyph, typeface::Typeface};

/// A paragraph laid out in lines: glyph runs placed for drawing, and the
/// queries of Compose's `TextLayoutResult`. Distances are in pixels from the
/// layout's top left corner; offsets are byte offsets in the text. The
/// default is empty, ready for [`TextLayouter::layout_into`].
///
/// [`TextLayouter::layout_into`]: crate::TextLayouter::layout_into
#[derive(Default)]
pub struct TextLayout {
    pub(crate) faces: Vec<Typeface>,
    pub(crate) glyphs: Vec<ShapedGlyph>,
    pub(crate) runs: Vec<LineRun>,
    pub(crate) lines: Vec<LayoutLine>,
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) did_overflow_height: bool,
    pub(crate) did_overflow_width: bool,
}

// A run of one typeface, size and bidi level inside a line, in visual order.
#[derive(Clone, Debug)]
pub(crate) struct LineRun {
    pub(crate) text: Range<usize>,
    pub(crate) glyphs: Range<usize>,
    pub(crate) face: usize,
    pub(crate) size: f32,
    // Pixels per design unit.
    pub(crate) scale: f32,
    // Extra width of each cluster, half on each side.
    pub(crate) spacing: f32,
    // Extra width of each space of a justified line, and the face's space.
    pub(crate) word_spacing: f32,
    pub(crate) space: Option<GlyphId>,
    // How far the run's baseline sits above the line's.
    pub(crate) baseline_shift: f32,
    pub(crate) level: BidiLevel,
    // The left edge from the line's left edge.
    pub(crate) x: f32,
    pub(crate) width: f32,
}

// One cluster of a run, left to right: the offset its text starts at, its
// glyphs within the run and its width with the run's spacing.
struct Cluster {
    offset: usize,
    glyphs: Range<usize>,
    width: f32,
}

impl LineRun {
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

#[derive(Clone, Debug)]
pub(crate) struct LayoutLine {
    pub(crate) text: Range<usize>,
    // The end of the text before the line's trailing whitespace.
    pub(crate) visible_end: usize,
    pub(crate) runs: Range<usize>,
    pub(crate) top: f32,
    pub(crate) baseline: f32,
    pub(crate) bottom: f32,
    // The left edge of the line's visible text.
    pub(crate) left: f32,
    pub(crate) width: f32,
    // The level of the paragraph the line is in.
    pub(crate) level: BidiLevel,
    pub(crate) ellipsized: bool,
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

impl GlyphRun<'_> {
    /// Each glyph with the position of its origin in the layout, left to
    /// right.
    pub fn positions(&self) -> impl Iterator<Item = (GlyphId, f32, f32)> + '_ {
        let run = self.run;
        let baseline = self.baseline - run.baseline_shift;
        let mut pen = self.pen;
        run.clusters(self.glyphs).flat_map(move |cluster| {
            let mut x = pen + run.spacing / 2.0;
            pen += cluster.width;
            self.glyphs[cluster.glyphs].iter().map(move |glyph| {
                let at = x + font_units(glyph.x_offset) * run.scale;
                x += font_units(glyph.advance) * run.scale;
                (
                    glyph.id,
                    at,
                    baseline - font_units(glyph.y_offset) * run.scale,
                )
            })
        })
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
        self.height = 0.0;
        self.did_overflow_height = false;
        self.did_overflow_width = false;
    }

    /// The width the lines were laid out in: the wrap width, or the widest
    /// line when lines do not wrap.
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

    /// Whether a line is wider than the layout: Compose's
    /// `didOverflowWidth`.
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
        let line = &self.lines[line];
        line.left + line.width
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
                    line.left + line.width
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
            .find(|run| local < run.x + run.width)
            .or_else(|| runs.last())
        else {
            return line.text.start;
        };
        self.offset_in_run(run, local - run.x)
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
                pen: line.left + run.x,
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
                .map(|(left, cluster, _)| (run.x + left, cluster.width))
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
                    let x = run.x + left;
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
            run.x + run.width - before
        } else {
            run.x + before
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
