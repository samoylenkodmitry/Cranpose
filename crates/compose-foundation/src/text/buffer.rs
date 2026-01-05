//! Mutable text buffer for editing text content.
//!
//! Matches Jetpack Compose's `TextFieldBuffer` from
//! `compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/text/input/TextFieldBuffer.kt`.

use super::TextRange;

/// A mutable text buffer that can be edited.
///
/// This provides methods for changing text content:
/// - [`replace`](Self::replace) - Replace a range with new text
/// - [`append`](Self::append) - Add text at the end
/// - [`insert`](Self::insert) - Insert text at cursor position
/// - [`delete`](Self::delete) - Delete a range of text
///
/// And for manipulating cursor/selection:
/// - [`place_cursor_at_end`](Self::place_cursor_at_end)
/// - [`place_cursor_before_char`](Self::place_cursor_before_char)
/// - [`select_all`](Self::select_all)
///
/// # Example
///
/// ```
/// use compose_foundation::text::{TextFieldBuffer, TextRange};
///
/// let mut buffer = TextFieldBuffer::new("Hello");
/// buffer.place_cursor_at_end();
/// buffer.insert(", World!");
/// assert_eq!(buffer.text(), "Hello, World!");
/// ```
#[derive(Debug, Clone)]
pub struct TextFieldBuffer {
    /// The text content
    text: String,
    /// Current selection (cursor when collapsed)
    selection: TextRange,
    /// IME composition range, if any
    composition: Option<TextRange>,
    /// Track whether changes have been made
    has_changes: bool,
}

impl TextFieldBuffer {
    /// Creates a new buffer with the given initial text.
    /// Cursor is placed at the end of the text.
    pub fn new(initial_text: impl Into<String>) -> Self {
        let text: String = initial_text.into();
        let len = text.len();
        Self {
            text,
            selection: TextRange::cursor(len),
            composition: None,
            has_changes: false,
        }
    }

    /// Creates a buffer with text and specified selection.
    pub fn with_selection(text: impl Into<String>, selection: TextRange) -> Self {
        let text: String = text.into();
        let selection = selection.coerce_in(text.len());
        Self {
            text,
            selection,
            composition: None,
            has_changes: false,
        }
    }

    /// Returns the current text content.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns the length of the text in bytes.
    pub fn len(&self) -> usize {
        self.text.len()
    }

    /// Returns true if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Returns the current selection range.
    pub fn selection(&self) -> TextRange {
        self.selection
    }

    /// Returns the current composition (IME) range, if any.
    pub fn composition(&self) -> Option<TextRange> {
        self.composition
    }

    /// Returns true if there's a non-collapsed selection.
    pub fn has_selection(&self) -> bool {
        !self.selection.collapsed()
    }

    /// Returns true if any changes have been made.
    pub fn has_changes(&self) -> bool {
        self.has_changes
    }

    // ========== Text Modification ==========

    /// Replaces text in the given range with new text.
    ///
    /// The selection is adjusted based on the replacement:
    /// - If replacing before selection, selection shifts
    /// - If replacing within selection, cursor moves to end of replacement
    pub fn replace(&mut self, range: TextRange, replacement: &str) {
        let min = range.min().min(self.text.len());
        let max = range.max().min(self.text.len());

        // Perform the replacement
        self.text.replace_range(min..max, replacement);

        // Adjust selection
        let new_end = min + replacement.len();
        self.selection = TextRange::cursor(new_end);

        // Clear composition on edit
        self.composition = None;
        self.has_changes = true;
    }

    /// Inserts text at the current cursor position (or replaces selection).
    pub fn insert(&mut self, text: &str) {
        if self.has_selection() {
            // Replace selection with new text
            self.replace(self.selection, text);
        } else {
            // Insert at cursor position
            let pos = self.selection.start.min(self.text.len());
            self.text.insert_str(pos, text);
            self.selection = TextRange::cursor(pos + text.len());
            self.composition = None;
            self.has_changes = true;
        }
    }

    /// Appends text at the end of the buffer.
    pub fn append(&mut self, text: &str) {
        self.text.push_str(text);
        self.has_changes = true;
    }

    /// Deletes text in the given range.
    pub fn delete(&mut self, range: TextRange) {
        self.replace(range, "");
    }

    /// Deletes the character before the cursor (backspace).
    pub fn delete_before_cursor(&mut self) {
        if self.has_selection() {
            // Delete selection
            self.delete(self.selection);
        } else if self.selection.start > 0 {
            // Delete one character before cursor
            // Find the previous character boundary
            let pos = self.selection.start;
            let prev_pos = self.prev_char_boundary(pos);
            self.delete(TextRange::new(prev_pos, pos));
        }
    }

    /// Deletes the character after the cursor (delete key).
    pub fn delete_after_cursor(&mut self) {
        if self.has_selection() {
            self.delete(self.selection);
        } else if self.selection.start < self.text.len() {
            let pos = self.selection.start;
            let next_pos = self.next_char_boundary(pos);
            self.delete(TextRange::new(pos, next_pos));
        }
    }

