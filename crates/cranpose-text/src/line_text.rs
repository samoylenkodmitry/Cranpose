use std::ops::Range;

use crate::{
    layout_input::LayoutInput,
    line_breaker::{hard_break, starts_paragraph},
};

// A line's text, the end of its visible text, the end of the text it draws,
// before the hard break that ends it, which draws nothing, its indent and
// the width the paragraph's lines wrap at.
pub(crate) struct LineText {
    pub(crate) range: Range<usize>,
    pub(crate) visible: usize,
    pub(crate) drawn: usize,
    pub(crate) indent: f32,
    pub(crate) width: f32,
}

// How far down a layout's lines reach, in layout pixels and, exactly, in
// whole device pixels.
#[derive(Clone, Copy, Default)]
pub(crate) struct LineDepth {
    pub(crate) layout: f32,
    pub(crate) pixels: f32,
}

impl LineDepth {
    // `pixels` whole device pixels down a layout on `grid`.
    pub(crate) fn at(pixels: f32, grid: f32) -> Self {
        Self {
            layout: pixels / grid,
            pixels,
        }
    }
}

// Where a line stands among a layout's lines: the bottom of the line above,
// whether it is the first and the last, and whether it shows an ellipsis.
#[derive(Clone, Copy)]
pub(crate) struct LineEdges {
    pub(crate) top: LineDepth,
    pub(crate) first: bool,
    pub(crate) last: bool,
    pub(crate) ellipsized: bool,
}

// The end of the text from `start` to `end` before the hard break that ends
// it, if one does.
pub(crate) fn before_hard_break(text: &str, start: usize, end: usize) -> usize {
    text.get(start..end)
        .map_or(end, |part| start + part.trim_end_matches(hard_break).len())
}

// The indent of the line starting at `offset`.
pub(crate) fn indent(input: &LayoutInput<'_>, offset: usize) -> f32 {
    let lengths = &input.lengths;
    paragraph_indent(
        input.text,
        offset,
        lengths.first_line_indent,
        lengths.rest_line_indent,
    )
}

// The indent of the line of `text` starting at `offset`: `first` where a
// paragraph starts, else `rest`. Out of line, so that the breaker and the
// layouter share one copy of the test.
#[inline(never)]
pub(crate) fn paragraph_indent(text: &str, offset: usize, first: f32, rest: f32) -> f32 {
    if starts_paragraph(text, offset) {
        first
    } else {
        rest
    }
}
