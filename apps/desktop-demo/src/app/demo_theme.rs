//! Colours the demo tabs take from the system theme.

use cranpose_core::remember;
use cranpose_services::isSystemInDarkTheme;
use cranpose_ui::{
    composable, Color, Column, ColumnSpec, LinearArrangement, Modifier, ScrollState,
};

/// The surface and text colours of one theme.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DemoPalette {
    /// The page behind everything.
    pub(crate) background: Color,
    /// A card on the page.
    pub(crate) surface: Color,
    /// A well set into a card.
    pub(crate) track: Color,
    /// Titles and body text.
    pub(crate) text: Color,
    /// Text of lower weight.
    pub(crate) muted: Color,
    /// Counters and other confirmations.
    pub(crate) positive: Color,
}

impl DemoPalette {
    pub(crate) fn new(dark: bool) -> Self {
        if dark {
            Self {
                background: Color::from_rgb_u8(0x11, 0x14, 0x1B),
                surface: Color::from_rgb_u8(0x1C, 0x21, 0x2B),
                track: Color::from_rgb_u8(0x2A, 0x31, 0x40),
                text: Color::from_rgb_u8(0xE5, 0xE7, 0xEB),
                muted: Color::from_rgb_u8(0x9C, 0xA3, 0xAF),
                positive: Color::from_rgb_u8(0x4A, 0xDE, 0x80),
            }
        } else {
            Self {
                background: Color::from_rgb_u8(0xF3, 0xF4, 0xF6),
                surface: Color::WHITE,
                track: Color::from_rgb_u8(0xE5, 0xE7, 0xEB),
                text: Color::from_rgb_u8(0x11, 0x18, 0x27),
                muted: Color::from_rgb_u8(0x6B, 0x72, 0x80),
                positive: Color::from_rgb_u8(0x16, 0xA3, 0x4A),
            }
        }
    }

    /// The palette of the system theme. Composition that reads it
    /// recomposes when the theme changes.
    pub(crate) fn current() -> Self {
        Self::new(isSystemInDarkTheme())
    }
}

/// A tab page: `palette`'s background over the whole tab, with `content`
/// scrolling inside it, `padding` from the edges and `spacing` between rows.
#[composable]
pub(crate) fn ThemedPage(
    palette: DemoPalette,
    padding: f32,
    spacing: f32,
    content: impl FnMut() + 'static,
) {
    let scroll = remember(|| ScrollState::new(0.0)).with(|state| *state);
    Column(
        Modifier::empty()
            .fill_max_size()
            .background(palette.background)
            .vertical_scroll(scroll, false)
            .padding(padding),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(spacing)),
        content,
    );
}