    /// Deletes text surrounding the cursor or selection.
    ///
    /// `before_bytes` and `after_bytes` are byte counts in UTF-8.
    /// The deletion respects character boundaries and preserves any IME composition range.
    pub fn delete_surrounding(&mut self, before_bytes: usize, after_bytes: usize) {
        if self.text.is_empty() || (before_bytes == 0 && after_bytes == 0) {
            return;
        }

        let selection = self.selection;
        let mut start = selection.min().saturating_sub(before_bytes);
        let mut end = selection
            .max()
            .saturating_add(after_bytes)
            .min(self.text.len());

        start = self.clamp_prev_boundary(start);
        end = self.clamp_next_boundary(end);

        if start >= end {
            return;
        }

        let mut ranges = Vec::new();
        if let Some(comp) = self.composition {
            let comp_start = comp.min();
            let comp_end = comp.max();

            if end <= comp_start || start >= comp_end {
                ranges.push((start, end));
            } else {
                if start < comp_start {
                    ranges.push((start, comp_start));
                }
                if end > comp_end {
                    ranges.push((comp_end, end));
                }
            }
        } else {
            ranges.push((start, end));
        }

        if ranges.is_empty() {
            return;
        }

        ranges.sort_by_key(|(range_start, _)| *range_start);
        let total_removed: usize = ranges.iter().map(|(s, e)| e - s).sum();
        if total_removed == 0 {
            return;
        }

        let original_text = self.text.clone();
        let mut new_text = String::with_capacity(original_text.len().saturating_sub(total_removed));
        let mut last = 0usize;
        for (range_start, range_end) in &ranges {
            if last < *range_start {
                new_text.push_str(&original_text[last..*range_start]);
            }
            last = *range_end;
        }
        new_text.push_str(&original_text[last..]);

        let removed_before = |pos: usize| -> usize {
            let mut removed = 0usize;
            for (range_start, range_end) in &ranges {
                if pos <= *range_start {
                    break;
                }
                let clamped_end = pos.min(*range_end);
                if clamped_end > *range_start {
                    removed += clamped_end - *range_start;
                }
            }
            removed
        };

        let cursor_pos = selection.min();
        let new_cursor = cursor_pos
            .saturating_sub(removed_before(cursor_pos))
            .min(new_text.len());

        self.text = new_text;
        self.selection = TextRange::cursor(new_cursor);
        self.composition = self.composition.map(|comp| {
            let comp_start = comp.min().saturating_sub(removed_before(comp.min()));
            let comp_end = comp.max().saturating_sub(removed_before(comp.max()));
            TextRange::new(comp_start, comp_end).coerce_in(self.text.len())
        });
        self.has_changes = true;
    }

    /// Clears all text.
    pub fn clear(&mut self) {
        self.text.clear();
        self.selection = TextRange::zero();
        self.composition = None;
        self.has_changes = true;
    }

    // ========== Cursor/Selection Manipulation ==========

    /// Places the cursor at the end of the text.
    pub fn place_cursor_at_end(&mut self) {
        self.selection = TextRange::cursor(self.text.len());
    }

    /// Places the cursor at the start of the text.
    pub fn place_cursor_at_start(&mut self) {
        self.selection = TextRange::zero();
    }

    /// Places the cursor before the character at the given index.
    pub fn place_cursor_before_char(&mut self, index: usize) {
        let pos = index.min(self.text.len());
        self.selection = TextRange::cursor(pos);
    }

    /// Places the cursor after the character at the given index.
    pub fn place_cursor_after_char(&mut self, index: usize) {
        let pos = (index + 1).min(self.text.len());
        self.selection = TextRange::cursor(pos);
    }

    /// Selects all text.
    pub fn select_all(&mut self) {
        self.selection = TextRange::all(self.text.len());
    }

    /// Extends the selection to the left by one character.
    /// If no selection exists, starts selection from current cursor position.
    /// The anchor (end) stays fixed while the cursor (start) moves left.
    pub fn extend_selection_left(&mut self) {
        if self.selection.start > 0 {
            let new_start = self.prev_char_boundary(self.selection.start);
            self.selection = TextRange::new(new_start, self.selection.end);
        }
    }

    /// Extends the selection to the right by one character.
    /// If no selection exists, starts selection from current cursor position.
    /// The anchor (start stays at origin) while cursor (end) moves right.
    pub fn extend_selection_right(&mut self) {
        if self.selection.end < self.text.len() {
            let new_end = self.next_char_boundary(self.selection.end);
            self.selection = TextRange::new(self.selection.start, new_end);
        }
    }

    /// Selects the given range.
    pub fn select(&mut self, range: TextRange) {
        self.selection = range.coerce_in(self.text.len());
    }

    /// Sets the composition (IME) range.
    pub fn set_composition(&mut self, range: Option<TextRange>) {
        self.composition = range.map(|r| r.coerce_in(self.text.len()));
    }

    // ========== Helper Methods ==========

