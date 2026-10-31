use cranpose_services::{AccessibilityOptions, isSystemInDarkTheme};
use cranpose_ui_graphics::Color;

use crate::theme::{LiquidColors, SchemeMode};

pub(crate) const DEFAULT_ACCENT: Color = Color::from_rgb_u8(0, 122, 255);

#[derive(Clone, Copy, Debug, PartialEq)]
#[expect(
    clippy::large_enum_variant,
    reason = "palettes are fixed values without heap allocation"
)]
/// The source of a [`crate::LiquidTheme`]'s semantic colors.
pub enum LiquidPalette {
    /// Generate system colors from an appearance mode and an accent.
    System {
        /// Whether to follow the system or use a fixed appearance.
        scheme: SchemeMode,
        /// The interactive accent color.
        accent: Color,
    },
    /// Supply all semantic colors, including their light or dark appearance.
    ///
    /// Accessibility inversion complements RGB channels, preserves opacity and
    /// reverses `is_dark`. Increased contrast is applied after inversion.
    Custom(LiquidColors),
}

impl Default for LiquidPalette {
    fn default() -> Self {
        Self::System {
            scheme: SchemeMode::Auto,
            accent: DEFAULT_ACCENT,
        }
    }
}

impl LiquidPalette {
    pub(crate) fn colors(self, options: AccessibilityOptions) -> LiquidColors {
        let colors = match self {
            Self::System { scheme, accent } => {
                let dark = match scheme {
                    SchemeMode::Auto => isSystemInDarkTheme(),
                    SchemeMode::Light => false,
                    SchemeMode::Dark => true,
                } != options.invert_colors;
                if dark {
                    LiquidColors::dark(accent)
                } else {
                    LiquidColors::light(accent)
                }
            }
            Self::Custom(colors) if options.invert_colors => crate::inverted_colors::invert(colors),
            Self::Custom(colors) => colors,
        };
        if options.increase_contrast {
            colors.with_more_contrast()
        } else {
            colors
        }
    }
}
