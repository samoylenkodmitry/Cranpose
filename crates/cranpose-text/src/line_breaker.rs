use std::ops::Range;

use icu_segmenter::{
    LineSegmenter, LineSegmenterBorrowed,
    options::{LineBreakOptions, LineBreakStrictness},
};

use crate::{cluster_widths::ClusterWidths, paragraph::LineBreak};

// A place a line may end: the byte offset where the next line starts, and
// how a line ends there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BreakOpportunity {
    pub(crate) offset: usize,
    pub(crate) kind: BreakKind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum BreakKind {
    // Between words.
    Soft,
    // After a hard break such as a line feed: a line must end here.
    Hard,
    // Inside a word, with a hyphen `width` wide after it. Even breaking adds
    // `penalty` to the cost of the line ending here.
    #[cfg_attr(
        not(feature = "hyphenation"),
        expect(dead_code, reason = "only hyphenation makes hyphen breaks")
    )]
    Hyphen {
        width: f32,
        penalty: f32,
    },
}

// A line the breaking ended: its text, and whether a hyphen follows it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BrokenLine {
    pub(crate) text: Range<usize>,
    pub(crate) end: LineEnd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LineEnd {
    Plain,
    Hyphen,
}

impl BrokenLine {
    pub(crate) fn plain(text: Range<usize>) -> Self {
        Self {
            text,
            end: LineEnd::Plain,
        }
    }
}

// The line break opportunities of UAX #14 at each strictness Compose's
// `LineBreak` presets use: normal for `Simple`, strict for `Paragraph`,
// loose for `Heading`. Thai, Lao, Khmer and Burmese, which break between
// dictionary words, need the `southeast-asian-line-breaks` feature.
pub(crate) struct LineBreaker {
    loose: LineSegmenterBorrowed<'static>,
    normal: LineSegmenterBorrowed<'static>,
    strict: LineSegmenterBorrowed<'static>,
}

impl Default for LineBreaker {
    fn default() -> Self {
        Self {
            loose: segmenter(LineBreakStrictness::Loose),
            normal: segmenter(LineBreakStrictness::Normal),
            strict: segmenter(LineBreakStrictness::Strict),
        }
    }
}

fn segmenter(strictness: LineBreakStrictness) -> LineSegmenterBorrowed<'static> {
    let mut options = LineBreakOptions::default();
    options.strictness = Some(strictness);
    #[cfg(feature = "southeast-asian-line-breaks")]
    let segmenter = LineSegmenter::new_dictionary(options);
    #[cfg(not(feature = "southeast-asian-line-breaks"))]
    let segmenter = LineSegmenter::new_for_non_complex_scripts(options);
    segmenter
}

impl LineBreaker {
    // Replaces `breaks` with the opportunities of `text` for the line break
    // preset `line_break`, ascending; the last is the end of the text.
    pub(crate) fn opportunities(
        &self,
        text: &str,
        line_break: LineBreak,
        breaks: &mut Vec<BreakOpportunity>,
    ) {
        let segmenter = match line_break {
            LineBreak::Paragraph => self.strict,
            LineBreak::Heading => self.loose,
            LineBreak::Simple | LineBreak::Unspecified => self.normal,
        };
        breaks.clear();
        breaks.extend(
            segmenter
                .segment_str(text)
                .filter(|&offset| offset > 0)
                .map(|offset| BreakOpportunity {
                    offset,
                    kind: if text
                        .get(..offset)
                        .and_then(|before| before.chars().next_back())
                        .is_some_and(hard_break)
                    {
                        BreakKind::Hard
                    } else {
                        BreakKind::Soft
                    },
                }),
        );
    }
}

// The widths lines of a paragraph wrap at: the first line's, and the
// others', which indents make narrower.
#[derive(Clone, Copy, Debug)]
pub(crate) struct WrapWidths {
    pub(crate) first: f32,
    pub(crate) rest: f32,
}

