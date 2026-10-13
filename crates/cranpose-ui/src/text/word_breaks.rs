//! Where a line of Thai, Lao, Khmer or Burmese may wrap. These scripts put no
//! space between words, so a line wraps between the words a dictionary finds
//! (ICU's line segmenter) instead of at spaces.

/// The character indexes into `boundaries`, inside `line`, before which a line
/// may wrap between words of a script written without spaces, ascending.
/// Empty when the line has no such script, or without the
/// `southeast-asian-line-breaks` feature.
pub(crate) fn unspaced_word_breaks(line: &str, boundaries: &[usize]) -> Vec<usize> {
    #[cfg(feature = "southeast-asian-line-breaks")]
    if line.chars().any(is_written_without_spaces) {
        let end = boundaries.len() - 1;
        return icu_segmenter::LineSegmenter::new_dictionary(Default::default())
            .segment_str(line)
            .filter_map(|byte| boundaries.binary_search(&byte).ok())
            .filter(|&idx| idx > 0 && idx < end)
            .collect();
    }
    #[cfg(not(feature = "southeast-asian-line-breaks"))]
    let _ = (line, boundaries);
    Vec::new()
}

/// Whether `ch` belongs to a script whose words a line breaks between by
/// dictionary: Unicode line break class SA (complex context).
#[cfg(feature = "southeast-asian-line-breaks")]
fn is_written_without_spaces(ch: char) -> bool {
    use icu_properties::{CodePointMapData, props::LineBreak};
    CodePointMapData::<LineBreak>::new().get(ch) == LineBreak::ComplexContext
}
