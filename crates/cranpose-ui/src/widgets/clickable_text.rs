//! ClickableText widget for handling clicks on annotated text.
//!
//! Mirrors Jetpack Compose's `ClickableText` from:
//! `compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/text/ClickableText.kt`

#![expect(non_snake_case)]

use std::rc::Rc;

use cranpose_core::NodeId;

use crate::{
    modifier::Modifier,
    text::{AnnotatedString, TextOverflow, TextStyle},
    widgets::BasicText,
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
    let text_for_click = text.clone();
    let style_for_click = style.clone();
    let on_click: Rc<dyn Fn(usize)> = Rc::new(on_click);

    let clickable_modifier = modifier.clickable(move |point| {
        let offset = crate::text::get_offset_for_position(
            &text_for_click,
            &style_for_click,
            point.x,
            point.y,
        );
        on_click(offset);
    });

    BasicText(
        text,
        clickable_modifier,
        style,
        TextOverflow::Clip,
        true,
        usize::MAX,
        1,
    )
}

#[cfg(test)]
#[path = "tests/clickable_text_tests.rs"]
mod tests;