// A text to break into lines: its text, the widths of its clusters, and the
// widths its lines wrap at.
#[derive(Clone, Copy)]
pub(crate) struct Wrapping<'a> {
    pub(crate) text: &'a str,
    pub(crate) widths: &'a ClusterWidths,
    pub(crate) wrap: WrapWidths,
}

impl Wrapping<'_> {
    // The width a line starting at `start` wraps at.
    fn limit(&self, start: usize) -> f32 {
        if starts_paragraph(self.text, start) {
            self.wrap.first
        } else {
            self.wrap.rest
        }
    }

    // The width of the line from `start` to `end` without its trailing
    // whitespace, which hangs past the line's end.
    fn line_width(&self, start: usize, end: usize) -> f32 {
        self.widths.width(start, visible_end(self.text, start, end))
    }

    // Greedy line breaking from `start` through `breaks`: each line takes as
    // many words as fit its wrap width, and a word wider than that breaks at
    // a hyphenation point or between clusters, one at least per line, as
    // Android's greedy breaker does. Appends the lines it ends and returns
    // where the unfinished last line starts.
    fn greedy(
        &self,
        mut start: usize,
        breaks: &[BreakOpportunity],
        lines: &mut Vec<BrokenLine>,
    ) -> usize {
        let mut fit = None;
        let mut index = 0;
        while let Some(&opportunity) = breaks.get(index) {
            let width = self.limit(start);
            if self.line_width(start, opportunity.offset) <= width {
                match opportunity.kind {
                    BreakKind::Hard => {
                        lines.push(BrokenLine::plain(start..opportunity.offset));
                        start = opportunity.offset;
                        fit = None;
                    }
                    BreakKind::Soft => fit = Some(opportunity.offset),
                    BreakKind::Hyphen { .. } => {}
                }
                index += 1;
            } else {
                let line = match fit.take() {
                    Some(end) => BrokenLine::plain(start..end),
                    None => self.cut_word(start, &breaks[..index], opportunity.offset, width),
                };
                start = line.text.end;
                lines.push(line);
            }
        }
        start
    }

    // The line from `start`, inside a word that does not fit `width` before
    // `end`: up to the last hyphenation point in `passed` that fits with its
    // hyphen, else up to the last cluster that fits, one cluster at least.
    fn cut_word(
        &self,
        start: usize,
        passed: &[BreakOpportunity],
        end: usize,
        width: f32,
    ) -> BrokenLine {
        let hyphen = passed
            .iter()
            .rev()
            .take_while(|opportunity| opportunity.offset > start)
            .find(|opportunity| match opportunity.kind {
                BreakKind::Hyphen { width: hyphen, .. } => {
                    self.widths.width(start, opportunity.offset) + hyphen <= width
                }
                BreakKind::Soft | BreakKind::Hard => false,
            });
        if let Some(opportunity) = hyphen {
            return BrokenLine {
                text: start..opportunity.offset,
                end: LineEnd::Hyphen,
            };
        }
        let cut = self.widths.last_fitting(start, end, width);
        let cut = if cut > start {
            cut
        } else {
            self.widths.next_boundary(start).unwrap_or(end)
        };
        BrokenLine::plain(start..cut)
    }

    // The limits from `.0` up to but not including `.1` at which greedy
    // breaking through `breaks` ends `line` where it does: it still fits,
    // and no later place the breaking would end it at fits.
    pub(crate) fn greedy_limits(
        &self,
        line: &BrokenLine,
        breaks: &[BreakOpportunity],
    ) -> (f32, f32) {
        let Range { start, end } = line.text;
        let at = breaks.partition_point(|opportunity| opportunity.offset < end);
        let ended_at = breaks
            .get(at)
            .filter(|opportunity| opportunity.offset == end)
            .map(|opportunity| opportunity.kind);
        let later = breaks.get(at + 1..).unwrap_or_default();
        match (line.end, ended_at) {
            (LineEnd::Hyphen, Some(BreakKind::Hyphen { width, .. })) => (
                self.widths.width(start, end) + width,
                self.next_word_fits(start, later),
            ),
            (LineEnd::Plain, Some(BreakKind::Soft)) => {
                let next = later
                    .iter()
                    .find(|opportunity| !is_hyphen(opportunity))
                    .map_or(f32::INFINITY, |opportunity| {
                        self.line_width(start, opportunity.offset)
                    });
                (self.line_width(start, end), next)
            }
            (LineEnd::Plain, Some(BreakKind::Hard)) => (self.line_width(start, end), f32::INFINITY),
            _ if end == self.text.len() => (self.line_width(start, end), f32::INFINITY),
            _ => self.cut_limits(line.text.clone(), breaks),
        }
    }

    // The limits of a line that cuts a word between clusters: the cut moves
    // once one more cluster fits, or a hyphenation point of the word does.
    fn cut_limits(
        &self,
        Range { start, end }: Range<usize>,
        breaks: &[BreakOpportunity],
    ) -> (f32, f32) {
        let width = self.widths.width(start, end);
        let fits = if width <= self.limit(start) {
            width
        } else {
            f32::NEG_INFINITY
        };
        let first = breaks.partition_point(|opportunity| opportunity.offset <= start);
        let cluster = self
            .widths
            .next_boundary(end)
            .map_or(f32::INFINITY, |next| self.widths.width(start, next));
        (
            fits,
            cluster.min(self.next_word_fits(start, &breaks[first..])),
        )
    }

    // The narrowest width the line from `start` takes ending at one of
    // `breaks` up to and with the next end between words.
    fn next_word_fits(&self, start: usize, breaks: &[BreakOpportunity]) -> f32 {
        let words = breaks
            .iter()
            .position(|opportunity| !is_hyphen(opportunity))
            .map_or(breaks.len(), |index| index + 1);
        breaks[..words]
            .iter()
            .map(|opportunity| match opportunity.kind {
                BreakKind::Hyphen { width, .. } => {
                    self.widths.width(start, opportunity.offset) + width
                }
                BreakKind::Soft | BreakKind::Hard => self.line_width(start, opportunity.offset),
            })
            .fold(f32::INFINITY, f32::min)
    }

    // Ends `lines` with the line from `start` to the end of the text, and an
    // empty line after a final hard break, so an empty text has one line.
    fn finish(&self, start: usize, breaks: &[BreakOpportunity], lines: &mut Vec<BrokenLine>) {
        let ends_with_break = breaks
            .last()
            .is_some_and(|opportunity| opportunity.kind == BreakKind::Hard);
        if start < self.text.len() || lines.is_empty() || ends_with_break {
            lines.push(BrokenLine::plain(start..self.text.len()));
        }
    }
}

