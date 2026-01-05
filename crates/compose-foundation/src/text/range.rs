//! Text range for representing cursor position and selection.
//!
//! Matches Jetpack Compose's `androidx.compose.ui.text.TextRange`.

/// Represents a range in text, used for cursor position and selection.
///
/// When `start == end`, this represents a cursor position (collapsed selection).
/// When `start != end`, this represents a text selection.
///
/// # Invariants
///
/// - Indices are in UTF-8 byte offsets (matching Rust's `String`)
/// - `start` can be greater than `end` for reverse selections
/// - Use `min()` and `max()` for ordered access
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash)]
pub struct TextRange {
    /// Start index of the range (can be > end for reverse selection)
    pub start: usize,
    /// End index of the range
    pub end: usize,
}

impl TextRange {
    /// Creates a new text range.
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// Creates a collapsed range (cursor) at the given position.
    pub const fn cursor(position: usize) -> Self {
        Self {
            start: position,
            end: position,
        }
    }

    /// Creates a range from 0 to 0 (cursor at start).
    pub const fn zero() -> Self {
        Self { start: 0, end: 0 }
    }

    /// Returns true if this range is collapsed (cursor, not selection).
    pub const fn collapsed(&self) -> bool {
        self.start == self.end
    }

    /// Returns the length of the selection in characters.
    pub fn length(&self) -> usize {
        self.end.abs_diff(self.start)
    }

    /// Returns the minimum (leftmost) index.
    pub fn min(&self) -> usize {
        self.start.min(self.end)
    }

    /// Returns the maximum (rightmost) index.
    pub fn max(&self) -> usize {
        self.start.max(self.end)
    }

    /// Returns true if this range contains the given index.
    pub fn contains(&self, index: usize) -> bool {
        index >= self.min() && index < self.max()
    }

    /// Coerces the range to be within [0, max].
    pub fn coerce_in(&self, max: usize) -> Self {
        Self {
            start: self.start.min(max),
            end: self.end.min(max),
        }
    }

    /// Returns a range covering the entire text of given length.
    pub const fn all(length: usize) -> Self {
        Self {
            start: 0,
            end: length,
        }
    }

    /// Safely slices the text, clamping to valid UTF-8 char boundaries.
    ///
    /// This handles edge cases where:
    /// - Range extends beyond text length
    /// - Range indices are not on char boundaries (e.g., in middle of multi-byte UTF-8)
    ///
    /// Returns an empty string if the range is invalid.
    pub fn safe_slice<'a>(&self, text: &'a str) -> &'a str {
        if text.is_empty() {
            return "";
        }

        let start = self.min().min(text.len());
        let end = self.max().min(text.len());

        // Clamp start to valid char boundary (scan backward)
        let start = if text.is_char_boundary(start) {
            start
        } else {
            // Find previous char boundary by scanning backward
            (0..start)
                .rev()
                .find(|&i| text.is_char_boundary(i))
                .unwrap_or(0)
        };

        // Clamp end to valid char boundary (scan forward)
        let end = if text.is_char_boundary(end) {
            end
        } else {
            // Find next char boundary by scanning forward
            (end..=text.len())
                .find(|&i| text.is_char_boundary(i))
                .unwrap_or(text.len())
        };

        if start <= end {
            &text[start..end]
        } else {
            ""
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_is_collapsed() {
        let cursor = TextRange::cursor(5);
        assert!(cursor.collapsed());
        assert_eq!(cursor.length(), 0);
        assert_eq!(cursor.start, 5);
        assert_eq!(cursor.end, 5);
    }

    #[test]
    fn selection_is_not_collapsed() {
        let selection = TextRange::new(2, 7);
        assert!(!selection.collapsed());
        assert_eq!(selection.length(), 5);
    }

    #[test]
    fn reverse_selection_length() {
        let reverse = TextRange::new(7, 2);
        assert_eq!(reverse.length(), 5);
        assert_eq!(reverse.min(), 2);
        assert_eq!(reverse.max(), 7);
    }

    #[test]
    fn coerce_in_bounds() {
        let range = TextRange::new(5, 100);
        let coerced = range.coerce_in(10);
        assert_eq!(coerced.start, 5);
        assert_eq!(coerced.end, 10);
    }

    #[test]
    fn contains_index() {
        let range = TextRange::new(2, 5);
        assert!(!range.contains(1));
        assert!(range.contains(2));
        assert!(range.contains(3));
        assert!(range.contains(4));
        assert!(!range.contains(5)); // exclusive end
    }

    #[test]
    fn safe_slice_basic() {
        let range = TextRange::new(0, 5);
        assert_eq!(range.safe_slice("Hello World"), "Hello");
    }

    #[test]
    fn safe_slice_beyond_bounds() {
        let range = TextRange::new(0, 100);
        assert_eq!(range.safe_slice("Hello"), "Hello");
    }

    #[test]
    fn safe_slice_unicode() {
        // "Hello 🌍" - emoji is 4 bytes
        let text = "Hello 🌍";
        // Range in middle of emoji (byte 7 is inside the 4-byte emoji starting at 6)
        let range = TextRange::new(0, 7);
        let slice = range.safe_slice(text);
        // Should clamp to valid boundary (either before or after emoji)
        assert!(slice == "Hello " || slice == "Hello 🌍");
    }
}
