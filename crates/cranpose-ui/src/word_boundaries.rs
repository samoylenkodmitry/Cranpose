//! Word boundaries for keyboard word moves and double-click selection.
//!
//! A word is a run of characters that start with an alphanumeric code point
//! or `_`. Characters are extended grapheme clusters, so a letter keeps its
//! combining marks and a boundary never splits an emoji.

use unicode_segmentation::UnicodeSegmentation;

fn is_word(grapheme: &str) -> bool {
    grapheme
        .chars()
        .next()
        .is_some_and(|c| c.is_alphanumeric() || c == '_')
}

pub(crate) fn find_word_start(text: &str, pos: usize) -> usize {
    let pos = text.floor_char_boundary(pos);
    let mut word_start = None;
    for (index, grapheme) in text[..pos].grapheme_indices(true).rev() {
        if is_word(grapheme) {
            word_start = Some(index);
        } else if let Some(start) = word_start {
            return start;
        }
    }
    0
}

pub(crate) fn find_word_end(text: &str, pos: usize) -> usize {
    let pos = text.floor_char_boundary(pos);
    let mut in_word = false;
    for (index, grapheme) in text[pos..].grapheme_indices(true) {
        if is_word(grapheme) {
            in_word = true;
        } else if in_word {
            return pos + index;
        }
    }
    text.len()
}

pub(crate) fn find_word_boundaries(text: &str, pos: usize) -> (usize, usize) {
    let pos = text.floor_char_boundary(pos);
    let word_after = text[pos..].graphemes(true).next().is_some_and(is_word);
    let word_before = text[..pos].graphemes(true).next_back().is_some_and(is_word);
    if !word_after && !word_before {
        return (pos, pos);
    }
    let start = text[..pos]
        .grapheme_indices(true)
        .rev()
        .take_while(|(_, grapheme)| is_word(grapheme))
        .last()
        .map_or(pos, |(index, _)| index);
    let end = text[pos..]
        .grapheme_indices(true)
        .find(|(_, grapheme)| !is_word(grapheme))
        .map_or(text.len(), |(index, _)| pos + index);
    (start, end)
}

#[cfg(test)]
#[path = "tests/word_boundaries_tests.rs"]
mod tests;