// Greedy line breaking of the whole text into `lines`.
pub(crate) fn break_lines(
    wrapping: &Wrapping<'_>,
    breaks: &[BreakOpportunity],
    lines: &mut Vec<BrokenLine>,
) {
    lines.clear();
    let start = wrapping.greedy(0, breaks, lines);
    wrapping.finish(start, breaks, lines);
}

// Even line breaking, as Compose's `LineBreak.Paragraph` and `Heading` ask:
// each paragraph breaks where the sum over its lines of the square of the
// width each leaves free is least, the last line's share weighted by
// `last_line`, a fraction for `Paragraph` and the whole for `Heading`, plus
// the penalty of each line that ends in a hyphen. A paragraph with a word
// wider than its lines breaks greedily. Keeps its scratch storage from one
// text to the next.
#[derive(Default)]
pub(crate) struct LineBalancer {
    // The paragraph's start and break opportunities, the least cost of the
    // lines from each to the paragraph's end, and where the first of them
    // ends.
    nodes: Vec<BreakOpportunity>,
    cost: Vec<f32>,
    next: Vec<usize>,
}

impl LineBalancer {
    pub(crate) fn balance(
        &mut self,
        wrapping: &Wrapping<'_>,
        breaks: &[BreakOpportunity],
        last_line: f32,
        lines: &mut Vec<BrokenLine>,
    ) {
        lines.clear();
        let mut start = 0;
        let mut from = 0;
        while from < breaks.len() {
            let to = breaks[from..]
                .iter()
                .position(|opportunity| opportunity.kind == BreakKind::Hard)
                .map_or(breaks.len(), |at| from + at + 1);
            let paragraph = &breaks[from..to];
            start = if self.balance_paragraph(wrapping, start, paragraph, last_line, lines) {
                paragraph
                    .last()
                    .map_or(start, |opportunity| opportunity.offset)
            } else {
                wrapping.greedy(start, paragraph, lines)
            };
            from = to;
        }
        wrapping.finish(start, breaks, lines);
    }

