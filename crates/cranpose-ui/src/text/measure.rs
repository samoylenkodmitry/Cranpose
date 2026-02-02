use crate::text_layout_result::TextLayoutResult;
use std::cell::RefCell;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextMetrics {
    pub width: f32,
    pub height: f32,
    /// Height of a single line of text
    pub line_height: f32,
    /// Number of lines in the text
    pub line_count: usize,
}

use super::style::TextStyle; // Add imports

pub trait TextMeasurer: 'static {
    fn measure(&self, text: &str, style: &TextStyle) -> TextMetrics;

    fn get_offset_for_position(&self, text: &str, style: &TextStyle, x: f32, y: f32) -> usize;

    fn get_cursor_x_for_offset(&self, text: &str, style: &TextStyle, offset: usize) -> f32;

    fn layout(&self, text: &str, style: &TextStyle) -> TextLayoutResult;
}

#[derive(Default)]
struct MonospacedTextMeasurer;

impl MonospacedTextMeasurer {
    const DEFAULT_SIZE: f32 = 16.0;
    const CHAR_WIDTH_RATIO: f32 = 0.6; // Width is 0.6 of Height

    fn get_metrics(style: &TextStyle) -> (f32, f32) {
        let size = if let super::unit::TextUnit::Sp(v) = style.font_size {
            v
        } else {
            Self::DEFAULT_SIZE
        };
        (size * Self::CHAR_WIDTH_RATIO, size) // (width, height)
    }
}

impl TextMeasurer for MonospacedTextMeasurer {
    fn measure(&self, text: &str, style: &TextStyle) -> TextMetrics {
        let (char_width, line_height) = Self::get_metrics(style);

        let lines: Vec<&str> = text.split('\n').collect();
        let line_count = lines.len().max(1);

        let width = lines
            .iter()
            .map(|line| line.chars().count() as f32 * char_width)
            .fold(0.0_f32, f32::max);

        TextMetrics {
            width,
            height: line_count as f32 * line_height,
            line_height,
            line_count,
        }
    }

    fn get_offset_for_position(&self, text: &str, style: &TextStyle, x: f32, y: f32) -> usize {
        let (char_width, line_height) = Self::get_metrics(style);

        if text.is_empty() {
            return 0;
        }

        let line_index = (y / line_height).floor().max(0.0) as usize;
        let lines: Vec<&str> = text.split('\n').collect();
        let target_line = line_index.min(lines.len().saturating_sub(1));

        let mut line_start_byte = 0;
        for line in lines.iter().take(target_line) {
            line_start_byte += line.len() + 1;
        }

        let line_text = lines.get(target_line).unwrap_or(&"");
        let char_index = (x / char_width).round() as usize;
        let line_char_count = line_text.chars().count();
        let clamped_index = char_index.min(line_char_count);

        let offset_in_line = line_text
            .char_indices()
            .nth(clamped_index)
            .map(|(i, _)| i)
            .unwrap_or(line_text.len());

        line_start_byte + offset_in_line
    }

    fn get_cursor_x_for_offset(&self, text: &str, style: &TextStyle, offset: usize) -> f32 {
        let (char_width, _) = Self::get_metrics(style);

        let clamped_offset = offset.min(text.len());
        let char_count = text[..clamped_offset].chars().count();
        char_count as f32 * char_width
    }

    fn layout(&self, text: &str, style: &TextStyle) -> TextLayoutResult {
        let (char_width, line_height) = Self::get_metrics(style);
        TextLayoutResult::monospaced(text, char_width, line_height)
    }
}

thread_local! {
    static TEXT_MEASURER: RefCell<Box<dyn TextMeasurer>> = RefCell::new(Box::new(MonospacedTextMeasurer));
}

pub fn set_text_measurer<M: TextMeasurer>(measurer: M) {
    TEXT_MEASURER.with(|m| {
        *m.borrow_mut() = Box::new(measurer);
    });
}

pub fn measure_text(text: &str, style: &TextStyle) -> TextMetrics {
    TEXT_MEASURER.with(|m| m.borrow().measure(text, style))
}

pub fn get_offset_for_position(text: &str, style: &TextStyle, x: f32, y: f32) -> usize {
    TEXT_MEASURER.with(|m| m.borrow().get_offset_for_position(text, style, x, y))
}

pub fn get_cursor_x_for_offset(text: &str, style: &TextStyle, offset: usize) -> f32 {
    TEXT_MEASURER.with(|m| m.borrow().get_cursor_x_for_offset(text, style, offset))
}

pub fn layout_text(text: &str, style: &TextStyle) -> TextLayoutResult {
    TEXT_MEASURER.with(|m| m.borrow().layout(text, style))
}
