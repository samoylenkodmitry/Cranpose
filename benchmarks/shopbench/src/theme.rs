// The design tokens of `spec/design/screens/Tokens.dc.html`: colors, the type
// scale and the image widths the API is asked for.

use cranpose::{
    Color, ParagraphStyle, SpanStyle, TextOptions, TextOverflow, TextStyle,
    text::{FontWeight, TextAlign, TextDecoration, TextUnit},
};

use crate::model::ImageRef;

pub(crate) const BACKGROUND: Color = Color::from_rgb_u8(0xFF, 0xFF, 0xFF);
pub(crate) const SURFACE: Color = Color::from_rgb_u8(0xF4, 0xF1, 0xEC);
pub(crate) const SKELETON: Color = Color::from_rgb_u8(0xED, 0xE9, 0xE3);
pub(crate) const BORDER: Color = Color::from_rgb_u8(0xE6, 0xE1, 0xD9);
pub(crate) const TEXT: Color = Color::from_rgb_u8(0x1C, 0x1B, 0x19);
pub(crate) const TEXT_MUTED: Color = Color::from_rgb_u8(0x6E, 0x6A, 0x63);
pub(crate) const TEXT_BODY: Color = Color::from_rgb_u8(0x3D, 0x3A, 0x35);
pub(crate) const SALE: Color = Color::from_rgb_u8(0xB4, 0x36, 0x1E);
pub(crate) const WHITE: Color = Color::from_rgb_u8(0xFF, 0xFF, 0xFF);
// The featured banner's caption band: `text` at 72 %.
pub(crate) const SCRIM: Color = Color::from_rgba_u8(0x1C, 0x1B, 0x19, 0xB8);

pub(crate) const PADDING: f32 = 16.0;
pub(crate) const BAR_HEIGHT: f32 = 44.0;
pub(crate) const TAB_BAR_HEIGHT: f32 = 49.0;
pub(crate) const CARD_WIDTH: f32 = 140.0;
pub(crate) const GRID_COLUMN_GAP: f32 = 12.0;
pub(crate) const GRID_ROW_GAP: f32 = 20.0;

// The `width` each place asks the image server for.
#[derive(Clone, Copy)]
pub(crate) enum ImageWidth {
    Banner = 1080,
    Card = 480,
    Thumbnail = 240,
}

pub(crate) fn image_url(image: Option<&ImageRef>, width: ImageWidth) -> Option<String> {
    let url = &image?.url;
    let separator = if url.contains('?') { '&' } else { '?' };
    Some(format!("{url}{separator}width={}", width as u32))
}

pub(crate) fn style(size: f32, weight: FontWeight, color: Color) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(color),
            font_size: TextUnit::Sp(size),
            font_weight: Some(weight),
            ..Default::default()
        },
        ..Default::default()
    }
}

// A style for text that wraps, with the line height of the type scale.
pub(crate) fn paragraph(
    size: f32,
    line_height: f32,
    weight: FontWeight,
    color: Color,
) -> TextStyle {
    TextStyle {
        paragraph_style: ParagraphStyle {
            line_height: TextUnit::Sp(line_height),
            ..Default::default()
        },
        ..style(size, weight, color)
    }
}

pub(crate) fn struck(size: f32, color: Color) -> TextStyle {
    let mut style = style(size, FontWeight::NORMAL, color);
    style.span_style.text_decoration = Some(TextDecoration::LINE_THROUGH);
    style
}

pub(crate) fn letter_spaced(style: TextStyle, spacing: f32) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            letter_spacing: TextUnit::Sp(spacing),
            ..style.span_style
        },
        ..style
    }
}

pub(crate) fn centered(style: TextStyle) -> TextStyle {
    TextStyle {
        paragraph_style: ParagraphStyle {
            text_align: TextAlign::Center,
            ..style.paragraph_style
        },
        ..style
    }
}

pub(crate) fn lines(max: usize) -> TextOptions {
    TextOptions {
        overflow: TextOverflow::Ellipsis,
        max_lines: Some(max),
        ..TextOptions::default()
    }
}
