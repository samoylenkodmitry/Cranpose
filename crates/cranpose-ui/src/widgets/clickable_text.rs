//! ClickableText widget for handling clicks on annotated text.
//!
//! Mirrors Jetpack Compose's `ClickableText` from:
//! `compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/text/ClickableText.kt`

#![expect(non_snake_case, reason = "names follow Compose's casing")]

use std::{cell::Cell, rc::Rc};

use cranpose_core::{NodeId, remember};
use cranpose_ui_graphics::Size;

use crate::{
    modifier::Modifier,
    text::{AnnotatedString, TextLayoutOptions, TextOverflow, TextStyle, measure},
    widgets::BasicTextWithOptions,
};

#[doc(hidden)]
pub trait IntoSharedAnnotatedString {
    fn into_shared(self) -> Rc<AnnotatedString>;
}

impl IntoSharedAnnotatedString for AnnotatedString {
    fn into_shared(self) -> Rc<AnnotatedString> {
        Rc::new(self)
    }
}

impl IntoSharedAnnotatedString for Rc<AnnotatedString> {
    fn into_shared(self) -> Rc<AnnotatedString> {
        self
    }
}

/// Displays annotated text and calls `on_click` with the byte offset under the pointer.
/// Use [`LinkedText`](crate::LinkedText) for URL and custom-action annotations.
///
/// # Example
///
/// ```rust
/// use cranpose_ui::{text::AnnotatedString, *};
///
/// #[composable]
/// fn OffsetLabel() {
///     ClickableText(
///         AnnotatedString::from("Click a character"),
///         Modifier::empty(),
///         TextStyle::default(),
///         |offset| println!("Byte offset: {offset}"),
///     );
/// }
/// ```
pub fn ClickableText<T>(
    text: T,
    modifier: Modifier,
    style: TextStyle,
    on_click: impl Fn(usize) + 'static,
) -> NodeId
where
    T: IntoSharedAnnotatedString,
{
    let text = text.into_shared();
    let text_for_click = Rc::clone(&text);
    let style_for_click = style.clone();
    let options = TextLayoutOptions {
        overflow: TextOverflow::Clip,
        soft_wrap: true,
        max_lines: usize::MAX,
        min_lines: 1,
    };
    // The click finds its offset in the text wrapped as the node draws it,
    // at the width of the node's content.
    let size = remember(|| Rc::new(Cell::new(Size::default()))).with(Rc::clone);
    let clickable_modifier = modifier
        .report_size(Rc::clone(&size))
        .clickable(move |point| {
            on_click(measure::offset_in_drawn_text(
                &text_for_click,
                &style_for_click,
                options,
                size.get().width,
                point,
            ));
        });

    BasicTextWithOptions(text, clickable_modifier, style, options)
}

#[cfg(test)]
#[path = "tests/clickable_text_tests.rs"]
mod tests;