    /// Finds the previous character boundary from a byte index.
    fn prev_char_boundary(&self, from: usize) -> usize {
        let mut pos = from.saturating_sub(1);
        while pos > 0 && !self.text.is_char_boundary(pos) {
            pos -= 1;
        }
        pos
    }

    /// Finds the next character boundary from a byte index.
    fn next_char_boundary(&self, from: usize) -> usize {
        let mut pos = from + 1;
        while pos < self.text.len() && !self.text.is_char_boundary(pos) {
            pos += 1;
        }
        pos.min(self.text.len())
    }

    fn clamp_prev_boundary(&self, from: usize) -> usize {
        if self.text.is_char_boundary(from) {
            from
        } else {
            self.prev_char_boundary(from)
        }
    }

    fn clamp_next_boundary(&self, from: usize) -> usize {
        if self.text.is_char_boundary(from) {
            from
        } else {
            self.next_char_boundary(from)
        }
    }

    // ========== Clipboard Operations ==========
    // Note: Actual system clipboard access is handled at the platform layer (AppShell).
    // These methods just provide the text content for clipboard operations.

    /// Returns the selected text for copy operations.
    /// Returns None if no selection.
    pub fn copy_selection(&self) -> Option<String> {
        if !self.has_selection() {
            return None;
        }

        let sel_start = self.selection.min();
        let sel_end = self.selection.max();
        Some(self.text[sel_start..sel_end].to_string())
    }

    /// Cuts the selected text (returns it and deletes from buffer).
    /// Returns the cut text, or None if no selection.
    pub fn cut_selection(&mut self) -> Option<String> {
        let copied = self.copy_selection();
        if copied.is_some() {
            self.delete(self.selection);
            self.has_changes = true;
        }
        copied
    }
}

impl Default for TextFieldBuffer {
    fn default() -> Self {
        Self::new("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_buffer_has_cursor_at_end() {
        let buffer = TextFieldBuffer::new("Hello");
        assert_eq!(buffer.text(), "Hello");
        assert_eq!(buffer.selection(), TextRange::cursor(5));
    }

    #[test]
    fn insert_at_cursor() {
        let mut buffer = TextFieldBuffer::new("Hello");
        buffer.place_cursor_at_end();
        buffer.insert(", World!");
        assert_eq!(buffer.text(), "Hello, World!");
        assert_eq!(buffer.selection(), TextRange::cursor(13));
    }

    #[test]
    fn insert_in_middle() {
        let mut buffer = TextFieldBuffer::new("Helo");
        buffer.place_cursor_before_char(2);
        buffer.insert("l");
        assert_eq!(buffer.text(), "Hello");
    }

    #[test]
    fn delete_selection() {
        let mut buffer = TextFieldBuffer::new("Hello World");
        buffer.select(TextRange::new(5, 11)); // " World"
        buffer.delete(buffer.selection());
        assert_eq!(buffer.text(), "Hello");
    }

    #[test]
    fn delete_before_cursor() {
        let mut buffer = TextFieldBuffer::new("Hello");
        buffer.place_cursor_at_end();
        buffer.delete_before_cursor();
        assert_eq!(buffer.text(), "Hell");
    }

    #[test]
    fn select_all() {
        let mut buffer = TextFieldBuffer::new("Hello");
        buffer.select_all();
        assert_eq!(buffer.selection(), TextRange::new(0, 5));
    }

    #[test]
    fn replace_selection() {
        let mut buffer = TextFieldBuffer::new("Hello World");
        buffer.select(TextRange::new(6, 11)); // "World"
        buffer.insert("Rust");
        assert_eq!(buffer.text(), "Hello Rust");
    }

    #[test]
    fn clear_buffer() {
        let mut buffer = TextFieldBuffer::new("Hello");
        buffer.clear();
        assert!(buffer.is_empty());
        assert_eq!(buffer.selection(), TextRange::zero());
    }

    #[test]
    fn unicode_handling() {
        let mut buffer = TextFieldBuffer::new("Hello 🌍");
        buffer.place_cursor_at_end();
        buffer.delete_before_cursor();
        assert_eq!(buffer.text(), "Hello ");
    }

    #[test]
    fn delete_surrounding_collapsed_cursor() {
        let mut buffer = TextFieldBuffer::new("abcdef");
        buffer.place_cursor_before_char(3); // cursor between c and d
        buffer.delete_surrounding(2, 2); // remove b..e
        assert_eq!(buffer.text(), "af");
        assert_eq!(buffer.selection(), TextRange::cursor(1));
    }

    #[test]
    fn delete_surrounding_preserves_composition() {
        let mut buffer = TextFieldBuffer::new("abcdef");
        buffer.place_cursor_before_char(3);
        buffer.set_composition(Some(TextRange::new(2, 4))); // "cd"
        buffer.delete_surrounding(3, 3);
        assert_eq!(buffer.text(), "cd");
        assert_eq!(buffer.selection(), TextRange::cursor(1));
        assert_eq!(buffer.composition(), Some(TextRange::new(0, 2)));
    }
}
