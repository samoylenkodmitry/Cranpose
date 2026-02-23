use cranpose_ui::text::{Hyphens, TextUnit};
use cranpose_ui::text_layout_result::TextLayoutResult;
use cranpose_ui::{
    prepare_text_layout, set_text_measurer, ParagraphStyle, SpanStyle, TextLayoutOptions,
    TextMeasurer, TextMetrics, TextOverflow, TextStyle,
};

struct ContractMeasurer;

impl TextMeasurer for ContractMeasurer {
    fn measure(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        _style: &TextStyle,
    ) -> TextMetrics {
        let line_count = text.text.split('\n').count().max(1);
        let width = text
            .text
            .split('\n')
            .map(|line| line.chars().count() as f32 * 6.0)
            .fold(0.0_f32, f32::max);
        TextMetrics {
            width,
            height: line_count as f32 * 10.0,
            line_height: 10.0,
            line_count,
        }
    }

    fn get_offset_for_position(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        _style: &TextStyle,
        x: f32,
        _y: f32,
    ) -> usize {
        let char_idx = (x / 6.0).round().max(0.0) as usize;
        text.text
            .char_indices()
            .nth(char_idx)
            .map(|(byte_idx, _)| byte_idx)
            .unwrap_or(text.text.len())
    }

    fn get_cursor_x_for_offset(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        _style: &TextStyle,
        offset: usize,
    ) -> f32 {
        text.text[..offset.min(text.text.len())].chars().count() as f32 * 6.0
    }

    fn layout(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        _style: &TextStyle,
    ) -> TextLayoutResult {
        TextLayoutResult::monospaced(&text.text, 6.0, 10.0)
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

#[test]
fn prepare_text_layout_uses_measurer_hyphen_contract() {
    set_text_measurer(ContractMeasurer);

    let style = TextStyle {
        span_style: SpanStyle {
            font_size: TextUnit::Sp(10.0),
            ..Default::default()
        },
        paragraph_style: ParagraphStyle {
            hyphens: Hyphens::Auto,
            ..Default::default()
        },
    };
    let options = TextLayoutOptions {
        overflow: TextOverflow::Clip,
        soft_wrap: true,
        max_lines: usize::MAX,
        min_lines: 1,
    };

    let prepared = prepare_text_layout(
        &cranpose_ui::text::AnnotatedString::from("Transformation"),
        &style,
        options,
        Some(24.0),
    );
    assert_eq!(
        prepared.text.text.lines().collect::<Vec<_>>(),
        vec!["Tra", "nsf", "orm", "ati", "on"]
    );
}
