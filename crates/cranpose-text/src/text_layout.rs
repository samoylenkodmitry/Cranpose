use std::ops::Range;

use crate::{bidi_level::BidiLevel, glyph_id::GlyphId, shaper::ShapedGlyph, typeface::Typeface};

/// A paragraph laid out in lines: glyph runs placed for drawing, and the
/// queries of Compose's `TextLayoutResult`. Distances are in pixels from the
/// layout's top left corner; offsets are byte offsets in the text.
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
    pub(crate) level: BidiLevel,
    // The left edge from the line's left edge.
    pub(crate) x: f32,
    pub(crate) width: f32,
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
    scale: f32,
    spacing: f32,
    pen: f32,
    baseline: f32,
}

impl GlyphRun<'_> {
    /// Each glyph with the position of its origin in the layout, left to
    /// right.
    pub fn positions(&self) -> impl Iterator<Item = (GlyphId, f32, f32)> + '_ {
        let mut pen = self.pen;
        let mut cluster = None;
        self.glyphs.iter().map(move |glyph| {
            if cluster != Some(glyph.cluster) {
                pen += if cluster.is_some() {
                    self.spacing
                } else {
                    self.spacing / 2.0
                };
                cluster = Some(glyph.cluster);
            }
            let x = pen + font_units(glyph.x_offset) * self.scale;
            pen += font_units(glyph.advance) * self.scale;
            (
                glyph.id,
                x,
                self.baseline - font_units(glyph.y_offset) * self.scale,
            )
        })
    }
}

impl TextLayout {
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
                scale: run.scale,
                spacing: run.spacing,
                pen: line.left + run.x,
                baseline: line.baseline,
            })
    }

    // The caret x of `offset` inside `run`, from the line's left edge.
    fn caret_in_run(&self, run: &LineRun, offset: usize) -> f32 {
        let mut before = 0.0;
        let mut cluster = None;
        for glyph in &self.glyphs[run.glyphs.clone()] {
            if glyph.text_offset() >= offset {
                continue;
            }
            if cluster != Some(glyph.cluster) {
                before += run.spacing;
                cluster = Some(glyph.cluster);
            }
            before += font_units(glyph.advance) * run.scale;
        }
        if run.level.is_rtl() {
            run.x + run.width - before
        } else {
            run.x + before
        }
    }

    // The offset nearest to `x` pixels from the left edge of `run`: each
    // cluster's left half gives the offset on its left.
    fn offset_in_run(&self, run: &LineRun, x: f32) -> usize {
        let glyphs = &self.glyphs[run.glyphs.clone()];
        let rtl = run.level.is_rtl();
        let mut left = 0.0;
        let mut at = 0;
        while at < glyphs.len() {
            let cluster = glyphs[at].cluster;
            let mut width = run.spacing;
            let mut end = at;
            while end < glyphs.len() && glyphs[end].cluster == cluster {
                width += font_units(glyphs[end].advance) * run.scale;
                end += 1;
            }
            if x < left + width / 2.0 {
                return if rtl {
                    logical_end(glyphs, at, run.text.end)
                } else {
                    glyphs[at].text_offset()
                };
            }
            left += width;
            at = end;
        }
        if rtl { run.text.start } else { run.text.end }
    }
}

// The offset where the cluster drawn at `at` ends in the text, in a run drawn
// right to left: the cluster drawn just left of it starts later in the text.
fn logical_end(glyphs: &[ShapedGlyph], at: usize, run_end: usize) -> usize {
    glyphs[..at]
        .iter()
        .rev()
        .map(ShapedGlyph::text_offset)
        .find(|&cluster| cluster != glyphs[at].text_offset())
        .unwrap_or(run_end)
}

// A distance in font design units as a float. Shaped values lie far inside
// the range of an `i16` for any font, whose em is at most 16,384 units.
pub(crate) fn font_units(value: i32) -> f32 {
    f32::from(i16::try_from(value).unwrap_or(if value < 0 { i16::MIN } else { i16::MAX }))
}
