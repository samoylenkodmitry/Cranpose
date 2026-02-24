//! ClickableText widget for handling clicks on annotated text.
//!
//! Mirrors Jetpack Compose's `ClickableText` from:
//! `compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/text/ClickableText.kt`

#![allow(non_snake_case)]

use crate::modifier::Modifier;
use crate::text::{AnnotatedString, TextOverflow, TextStyle};
use crate::widgets::BasicText;
use cranpose_core::NodeId;
use std::rc::Rc;

/// Displays an [`AnnotatedString`] and calls `on_click` with the **byte offset** of the character
/// under the pointer at the time of the click.
///
/// Callers typically use the offset to query string annotations:
///
/// ```rust,ignore
/// ClickableText(
///     text.clone(),
///     Modifier::empty(),
///     TextStyle::default(),
///     |offset| {
///         for ann in text.get_string_annotations("URL", offset, offset + 1) {
///             uri_handler.open_uri(&ann.item.annotation).ok();
///         }
///     },
/// );
/// ```
///
/// # JC parity
///
/// ```kotlin
/// @Composable
/// fun ClickableText(
///     text: AnnotatedString,
///     modifier: Modifier = Modifier,
///     style: TextStyle = TextStyle.Default,
///     onClick: (Int) -> Unit,
/// )
/// ```
#[allow(clippy::needless_pass_by_value)]
pub fn ClickableText(
    text: AnnotatedString,
    modifier: Modifier,
    style: TextStyle,
    on_click: impl Fn(usize) + 'static,
) -> NodeId {
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
mod tests {
    use super::*;
    use cranpose_core::{location_key, Composition, MemoryApplier};

    #[test]
    fn clickable_text_composes_without_panic() {
        let mut comp = Composition::new(MemoryApplier::new());
        comp.render(location_key(file!(), line!(), column!()), || {
            ClickableText(
                AnnotatedString::from("Hello"),
                Modifier::empty(),
                TextStyle::default(),
                |_offset| {},
            );
        })
        .expect("composition succeeds");
    }
}
