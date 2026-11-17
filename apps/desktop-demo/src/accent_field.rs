// A text field tinted with one accent, as the demo's fields and the Compose
// reference fields show it: the caret and the selection's handles in the
// accent, the selection's highlight in it at about a third.

use cranpose_core::CompositionLocalProvider;
use cranpose_foundation::text::TextFieldState;
use cranpose_ui::{
    composable, local_text_selection_colors, BasicTextFieldOptions, BasicTextFieldWithOptions,
    Color, Modifier, TextSelectionColors, TextStyle,
};

const HIGHLIGHT_ALPHA: f32 = 0.32;

#[composable(no_skip)]
pub fn AccentField(state: TextFieldState, modifier: Modifier, (style, accent): (TextStyle, Color)) {
    let colors = TextSelectionColors {
        handle_color: accent,
        background_color: accent.with_alpha(HIGHLIGHT_ALPHA),
    };
    CompositionLocalProvider(
        [local_text_selection_colors().provides(colors)],
        move || {
            BasicTextFieldWithOptions(
                state,
                modifier,
                BasicTextFieldOptions {
                    text_style: style,
                    cursor_color: accent,
                    ..BasicTextFieldOptions::default()
                },
            );
        },
    );
}
