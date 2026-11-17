//! The colors of selected text: Compose's `TextSelectionColors` and
//! `LocalTextSelectionColors`.

use cranpose_core::{CompositionLocal, compositionLocalOf};
use cranpose_ui_graphics::Color;

/// The colors that show selected text: the handles that drag the ends of a
/// selection, and the highlight behind the selected text. Compose's
/// `TextSelectionColors`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextSelectionColors {
    /// The color of the handles at the ends of a selection.
    pub handle_color: Color,
    /// The color drawn behind selected text.
    pub background_color: Color,
}

// The default blue: handles in it solid, the highlight at about a third.
pub(crate) const DEFAULT_TEXT_SELECTION_COLORS: TextSelectionColors = TextSelectionColors {
    handle_color: Color(0.0, 0.478, 1.0, 1.0),
    background_color: Color(0.0, 0.478, 1.0, 0.32),
};

impl Default for TextSelectionColors {
    fn default() -> Self {
        DEFAULT_TEXT_SELECTION_COLORS
    }
}

/// The selection colors that the text fields and the text in a
/// [`SelectionContainer`](crate::widgets::SelectionContainer) below show:
/// Compose's `LocalTextSelectionColors`.
///
/// ```
/// use cranpose_core::CompositionLocalProvider;
/// use cranpose_ui::{widgets::SelectionContainer, *};
///
/// #[composable]
/// fn GoldSelection() {
///     let gold = TextSelectionColors {
///         handle_color: Color(0.85, 0.65, 0.2, 1.0),
///         background_color: Color(0.85, 0.65, 0.2, 0.5),
///     };
///     CompositionLocalProvider([local_text_selection_colors().provides(gold)], || {
///         SelectionContainer(Modifier::empty(), || {
///             Text(
///                 "Selected text shows gold.",
///                 Modifier::empty(),
///                 TextStyle::default(),
///             );
///         });
///     });
/// }
/// ```
pub fn local_text_selection_colors() -> CompositionLocal<TextSelectionColors> {
    crate::environment_locals::cached_local(
        |locals| &locals.text_selection_colors,
        || compositionLocalOf(TextSelectionColors::default),
    )
}
