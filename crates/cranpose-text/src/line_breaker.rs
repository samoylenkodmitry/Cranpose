use std::ops::Range;

use icu_segmenter::{
    LineSegmenter, LineSegmenterBorrowed,
    options::{LineBreakOptions, LineBreakStrictness},
};

use crate::cluster_widths::ClusterWidths;

// A place a line may end: the byte offset where the next line starts, and
// whether a line must end there, after a hard break such as a line feed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BreakOpportunity {
    pub(crate) offset: usize,
    pub(crate) mandatory: bool,
}

// The line break opportunities of UAX #14 at the normal strictness of
// Compose's `LineBreak.Simple`. Thai, Lao, Khmer and Burmese, which break
// between dictionary words, need the `southeast-asian-line-breaks` feature.
pub(crate) struct LineBreaker {
    segmenter: LineSegmenterBorrowed<'static>,
}

impl Default for LineBreaker {
    fn default() -> Self {
        let mut options = LineBreakOptions::default();
        options.strictness = Some(LineBreakStrictness::Normal);
        #[cfg(feature = "southeast-asian-line-breaks")]
        let segmenter = LineSegmenter::new_dictionary(options);
        #[cfg(not(feature = "southeast-asian-line-breaks"))]
        let segmenter = LineSegmenter::new_for_non_complex_scripts(options);
        Self { segmenter }
    }
}

impl LineBreaker {
    // Replaces `breaks` with the opportunities of `text`, ascending; the
    // last is the end of the text.
    pub(crate) fn opportunities(&self, text: &str, breaks: &mut Vec<BreakOpportunity>) {
        breaks.clear();
        breaks.extend(
            self.segmenter
                .segment_str(text)
                .filter(|&offset| offset > 0)
                .map(|offset| BreakOpportunity {
                    offset,
                    mandatory: text
                        .get(..offset)
                        .and_then(|before| before.chars().next_back())
                        .is_some_and(hard_break),
                }),
        );
    }
}

// Greedy line breaking: each line of `text` takes as many of `breaks` as fit
// `width` without its trailing whitespace, which hangs past the line's end,
// and a word wider than `width` breaks between clusters, one at least per
// line. Replaces `lines` with the line ranges; a text that is empty or ends
// with a hard break gets an empty last line.
pub(crate) fn break_lines(
    text: &str,
    widths: &ClusterWidths,
    breaks: &[BreakOpportunity],
    width: f32,
    lines: &mut Vec<Range<usize>>,
) {
    lines.clear();
    let mut start = 0;
    let mut fit = None;
    let mut index = 0;
    while let Some(&opportunity) = breaks.get(index) {
        if widths.width(start, visible_end(text, start, opportunity.offset)) <= width {
            if opportunity.mandatory {
                lines.push(start..opportunity.offset);
                start = opportunity.offset;
                fit = None;
            } else {
                fit = Some(opportunity.offset);
            }
            index += 1;
        } else {
            let end = fit.take().unwrap_or_else(|| {
                let cut = widths.last_fitting(start, opportunity.offset, width);
                if cut > start {
                    cut
                } else {
                    widths.next_boundary(start).unwrap_or(opportunity.offset)
                }
            });
            lines.push(start..end);
            start = end;
        }
    }
    let ends_with_break = breaks
        .last()
        .is_some_and(|opportunity| opportunity.mandatory);
    if start < text.len() || lines.is_empty() || ends_with_break {
        lines.push(start..text.len());
    }
}

// The end of the text from `start` to `end` before its trailing whitespace.
pub(crate) fn visible_end(text: &str, start: usize, end: usize) -> usize {
    text.get(start..end)
        .map_or(end, |part| start + part.trim_end().len())
}

// The characters after which UAX #14 ends a line: the classes BK, CR, LF and
// NL.
fn hard_break(ch: char) -> bool {
    matches!(
        ch,
        '\n' | '\r' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}
