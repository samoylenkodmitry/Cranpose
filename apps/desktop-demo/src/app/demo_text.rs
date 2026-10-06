//! Text styles the demo tabs share.

use cranpose_ui::{
    text::{FontWeight, SpanStyle, TextUnit},
    Color, TextStyle,
};

/// Text of `size` sp in `color`, bold when asked.
pub(crate) fn text_style(size: f32, color: Color, bold: bool) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(color),
            font_size: TextUnit::Sp(size),
            font_weight: bold.then_some(FontWeight::BOLD),
            ..Default::default()
        },
        ..Default::default()
    }
}
