use cranpose::prelude::*;
use cranpose_ui::text::{FontWeight, TextUnit};

/// The colors every screen draws with.
#[derive(Clone, Copy, PartialEq)]
pub struct Palette {
    /// Window background.
    pub background: Color,
    /// Cards and bars.
    pub surface: Color,
    /// Raised elements on a surface.
    pub raised: Color,
    /// Accent for actions.
    pub primary: Color,
    /// Text on the accent.
    pub on_primary: Color,
    /// Body text.
    pub text: Color,
    /// Secondary text.
    pub muted: Color,
    /// Healthy status.
    pub good: Color,
    /// Errors and destructive actions.
    pub danger: Color,
}

/// The app's single palette.
pub const PALETTE: Palette = Palette {
    background: Color(0.07, 0.08, 0.10, 1.0),
    surface: Color(0.12, 0.13, 0.16, 1.0),
    raised: Color(0.18, 0.19, 0.23, 1.0),
    primary: Color(0.42, 0.62, 0.98, 1.0),
    on_primary: Color(0.05, 0.06, 0.09, 1.0),
    text: Color(0.93, 0.94, 0.96, 1.0),
    muted: Color(0.60, 0.63, 0.68, 1.0),
    good: Color(0.40, 0.82, 0.55, 1.0),
    danger: Color(0.95, 0.45, 0.48, 1.0),
};

/// Body text in `color`.
pub fn body(color: Color) -> TextStyle {
    sized(color, 14.0, None)
}

/// Small print in `color`.
pub fn caption(color: Color) -> TextStyle {
    sized(color, 12.0, None)
}

/// A section heading in `color`.
pub fn heading(color: Color) -> TextStyle {
    sized(color, 18.0, Some(FontWeight::BOLD))
}

fn sized(color: Color, size: f32, weight: Option<FontWeight>) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(color),
            font_size: TextUnit::Sp(size),
            font_weight: weight,
            ..SpanStyle::default()
        },
        ..TextStyle::default()
    }
}
