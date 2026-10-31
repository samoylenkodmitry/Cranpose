use smallvec::SmallVec;

use crate::{
    bidi_level::BidiLevel,
    index_range::widen,
    laid_glyphs::{LaidGlyphs, RunGlyphs},
    paragraph::TextAlign,
    text_layout::{LayoutLine, LineRun, WidthRange},
    typeface::Typeface,
};

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
    // Whether a line runs right to left.
    pub(crate) rtl: bool,
    // Whether a line sits off its indent or stretches: new lines sit at
    // their indents, unstretched, until `align_lines` places them.
    pub(crate) off_indent: bool,
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
    pub(crate) fn width_at(&self, wrap: f32, min_width: f32) -> f32 {
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
    pub(crate) fn run_glyphs(&self, run: &LineRun) -> (RunGlyphs<'_>, &Typeface) {
        (
            self.glyphs.run(run.glyphs.range()),
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
        self.rtl = false;
        self.off_indent = false;
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

    // Whether `align` places every line at its indent whatever the width:
    // the start and the left do, of lines that run left to right.
    pub(crate) fn keeps_lines_at_indent(&self, align: TextAlign) -> bool {
        !self.rtl
            && matches!(
                align,
                TextAlign::Start | TextAlign::Left | TextAlign::Unspecified
            )
    }

    // Whether placing the lines across `width` as `align` says moves one:
    // every justified text does, and other text when a line's place across
    // the free width changes.
    pub(crate) fn lines_move(&self, width: f32, align: TextAlign) -> bool {
        if self.keeps_lines_at_indent(align) {
            return self.off_indent;
        }
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
        self.off_indent = false;
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
            self.off_indent |= line.stretch != 0.0 || line.left.to_bits() != line.indent.to_bits();
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
            let count = glyphs
                .run(run.glyphs.range())
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
