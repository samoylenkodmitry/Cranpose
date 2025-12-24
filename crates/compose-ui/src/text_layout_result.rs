//! Text layout result with cached glyph positions.
//!
//! This module provides `TextLayoutResult` which caches glyph X positions
//! computed during text measurement, enabling O(1) cursor positioning and
//! selection rendering instead of O(n²) substring measurements.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Layout information for a single line of text.
#[derive(Debug, Clone)]
pub struct LineLayout {
    /// Byte offset where line starts
    pub start_offset: usize,
    /// Byte offset where line ends (exclusive, before \n or at text end)
    pub end_offset: usize,
    /// Y position of line top
    pub y: f32,
    /// Height of line
    pub height: f32,
}

/// Cached text layout result with pre-computed glyph positions.
///
/// Compute once during `measure()`, reuse for:
/// - Cursor X position rendering
/// - Selection highlight geometry
/// - Click-to-position cursor
#[derive(Debug, Clone)]
pub struct TextLayoutResult {
    /// Total width of laid out text
    pub width: f32,
    /// Total height of laid out text
    pub height: f32,
    /// Height of a single line
    pub line_height: f32,
    /// X position at each character boundary (including end)
    /// glyph_x_positions[i] = x position before character at char index i
    /// glyph_x_positions[char_count] = x position at end of text
    glyph_x_positions: Vec<f32>,
    /// Byte offset for each character index
    /// char_to_byte[i] = byte offset of character at char index i
    char_to_byte: Vec<usize>,
    /// Line layout information
    pub lines: Vec<LineLayout>,
    /// Hash of text this was computed for (for validation)
    text_hash: u64,
}

impl TextLayoutResult {
    /// Creates a new layout result with the given glyph positions.
    pub fn new(
        width: f32,
        height: f32,
        line_height: f32,
        glyph_x_positions: Vec<f32>,
        char_to_byte: Vec<usize>,
        lines: Vec<LineLayout>,
        text: &str,
    ) -> Self {
        Self {
            width,
            height,
            line_height,
            glyph_x_positions,
            char_to_byte,
            lines,
            text_hash: Self::hash_text(text),
        }
    }

    /// Returns X position for cursor at given byte offset.
    /// O(1) lookup from pre-computed positions.
    pub fn get_cursor_x(&self, byte_offset: usize) -> f32 {
        // Binary search for char index containing this byte offset
        let char_idx = self
            .char_to_byte
            .iter()
            .position(|&b| b > byte_offset)
            .map(|i| i.saturating_sub(1))
            .unwrap_or(self.char_to_byte.len().saturating_sub(1));

        // Return X position at that char boundary
        self.glyph_x_positions
            .get(char_idx)
            .copied()
            .unwrap_or(self.width)
    }

    /// Returns byte offset for X position.
    /// O(log n) binary search through glyph positions.
    pub fn get_offset_for_x(&self, x: f32) -> usize {
        if self.glyph_x_positions.is_empty() {
            return 0;
        }

        // Binary search for closest glyph boundary
        let char_idx = match self
            .glyph_x_positions
            .binary_search_by(|pos| pos.partial_cmp(&x).unwrap_or(std::cmp::Ordering::Equal))
        {
            Ok(i) => i,
            Err(i) => {
                // Between two positions - pick closest
                if i == 0 {
                    0
                } else if i >= self.glyph_x_positions.len() {
                    self.glyph_x_positions.len() - 1
                } else {
                    let before = self.glyph_x_positions[i - 1];
                    let after = self.glyph_x_positions[i];
                    if (x - before) < (after - x) {
                        i - 1
                    } else {
                        i
                    }
                }
            }
        };

        // Convert char index to byte offset
        self.char_to_byte.get(char_idx).copied().unwrap_or(0)
    }

    /// Checks if this layout result is valid for the given text.
    pub fn is_valid_for(&self, text: &str) -> bool {
        self.text_hash == Self::hash_text(text)
    }

    fn hash_text(text: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        hasher.finish()
    }

    /// Creates a simple layout for monospaced text (for fallback).
    pub fn monospaced(text: &str, char_width: f32, line_height: f32) -> Self {
        let mut glyph_x_positions = Vec::new();
        let mut char_to_byte = Vec::new();
        let mut x = 0.0;

        for (byte_offset, _c) in text.char_indices() {
            glyph_x_positions.push(x);
            char_to_byte.push(byte_offset);
            x += char_width;
        }
        // Add end position
        glyph_x_positions.push(x);
        char_to_byte.push(text.len());

        // Compute lines - collect once and reuse
        let line_texts: Vec<&str> = text.split('\n').collect();
        let line_count = line_texts.len();
        let mut lines = Vec::with_capacity(line_count);
        let mut line_start = 0;
        let mut y = 0.0;
        let mut max_width: f32 = 0.0;

        for (i, line_text) in line_texts.iter().enumerate() {
            let line_end = if i == line_count - 1 {
                text.len()
            } else {
                line_start + line_text.len()
            };

            // Track max width while iterating
            let line_width = line_text.chars().count() as f32 * char_width;
            max_width = max_width.max(line_width);

            lines.push(LineLayout {
                start_offset: line_start,
                end_offset: line_end,
                y,
                height: line_height,
            });

            line_start = line_end + 1; // +1 for newline
            y += line_height;
        }

        // Ensure at least one line
        if lines.is_empty() {
            lines.push(LineLayout {
                start_offset: 0,
                end_offset: 0,
                y: 0.0,
                height: line_height,
            });
        }

        Self::new(
            max_width,
            lines.len() as f32 * line_height,
            line_height,
            glyph_x_positions,
            char_to_byte,
            lines,
            text,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_monospaced_layout() {
        let layout = TextLayoutResult::monospaced("Hello", 10.0, 20.0);

        // Check positions
        assert_eq!(layout.get_cursor_x(0), 0.0); // Before 'H'
        assert_eq!(layout.get_cursor_x(5), 50.0); // After 'o'
    }

    #[test]
    fn test_get_offset_for_x() {
        let layout = TextLayoutResult::monospaced("Hello", 10.0, 20.0);

        // Click at x=25 should be closest to offset 2 or 3
        let offset = layout.get_offset_for_x(25.0);
        assert!(offset == 2 || offset == 3);
    }

    #[test]
    fn test_multiline() {
        let layout = TextLayoutResult::monospaced("Hi\nWorld", 10.0, 20.0);

        assert_eq!(layout.lines.len(), 2);
        assert_eq!(layout.lines[0].start_offset, 0);
        assert_eq!(layout.lines[1].start_offset, 3); // After "Hi\n"
    }

    #[test]
    fn test_validity() {
        let layout = TextLayoutResult::monospaced("Hello", 10.0, 20.0);

        assert!(layout.is_valid_for("Hello"));
        assert!(!layout.is_valid_for("World"));
    }
}
