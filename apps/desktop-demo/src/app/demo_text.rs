//! Text styles the demo tabs share.

use cranpose_ui::{
    text::{FontWeight, SpanStyle, TextUnit},
    Color, TextStyle,
};

/// Text of `size` sp in `color` and `font_weight`.
pub(crate) fn text_style(size: f32, color: Color, font_weight: Option<FontWeight>) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(color),
            font_size: TextUnit::Sp(size),
            font_weight,
            ..Default::default()
        },
        ..Default::default()
    }
}
