use std::ops::Range;

use crate::{
    layout_input::LayoutInput,
    line_breaker::{hard_break, starts_paragraph},
};

// A line's text, the end of its visible text, the end of the text it draws,
// before the hard break that ends it, which draws nothing, its indent and
// the width its text may take, the paragraph's past the indent.
pub(crate) struct LineText {
    pub(crate) range: Range<usize>,
    pub(crate) visible: usize,
    pub(crate) drawn: usize,
    pub(crate) indent: f32,
    pub(crate) room: f32,
}

// Where a line stands among a layout's lines: the bottom of the line above,
// whether it is the first and the last, and whether it shows an ellipsis.
#[derive(Clone, Copy)]
pub(crate) struct LineEdges {
    pub(crate) top: f32,
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
    if starts_paragraph(input.text, offset) {
        input.lengths.first_line_indent
    } else {
        input.lengths.rest_line_indent
    }
}
