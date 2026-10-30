use cranpose_core::CompositionLocalProvider;
use cranpose_macros::composable;

use crate::theme::{LiquidColors, local_liquid_colors};

/// Supplies a complete semantic palette to every Liquid control in `content`.
///
/// Typography, glass strength and accessibility services remain inherited.
/// The supplied colors are used unchanged; callers can derive them from
/// [`crate::liquid_colors`] and apply [`LiquidColors::with_more_contrast`] when
/// constructing a palette for increased contrast. Text selection surfaces use
/// the supplied palette's light or dark appearance.
#[composable]
pub fn ProvideLiquidColors(colors: LiquidColors, content: impl FnOnce()) {
    CompositionLocalProvider(
        [
            local_liquid_colors().provides(colors),
            cranpose_ui::local_on_light_surface().provides(!colors.is_dark),
        ],
        content,
    );
}
