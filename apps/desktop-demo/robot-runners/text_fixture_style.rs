use cranpose::Color;
use cranpose_ui::text::{TextStyle, TextUnit};

pub(crate) const WINDOW_WIDTH: u32 = 460;
pub(crate) const WINDOW_HEIGHT: u32 = 340;
pub(crate) const BACKDROP: Color = Color(0.149, 0.129, 0.125, 1.0);
pub(crate) const TEXT_COLOR: Color = Color(0.94, 0.92, 0.90, 1.0);
pub(crate) const ACCENT: Color = Color(0.965, 0.208, 0.557, 1.0);
pub(crate) const FIELD_X: f32 = 20.0;
pub(crate) const FIELD_Y: f32 = 170.0;
pub(crate) const FIELD_WIDTH: f32 = 420.0;

pub(crate) fn text_style() -> TextStyle {
    let mut style = TextStyle::default();
    style.span_style.color = Some(TEXT_COLOR);
    style.span_style.font_size = TextUnit::Sp(16.0);
    style.paragraph_style.line_height = TextUnit::Sp(24.0);
    style
}
