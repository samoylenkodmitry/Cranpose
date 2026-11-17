// The tab's two typefaces: Caprasimo for headlines and Figtree for text.
// The demo registers both (see `fonts.rs`).

use cranpose_ui::{
    text::{FontFamily, FontWeight, TextUnit},
    Color, TextStyle,
};

pub(super) const DISPLAY_FAMILY: &str = "Caprasimo";
pub(super) const TEXT_FAMILY: &str = "Figtree Medium";
pub(super) const STRONG_FAMILY: &str = "Figtree";

fn style(family: &str, weight: FontWeight, size: f32, line: f32, color: Color) -> TextStyle {
    let mut style = TextStyle::default();
    style.span_style.font_family = Some(FontFamily::named(family));
    style.span_style.font_weight = Some(weight);
    style.span_style.font_size = TextUnit::Sp(size);
    style.span_style.color = Some(color);
    style.paragraph_style.line_height = TextUnit::Sp(size * line);
    style
}

// A headline in Caprasimo, set tight.
pub(super) fn headline(size: f32, color: Color) -> TextStyle {
    style(DISPLAY_FAMILY, FontWeight::NORMAL, size, 1.02, color)
}

// Caprasimo at `size` with lines `line` sizes apart.
pub(super) fn display(size: f32, line: f32, color: Color) -> TextStyle {
    style(DISPLAY_FAMILY, FontWeight::NORMAL, size, line, color)
}

// Running text in Figtree Medium.
pub(super) fn text(size: f32, color: Color) -> TextStyle {
    style(TEXT_FAMILY, FontWeight::MEDIUM, size, 1.4, color)
}

// Bold text in Figtree Bold.
pub(super) fn strong(size: f32, color: Color) -> TextStyle {
    style(STRONG_FAMILY, FontWeight::BOLD, size, 1.35, color)
}
