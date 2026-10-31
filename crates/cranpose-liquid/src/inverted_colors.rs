use cranpose_ui_graphics::Color;

use crate::theme::LiquidColors;

pub(crate) fn invert(mut colors: LiquidColors) -> LiquidColors {
    let LiquidColors {
        is_dark,
        label,
        secondary_label,
        tertiary_label,
        separator,
        fill,
        secondary_fill,
        surface_glass,
        background,
        surface,
        surface_pressed,
        accent,
        on_accent,
        destructive,
        success,
        toggle_on,
        toggle_off,
        warning,
        glass_tint,
        glass_stroke,
    } = &mut colors;
    *is_dark = !*is_dark;
    for color in [
        label,
        secondary_label,
        tertiary_label,
        separator,
        fill,
        secondary_fill,
        surface_glass,
        background,
        surface,
        surface_pressed,
        accent,
        on_accent,
        destructive,
        success,
        toggle_on,
        toggle_off,
        warning,
        glass_tint,
        glass_stroke,
    ] {
        *color = Color(1. - color.0, 1. - color.1, 1. - color.2, color.3);
    }
    colors
}
