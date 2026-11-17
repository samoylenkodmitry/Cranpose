// The wrapped fields of the text input tab's touch selection demo, on a dark
// surface and on a light one, with the caret and the selection in the
// demo's pink accent.

use cranpose_core::{remember, CompositionLocalProvider};
use cranpose_foundation::text::TextFieldState;
use cranpose_ui::{composable, local_on_light_surface, text::TextUnit, Color, Modifier, TextStyle};

use crate::accent_field::AccentField;

const TEXT: &str = "Silence. Melody. Then beats. Subtle electronics                          weave through mantra trance, pulsing, catching                          melodies that rise and dissolve like glass.";
const ACCENT: Color = Color(0.965, 0.208, 0.557, 1.0);

// The surface a wrapped field lies on.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Surface {
    Dark,
    // White, where the glass ink of the selection menu flips dark.
    Light,
}

#[composable]
pub(super) fn WrappedField(surface: Surface) {
    let light = surface == Surface::Light;
    CompositionLocalProvider([local_on_light_surface().provides(light)], move || {
        let state = remember(|| TextFieldState::new(TEXT)).with(|state| *state);
        let mut style = TextStyle::default();
        style.span_style.font_size = TextUnit::Sp(16.0);
        style.paragraph_style.line_height = TextUnit::Sp(24.0);
        let (background, description) = if light {
            style.span_style.color = Some(Color(0.1, 0.1, 0.12, 1.0));
            (
                Color(1.0, 1.0, 1.0, 1.0),
                "Wrapped text field on a light surface",
            )
        } else {
            (Color(0.149, 0.129, 0.125, 1.0), "Wrapped text field")
        };
        AccentField(
            state,
            Modifier::empty()
                .fill_max_width()
                .background(background)
                .rounded_corners(10.0)
                .padding(14.0)
                .content_description(description),
            (style, ACCENT),
        );
    });
}
