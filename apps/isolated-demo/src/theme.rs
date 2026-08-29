use cranpose::prelude::*;
use cranpose_ui::text::{FontWeight, TextUnit};

#[derive(Clone, Copy, PartialEq)]
pub(crate) struct Palette {
    pub(crate) background: Color,
    pub(crate) surface: Color,
    pub(crate) primary: Color,
    pub(crate) on_primary: Color,
    pub(crate) text: Color,
    pub(crate) muted_text: Color,
    pub(crate) danger: Color,
}

const LIGHT_PALETTE: Palette = Palette {
    background: Color(0.96, 0.96, 0.97, 1.0),
    surface: Color(1.0, 1.0, 1.0, 1.0),
    primary: Color(0.16, 0.40, 0.85, 1.0),
    on_primary: Color(1.0, 1.0, 1.0, 1.0),
    text: Color(0.10, 0.10, 0.12, 1.0),
    muted_text: Color(0.42, 0.44, 0.48, 1.0),
    danger: Color(0.74, 0.20, 0.24, 1.0),
};

const DARK_PALETTE: Palette = Palette {
    background: Color(0.07, 0.08, 0.10, 1.0),
    surface: Color(0.14, 0.15, 0.18, 1.0),
    primary: Color(0.42, 0.62, 0.98, 1.0),
    on_primary: Color(0.05, 0.06, 0.09, 1.0),
    text: Color(0.93, 0.94, 0.96, 1.0),
    muted_text: Color(0.62, 0.64, 0.68, 1.0),
    danger: Color(0.92, 0.42, 0.46, 1.0),
};

impl Palette {
    pub(crate) fn for_mode(dark: bool) -> Self {
        if dark {
            DARK_PALETTE
        } else {
            LIGHT_PALETTE
        }
    }
}

pub(crate) fn body_text_style(color: Color) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(color),
            ..SpanStyle::default()
        },
        ..TextStyle::default()
    }
}

pub(crate) fn heading_text_style(color: Color) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(color),
            font_size: TextUnit::Sp(20.0),
            font_weight: Some(FontWeight::BOLD),
            ..SpanStyle::default()
        },
        ..TextStyle::default()
    }
}
