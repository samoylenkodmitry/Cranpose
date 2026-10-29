use crate::{
    layout_options::{TextLayoutOptions, TextOverflow},
    line_breaker::{BrokenLine, starts_paragraph},
};

// Where a line that does not fit shows an ellipsis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EllipsisAt {
    Start,
    Middle,
    End,
}

pub(crate) const ELLIPSIS: &str = "\u{2026}";

// Where the last line, or a line on its own, shows an ellipsis: a start or
// middle ellipsis only on a single line, as Android lays them out; text of
// more lines clips.
pub(crate) fn ellipsis_at(options: &TextLayoutOptions) -> Option<EllipsisAt> {
    match options.overflow {
        TextOverflow::Ellipsis => Some(EllipsisAt::End),
        TextOverflow::StartEllipsis if options.max_lines == 1 => Some(EllipsisAt::Start),
        TextOverflow::MiddleEllipsis if options.max_lines == 1 => Some(EllipsisAt::Middle),
        _ => None,
    }
}

// Makes line `last` run to the end of its paragraph over the lines after it
// a line limit drops, so its ellipsis cuts the paragraph where it stops
// fitting, as Android's StaticLayout treats them as one line.
pub(crate) fn take_rest_of_paragraph(lines: &mut [BrokenLine], text: &str, last: usize) {
    let end = lines[last + 1..]
        .iter()
        .take_while(|line| !starts_paragraph(text, line.text.start))
        .last()
        .map_or(lines[last].text.end, |line| line.text.end);
    lines[last] = BrokenLine::plain(lines[last].text.start..end);
}
