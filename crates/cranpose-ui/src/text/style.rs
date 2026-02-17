use super::decoration::{Shadow, TextDecoration};
use super::font::{FontFamily, FontStyle, FontSynthesis, FontWeight};
use super::paragraph::{Hyphens, LineBreak, TextAlign, TextDirection, TextIndent};
use super::unit::TextUnit;
use crate::modifier::Color;

#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    pub color: Option<Color>,
    pub font_size: TextUnit,
    pub font_weight: Option<FontWeight>,
    pub font_style: Option<FontStyle>,
    pub font_synthesis: Option<FontSynthesis>,
    pub font_family: Option<FontFamily>,
    pub font_feature_settings: Option<String>,
    pub letter_spacing: TextUnit,
    pub baseline_shift: Option<f32>, // TODO: Implement BaselineShift struct/enum
    pub text_geometric_transform: Option<()>, // TODO: Implement TextGeometricTransform
    pub locale_list: Option<()>,     // TODO: Implement LocaleList
    pub background: Option<Color>,
    pub text_decoration: Option<TextDecoration>,
    pub shadow: Option<Shadow>,
    pub text_align: Option<TextAlign>,
    pub text_direction: Option<TextDirection>,
    pub line_height: TextUnit,
    pub text_indent: Option<TextIndent>,
    pub line_break: LineBreak,
    pub hyphens: Hyphens,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            color: None, // Unspecified
            font_size: TextUnit::Unspecified,
            font_weight: None,
            font_style: None,
            font_synthesis: None,
            font_family: None,
            font_feature_settings: None,
            letter_spacing: TextUnit::Unspecified,
            baseline_shift: None,
            text_geometric_transform: None,
            locale_list: None,
            background: None,
            text_decoration: None,
            shadow: None,
            text_align: None,
            text_direction: None,
            line_height: TextUnit::Unspecified,
            text_indent: None,
            line_break: LineBreak::Unspecified,
            hyphens: Hyphens::Unspecified,
        }
    }
}
