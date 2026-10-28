use std::ops::Range;

use icu_segmenter::{WordSegmenter, options::WordBreakInvariantOptions};

/// The word of `text` around `offset`, by the word boundaries of UAX #29:
/// Compose's `getWordBoundary`. At a boundary it is the word that starts
/// there, at the end of the text the word that ends there. Thai, Lao, Khmer
/// and Burmese words need the `southeast-asian-line-breaks` feature.
pub fn word_boundary(text: &str, offset: usize) -> Range<usize> {
    #[cfg(feature = "southeast-asian-line-breaks")]
    let segmenter = WordSegmenter::new_dictionary(WordBreakInvariantOptions::default());
    #[cfg(not(feature = "southeast-asian-line-breaks"))]
    let segmenter =
        WordSegmenter::new_for_non_complex_scripts(WordBreakInvariantOptions::default());
    let mut start = 0;
    for boundary in segmenter.segment_str(text).filter(|&boundary| boundary > 0) {
        if offset < boundary {
            return start..boundary;
        }
        start = boundary;
    }
    let before = text.get(..offset.min(text.len())).map_or(0, |before| {
        segmenter
            .segment_str(before)
            .filter(|&boundary| boundary < before.len())
            .last()
            .unwrap_or(0)
    });
    before..offset.min(text.len())
}