    // Appends the even lines of the paragraph from `start` through
    // `breaks`; false, appending nothing, when a word does not fit.
    fn balance_paragraph(
        &mut self,
        wrapping: &Wrapping<'_>,
        start: usize,
        breaks: &[BreakOpportunity],
        last_line: f32,
        lines: &mut Vec<BrokenLine>,
    ) -> bool {
        self.nodes.clear();
        self.nodes.push(BreakOpportunity {
            offset: start,
            kind: BreakKind::Soft,
        });
        self.nodes.extend_from_slice(breaks);
        let last = self.nodes.len() - 1;
        self.cost.clear();
        self.cost.resize(self.nodes.len(), f32::INFINITY);
        self.next.clear();
        self.next.resize(self.nodes.len(), last);
        self.cost[last] = 0.0;
        for from in (0..last).rev() {
            let line_start = self.nodes[from].offset;
            let limit = wrapping.limit(line_start);
            for to in from + 1..=last {
                let end = self.nodes[to];
                let (hyphen, penalty) = match end.kind {
                    BreakKind::Hyphen { width, penalty } => (width, penalty),
                    BreakKind::Soft | BreakKind::Hard => (0.0, 0.0),
                };
                let width = wrapping.line_width(line_start, end.offset) + hyphen;
                if width > limit {
                    // A hyphen can tip a line over that the rest of its word
                    // keeps within; past a word, every later line is wider.
                    if let BreakKind::Hyphen { .. } = end.kind {
                        continue;
                    }
                    break;
                }
                let free = limit - width;
                let weight = if to == last { last_line } else { 1.0 };
                let cost = free * free * weight + penalty + self.cost[to];
                if cost < self.cost[from] {
                    self.cost[from] = cost;
                    self.next[from] = to;
                }
            }
        }
        if !self.cost[0].is_finite() {
            return false;
        }
        let mut at = 0;
        while at < last {
            let to = self.next[at];
            lines.push(BrokenLine {
                text: self.nodes[at].offset..self.nodes[to].offset,
                end: match self.nodes[to].kind {
                    BreakKind::Hyphen { .. } => LineEnd::Hyphen,
                    BreakKind::Soft | BreakKind::Hard => LineEnd::Plain,
                },
            });
            at = to;
        }
        true
    }
}

fn is_hyphen(opportunity: &BreakOpportunity) -> bool {
    matches!(opportunity.kind, BreakKind::Hyphen { .. })
}

// Whether `text` holds a hard line break.
pub(crate) fn has_hard_break(text: &str) -> bool {
    text.contains([
        '\n', '\r', '\u{0B}', '\u{0C}', '\u{85}', '\u{2028}', '\u{2029}',
    ])
}

// Whether `offset` starts a paragraph: the text's start, or just after a hard
// break.
pub(crate) fn starts_paragraph(text: &str, offset: usize) -> bool {
    text.get(..offset)
        .and_then(|before| before.chars().next_back())
        .is_none_or(hard_break)
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
