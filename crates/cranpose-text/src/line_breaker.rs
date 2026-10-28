use icu_segmenter::{
    LineSegmenter, LineSegmenterBorrowed,
    options::{LineBreakOptions, LineBreakStrictness},
};

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

// The characters after which UAX #14 ends a line: the classes BK, CR, LF and
// NL.
fn hard_break(ch: char) -> bool {
    matches!(
        ch,
        '\n' | '\r' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}
