//! User-perceived character boundaries for text editing: Unicode's extended
//! grapheme clusters, such as a flag, an emoji joined with zero-width
//! joiners, or a letter with its combining marks. Offsets are UTF-8 bytes.

use unicode_segmentation::GraphemeCursor;

/// The boundary before `offset`, where Backspace, a left arrow or a
/// shift-left selection lands. `0` at the start of the text.
pub fn previous_grapheme_boundary(text: &str, offset: usize) -> usize {
    cursor(text, offset)
        .prev_boundary(text, 0)
        .ok()
        .flatten()
        .unwrap_or(0)
}

/// The boundary after `offset`, where Delete, a right arrow or a
/// shift-right selection lands. The text's length at its end.
pub fn next_grapheme_boundary(text: &str, offset: usize) -> usize {
    cursor(text, offset)
        .next_boundary(text, 0)
        .ok()
        .flatten()
        .unwrap_or(text.len())
}

/// The cursor holds the whole text as its one chunk, so it never asks for
/// more context and a step always succeeds.
fn cursor(text: &str, offset: usize) -> GraphemeCursor {
    GraphemeCursor::new(text.floor_char_boundary(offset), text.len(), true)
}
