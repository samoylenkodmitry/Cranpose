//! Compose's `OutputTransformation`: how a text field shows its text, and
//! how offsets in the text line up with offsets in what it shows.

use std::{ops::Range, rc::Rc};

use unicode_segmentation::UnicodeSegmentation;

/// Changes how a text field shows its text without changing the text, as
/// Compose's `OutputTransformation`: a phone number shown with spaces, a
/// password shown as dots. The caret, selection, composition and pointer
/// positions map between the text and what is shown.
///
/// A transformation names its changes in [`OutputChanges`], in offsets of
/// the text it gets.
#[derive(Clone)]
pub struct OutputTransformation(Rc<Transform>);

type Transform = dyn Fn(&str, &mut OutputChanges);

impl OutputTransformation {
    pub fn new(transform: impl Fn(&str, &mut OutputChanges) + 'static) -> Self {
        Self(Rc::new(transform))
    }

    /// Shows every character (extended grapheme cluster) as `mask`.
    pub fn mask(mask: char) -> Self {
        let mut buffer = [0; 4];
        let mask: Rc<str> = Rc::from(&*mask.encode_utf8(&mut buffer));
        Self::new(move |text, changes| {
            for (start, grapheme) in text.grapheme_indices(true) {
                changes.replace(start..start + grapheme.len(), &mask);
            }
        })
    }

    /// The text as this transformation shows it.
    pub fn apply(&self, text: &str) -> ShownText {
        let mut changes = OutputChanges::default();
        (self.0)(text, &mut changes);
        changes.apply(text)
    }
}

impl std::fmt::Debug for OutputTransformation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OutputTransformation")
    }
}

impl PartialEq for OutputTransformation {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

/// The changes an output transformation makes, in byte offsets of the text
/// it was given. They may come in any order; a change that overlaps an
/// earlier one is dropped.
#[derive(Default)]
pub struct OutputChanges {
    changes: Vec<(Range<usize>, Rc<str>)>,
}

impl OutputChanges {
    /// Shows `text` before the character at `at`.
    pub fn insert(&mut self, at: usize, text: &str) {
        self.replace(at..at, text);
    }

    /// Shows `text` in place of `range`.
    pub fn replace(&mut self, range: Range<usize>, text: &str) {
        self.changes.push((range, Rc::from(text)));
    }

    fn apply(mut self, text: &str) -> ShownText {
        self.changes
            .sort_by_key(|(range, _)| (range.start, range.end));
        let mut shown = String::with_capacity(text.len());
        let mut segments = Vec::with_capacity(self.changes.len() * 2 + 1);
        let mut source = 0;
        for (range, replacement) in &self.changes {
            let (start, end) = (
                text.floor_char_boundary(range.start),
                text.floor_char_boundary(range.end.max(range.start)),
            );
            if start < source {
                continue;
            }
            push_segment(
                &mut segments,
                &mut shown,
                source..start,
                &text[source..start],
                false,
            );
            push_segment(&mut segments, &mut shown, start..end, replacement, true);
            source = end;
        }
        push_segment(
            &mut segments,
            &mut shown,
            source..text.len(),
            &text[source..],
            false,
        );
        ShownText {
            text: shown,
            mapping: OffsetMapping { segments },
        }
    }
}

fn push_segment(
    segments: &mut Vec<Segment>,
    shown: &mut String,
    source: Range<usize>,
    text: &str,
    changed: bool,
) {
    if source.is_empty() && text.is_empty() {
        return;
    }
    let start = shown.len();
    shown.push_str(text);
    segments.push(Segment {
        source,
        shown: start..shown.len(),
        changed,
    });
}

/// A text as a field shows it, with how its offsets line up with the text's.
#[derive(Clone, Debug, PartialEq)]
pub struct ShownText {
    pub text: String,
    pub mapping: OffsetMapping,
}

/// How offsets in a text line up with offsets in the text a field shows
/// for it. Text a transformation left alone maps offset for offset; a
/// changed run maps as a whole, so the caret never stands inside text the
/// transformation put in.
#[derive(Clone, Debug, PartialEq)]
pub struct OffsetMapping {
    segments: Vec<Segment>,
}

#[derive(Clone, Debug, PartialEq)]
struct Segment {
    source: Range<usize>,
    shown: Range<usize>,
    changed: bool,
}

impl OffsetMapping {
    /// Where an offset of the text is shown. An offset at the start of a
    /// changed run shows before it, one inside or at its end after it.
    pub fn to_shown(&self, source: usize) -> usize {
        let index = self
            .segments
            .partition_point(|segment| segment.source.end < source);
        let Some(segment) = self.segments.get(index) else {
            return self.segments.last().map_or(source, |last| last.shown.end);
        };
        if !segment.changed {
            segment.shown.start + source.saturating_sub(segment.source.start)
        } else if source <= segment.source.start {
            segment.shown.start
        } else {
            segment.shown.end
        }
    }

    /// The text offset a shown offset stands for. A shown offset inside a
    /// changed run stands for the run's nearer end.
    pub fn to_source(&self, shown: usize) -> usize {
        let index = self
            .segments
            .partition_point(|segment| segment.shown.end < shown);
        let Some(segment) = self.segments.get(index) else {
            return self.segments.last().map_or(shown, |last| last.source.end);
        };
        if !segment.changed {
            return segment.source.start + shown.saturating_sub(segment.shown.start);
        }
        let to_start = shown.saturating_sub(segment.shown.start);
        let to_end = segment.shown.end.saturating_sub(shown);
        if to_start < to_end {
            segment.source.start
        } else {
            segment.source.end
        }
    }

    /// The shown range a range of the text covers.
    pub fn range_to_shown(&self, range: Range<usize>) -> Range<usize> {
        self.to_shown(range.start)..self.to_shown(range.end)
    }
}
