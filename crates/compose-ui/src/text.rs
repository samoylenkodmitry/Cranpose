use std::sync::{OnceLock, RwLock};

use crate::text_layout_result::TextLayoutResult;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextMetrics {
    pub width: f32,
    pub height: f32,
    /// Height of a single line of text
    pub line_height: f32,
    /// Number of lines in the text
    pub line_count: usize,
}

pub trait TextMeasurer: Send + Sync + 'static {
    fn measure(&self, text: &str) -> TextMetrics;

    /// Returns byte offset in text for given x position.
    /// Used for cursor positioning on click.
    ///
    /// The y parameter is for future multiline support.
    fn get_offset_for_position(&self, text: &str, x: f32, y: f32) -> usize;

    /// Returns x position for given byte offset.
    /// Used for cursor rendering and selection geometry.
    fn get_cursor_x_for_offset(&self, text: &str, offset: usize) -> f32;

    /// Computes full text layout with cached glyph positions.
    /// Returns TextLayoutResult for O(1) position lookups.
    fn layout(&self, text: &str) -> TextLayoutResult;
}

#[derive(Default)]
struct MonospacedTextMeasurer;

impl MonospacedTextMeasurer {
    const CHAR_WIDTH: f32 = 8.0;
    const LINE_HEIGHT: f32 = 20.0;
}

impl TextMeasurer for MonospacedTextMeasurer {
    fn measure(&self, text: &str) -> TextMetrics {
        // Split by newlines to handle multiline
        let lines: Vec<&str> = text.split('\n').collect();
        let line_count = lines.len().max(1);

        // Width is the max width of any line
        let width = lines
            .iter()
            .map(|line| line.chars().count() as f32 * Self::CHAR_WIDTH)
            .fold(0.0_f32, f32::max);

        TextMetrics {
            width,
            height: line_count as f32 * Self::LINE_HEIGHT,
            line_height: Self::LINE_HEIGHT,
            line_count,
        }
    }

    fn get_offset_for_position(&self, text: &str, x: f32, y: f32) -> usize {
        if text.is_empty() {
            return 0;
        }

        // Find which line was clicked based on Y coordinate
        let line_index = (y / Self::LINE_HEIGHT).floor().max(0.0) as usize;
        let lines: Vec<&str> = text.split('\n').collect();
        let target_line = line_index.min(lines.len().saturating_sub(1));

        // Calculate byte offset to start of target line
        let mut line_start_byte = 0;
        for line in lines.iter().take(target_line) {
            line_start_byte += line.len() + 1; // +1 for newline
        }

        // Find position within the line using X coordinate
        let line_text = lines.get(target_line).unwrap_or(&"");
        let char_index = (x / Self::CHAR_WIDTH).round() as usize;
        let line_char_count = line_text.chars().count();
        let clamped_index = char_index.min(line_char_count);

        // Convert character index to byte offset within the line
        let offset_in_line = line_text
            .char_indices()
            .nth(clamped_index)
            .map(|(i, _)| i)
            .unwrap_or(line_text.len());

        line_start_byte + offset_in_line
    }

    fn get_cursor_x_for_offset(&self, text: &str, offset: usize) -> f32 {
        // Count characters up to byte offset
        let clamped_offset = offset.min(text.len());
        let char_count = text[..clamped_offset].chars().count();
        char_count as f32 * Self::CHAR_WIDTH
    }

    fn layout(&self, text: &str) -> TextLayoutResult {
        TextLayoutResult::monospaced(text, Self::CHAR_WIDTH, Self::LINE_HEIGHT)
    }
}

fn global_text_measurer() -> &'static RwLock<Box<dyn TextMeasurer>> {
    static TEXT_MEASURER: OnceLock<RwLock<Box<dyn TextMeasurer>>> = OnceLock::new();
    TEXT_MEASURER.get_or_init(|| RwLock::new(Box::new(MonospacedTextMeasurer)))
}

pub fn set_text_measurer<M: TextMeasurer>(measurer: M) {
    let mut guard = global_text_measurer()
        .write()
        .expect("text measurer lock poisoned");
    *guard = Box::new(measurer);
}

pub fn measure_text(text: &str) -> TextMetrics {
    global_text_measurer()
        .read()
        .expect("text measurer lock poisoned")
        .measure(text)
}

/// Returns byte offset in text for given x position.
/// Used for cursor positioning on click.
pub fn get_offset_for_position(text: &str, x: f32, y: f32) -> usize {
    global_text_measurer()
        .read()
        .expect("text measurer lock poisoned")
        .get_offset_for_position(text, x, y)
}

/// Returns x position for given byte offset.
/// Used for cursor rendering and selection geometry.
pub fn get_cursor_x_for_offset(text: &str, offset: usize) -> f32 {
    global_text_measurer()
        .read()
        .expect("text measurer lock poisoned")
        .get_cursor_x_for_offset(text, offset)
}

/// Computes full text layout with cached glyph positions.
/// Returns TextLayoutResult for O(1) position lookups.
pub fn layout_text(text: &str) -> TextLayoutResult {
    global_text_measurer()
        .read()
        .expect("text measurer lock poisoned")
        .layout(text)
}
