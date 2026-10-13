//! Where a line of text written without spaces may wrap. Chinese and Japanese
//! wrap between their characters, but not before closing punctuation or a
//! small kana, nor after opening punctuation (the Unicode line breaking rules
//! for classes ID and CJ). Thai, Lao, Khmer and Burmese wrap between the
//! words a dictionary finds (ICU's line segmenter).

use icu_properties::{CodePointMapData, props::LineBreak};

/// The character indexes into `boundaries`, inside `line`, before which a line
/// may wrap inside text written without spaces, ascending. Empty when the line
/// has none; Thai, Lao, Khmer and Burmese need the
/// `southeast-asian-line-breaks` feature.
pub(crate) fn unspaced_word_breaks(line: &str, boundaries: &[usize]) -> Vec<usize> {
    let classes = CodePointMapData::<LineBreak>::new();
    let end = boundaries.len() - 1;
    let breaks: Vec<usize> = if line.chars().any(|ch| ideographic(classes.get(ch))) {
        (1..end)
            .filter(|&idx| {
                let before = line[..boundaries[idx]].chars().next_back();
                let after = line[boundaries[idx]..].chars().next();
                before.zip(after).is_some_and(|(before, after)| {
                    may_wrap_between(classes.get(before), classes.get(after))
                })
            })
            .collect()
    } else {
        Vec::new()
    };
    #[cfg(feature = "southeast-asian-line-breaks")]
    if line
        .chars()
        .any(|ch| classes.get(ch) == LineBreak::ComplexContext)
    {
        let words = icu_segmenter::LineSegmenter::new_dictionary(Default::default())
            .segment_str(line)
            .filter_map(|byte| boundaries.binary_search(&byte).ok())
            .filter(|&idx| idx > 0 && idx < end);
        let mut breaks = breaks;
        breaks.extend(words);
        breaks.sort_unstable();
        breaks.dedup();
        return breaks;
    }
    breaks
}

/// A Chinese or Japanese character a line may wrap beside.
fn ideographic(class: LineBreak) -> bool {
    class == LineBreak::Ideographic || class == LineBreak::ConditionalJapaneseStarter
}

/// Whether a line may wrap between a character of class `before` and one of
/// class `after`, one of them Chinese or Japanese.
fn may_wrap_between(before: LineBreak, after: LineBreak) -> bool {
    let starts_badly = matches!(
        after,
        LineBreak::ClosePunctuation
            | LineBreak::CloseParenthesis
            | LineBreak::ConditionalJapaneseStarter
            | LineBreak::Exclamation
            | LineBreak::InfixNumeric
            | LineBreak::Nonstarter
            | LineBreak::BreakSymbols
            | LineBreak::BreakAfter
            | LineBreak::Hyphen
            | LineBreak::Quotation
            | LineBreak::Glue
            | LineBreak::WordJoiner
            | LineBreak::CombiningMark
            | LineBreak::ZWJ
    );
    let ends_badly = matches!(
        before,
        LineBreak::OpenPunctuation
            | LineBreak::Quotation
            | LineBreak::Glue
            | LineBreak::WordJoiner
            | LineBreak::BreakBefore
            | LineBreak::ZWJ
    );
    (ideographic(before) || ideographic(after)) && !starts_badly && !ends_badly
}
