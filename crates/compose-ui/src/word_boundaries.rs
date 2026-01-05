//! Word boundary detection for text field navigation.
//!
//! This module provides Unicode-aware word boundary functions for
//! Ctrl+Left/Right navigation and double-click word selection.
//!
//! Used by:
//! - `handle_key_event_impl` for Ctrl+Arrow navigation
//! - `create_handler` for double-click word selection

/// Finds the position at the start of the previous word.
/// Used for Ctrl+Left navigation.
///
/// Uses Unicode-aware `char::is_alphanumeric()` for proper international text support.
pub fn find_word_start(text: &str, pos: usize) -> usize {
    if pos == 0 || text.is_empty() {
        return 0;
    }

    // Get char indices up to pos
    let chars_before: Vec<(usize, char)> = text[..pos.min(text.len())].char_indices().collect();

    if chars_before.is_empty() {
        return 0;
    }

    let mut idx = chars_before.len();

    // Skip any whitespace/punctuation before (moving left)
    while idx > 0 {
        let c = chars_before[idx - 1].1;
        if c.is_alphanumeric() || c == '_' {
            break;
        }
        idx -= 1;
    }

    // Now scan back through word chars
    while idx > 0 {
        let c = chars_before[idx - 1].1;
        if !c.is_alphanumeric() && c != '_' {
            break;
        }
        idx -= 1;
    }

    if idx == 0 {
        0
    } else {
        chars_before[idx].0
    }
}

/// Finds the position at the end of the next word.
/// Used for Ctrl+Right navigation.
///
/// Uses Unicode-aware `char::is_alphanumeric()` for proper international text support.
pub fn find_word_end(text: &str, pos: usize) -> usize {
    let len = text.len();
    if pos >= len || text.is_empty() {
        return len;
    }

    // Get char indices from pos onwards
    let chars_after: Vec<(usize, char)> = text[pos.min(len)..]
        .char_indices()
        .map(|(i, c)| (pos + i, c))
        .collect();

    if chars_after.is_empty() {
        return len;
    }

    let mut idx = 0;

    // Skip any whitespace/punctuation (moving right)
    while idx < chars_after.len() {
        let c = chars_after[idx].1;
        if c.is_alphanumeric() || c == '_' {
            break;
        }
        idx += 1;
    }

    // Now scan forward through word chars
    while idx < chars_after.len() {
        let c = chars_after[idx].1;
        if !c.is_alphanumeric() && c != '_' {
            break;
        }
        idx += 1;
    }

    if idx >= chars_after.len() {
        len
    } else {
        chars_after[idx].0
    }
}

/// Finds the word boundaries (start, end) around a byte position.
/// Used for double-click word selection.
///
/// Returns the byte offsets of the word start and end.
pub fn find_word_boundaries(text: &str, pos: usize) -> (usize, usize) {
    if text.is_empty() {
        return (0, 0);
    }

    let pos = pos.min(text.len());

    // Find the character at pos
    let char_at_pos = text[pos..].chars().next();

    // If we're on whitespace/punctuation, just return the position
    let is_word_char = |c: char| c.is_alphanumeric() || c == '_';

    if char_at_pos.map(|c| !is_word_char(c)).unwrap_or(true) {
        // Check char before pos instead
        let char_before = text[..pos].chars().last();
        if char_before.map(|c| !is_word_char(c)).unwrap_or(true) {
            return (pos, pos);
        }
    }

    // Find word start (scan backwards)
    let mut start = pos;
    for (i, c) in text[..pos].char_indices().rev() {
        if !is_word_char(c) {
            start = i + c.len_utf8();
            break;
        }
        start = i;
    }

    // Find word end (scan forwards)
    let mut end = pos;
    for (i, c) in text[pos..].char_indices() {
        if !is_word_char(c) {
            end = pos + i;
            break;
        }
        end = pos + i + c.len_utf8();
    }

    (start, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_word_start() {
        assert_eq!(find_word_start("hello world", 6), 0); // Before 'w', goes to 'h'
        assert_eq!(find_word_start("hello world", 11), 6); // End, goes to 'w'
        assert_eq!(find_word_start("hello", 0), 0);
    }

    #[test]
    fn test_find_word_end() {
        assert_eq!(find_word_end("hello world", 0), 5); // At 'h', goes to after 'o'
        assert_eq!(find_word_end("hello world", 6), 11); // At 'w', goes to end
    }

    #[test]
    fn test_find_word_boundaries() {
        let (start, end) = find_word_boundaries("hello world", 2);
        assert_eq!(start, 0);
        assert_eq!(end, 5);

        let (start, end) = find_word_boundaries("hello world", 8);
        assert_eq!(start, 6);
        assert_eq!(end, 11);
    }
}
