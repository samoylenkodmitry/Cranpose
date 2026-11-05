// The text style of the benchmark screens: one color and size.

use cranpose::{prelude::*, text::TextUnit};

// Text of `color`, `size` sp high.
pub(super) fn text_style(color: Color, size: f32) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(color),
            font_size: TextUnit::Sp(size),
            ..SpanStyle::default()
        },
        ..TextStyle::default()
    }
}
