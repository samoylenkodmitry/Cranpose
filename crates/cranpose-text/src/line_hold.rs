// The widths at which a layout keeps its lines: a layout of the text at a
// width inside them breaks it into the same lines.

use crate::{
    cluster_widths::ClusterWidths,
    ellipsis_at::ellipsis_at,
    layout_input::LayoutInput,
    layout_options::TextLayoutOptions,
    line_breaker::{BreakOpportunity, BrokenLine, Wrapping, visible_end},
    line_text::LineText,
    paragraph::LineBreak,
    text_layout::WidthRange,
};

// The hold of a text that fits on one line `line` wide with its indent:
// any width that keeps it on one line, when it wraps.
pub(crate) fn fitting_hold(options: &TextLayoutOptions, line: f32) -> WidthRange {
    WidthRange {
        min: if options.soft_wrap {
            line
        } else {
            f32::NEG_INFINITY
        },
        max: f32::INFINITY,
    }
}

// The hold of a text broken into `lines` at `breaks`: every width when
// lines do not wrap, for greedy breaking the wrap widths at which each line
// that shows keeps its end, and none for even breaking, whose
// lines can move with any width. No line past `max_lines` shows, and with
// an ellipsis the last that shows takes the rest of its paragraph and holds
// by its ellipsis.
pub(crate) fn wrapped_hold(
    lines: &[BrokenLine],
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
    let shown = if lines.len() > options.max_lines {
        options
            .max_lines
            .saturating_sub(usize::from(ellipsis_at(options).is_some()))
    } else {
        lines.len()
    };
    let mut hold = WidthRange::ALL;
    for line in &lines[..shown] {
        let (min, max) = wrapping.greedy_limits(line, breaks);
        hold.narrow(min, max);
    }
    Some(hold)
}

// The widths at which an end ellipsis follows the same text of `line`, cut
// at `cut`: from the width at which the text it shows fits with `edge`, the
// ellipsis and the line's indent, up to the width at which one more cluster
// that shows fits, since the spaces the cut passes show nothing. A line that
// shows no text before its ellipsis shows none at any narrower width.
pub(crate) fn end_ellipsis_hold(
    widths: &ClusterWidths,
    text: &str,
    line: &LineText,
    cut: usize,
    edge: f32,
) -> WidthRange {
    let start = line.range.start;
    let fitting = |offset| widths.width(start, offset) + edge;
    let shown = visible_end(text, start, cut);
    let mut passed = cut;
    let max = loop {
        match widths
            .next_boundary(passed)
            .filter(|_| passed < line.visible)
        {
            Some(next) if visible_end(text, start, next) == shown => passed = next,
            Some(next) => break fitting(next),
            None => break f32::INFINITY,
        }
    };
    WidthRange {
        min: if shown > start {
            fitting(shown)
        } else {
            f32::NEG_INFINITY
        },
        max,
    }
}
