use cranpose_ui::{
    TextMeasurer, TextMetrics, TextStyle, text::AnnotatedString,
    text_layout_result::TextLayoutResult,
};

pub(crate) const CHAR_WIDTH: f32 = 6.0;
const LINE_HEIGHT: f32 = 10.0;

pub(crate) struct ContractMeasurer;

impl TextMeasurer for ContractMeasurer {
    fn measure(&self, text: &AnnotatedString, _style: &TextStyle) -> TextMetrics {
        let line_count = text.text.split('\n').count().max(1);
        let width = text
            .text
            .split('\n')
            .map(|line| line.chars().count() as f32 * CHAR_WIDTH)
            .fold(0.0_f32, f32::max);
        TextMetrics {
            width,
            height: line_count as f32 * LINE_HEIGHT,
            line_height: LINE_HEIGHT,
            line_count,
        }
    }

    fn get_offset_for_position(
        &self,
        text: &AnnotatedString,
        _style: &TextStyle,
        x: f32,
        _y: f32,
    ) -> usize {
        let char_idx = (x / CHAR_WIDTH).round().max(0.0) as usize;
        text.text
            .char_indices()
            .nth(char_idx)
            .map_or(text.text.len(), |(byte_idx, _)| byte_idx)
    }

    fn get_cursor_x_for_offset(
        &self,
        text: &AnnotatedString,
        _style: &TextStyle,
        offset: usize,
    ) -> f32 {
        text.text[..offset.min(text.text.len())].chars().count() as f32 * CHAR_WIDTH
    }

    fn layout(&self, text: &AnnotatedString, _style: &TextStyle) -> TextLayoutResult {
        TextLayoutResult::monospaced(&text.text, CHAR_WIDTH, LINE_HEIGHT)
    }

    fn choose_auto_hyphen_break(
        &self,
        _line: &str,
        _style: &TextStyle,
        _segment_start_char: usize,
        measured_break_char: usize,
    ) -> Option<usize> {
        measured_break_char.checked_sub(1)
    }
}
