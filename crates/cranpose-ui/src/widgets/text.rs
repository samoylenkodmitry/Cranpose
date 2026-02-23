//! Text widget implementation
//!
//! This implementation follows Jetpack Compose's BasicText architecture where text content
//! is implemented as a modifier node rather than as a measure policy. This properly separates
//! concerns: MeasurePolicy handles child layout, while TextModifierNode handles text content
//! measurement, drawing, and semantics.

#![allow(non_snake_case)]

use crate::composable;
use crate::layout::policies::EmptyMeasurePolicy;
use crate::modifier::Modifier;
use crate::text::{TextLayoutOptions, TextOverflow, TextStyle};
use crate::text_modifier_node::TextModifierElement;
use crate::widgets::Layout;
use cranpose_core::{MutableState, NodeId, State};
use cranpose_foundation::modifier_element;
use std::rc::Rc; // Added Rc import

#[derive(Clone)]
pub struct DynamicTextSource(Rc<dyn Fn() -> crate::text::AnnotatedString>);

impl DynamicTextSource {
    pub fn new<F>(resolver: F) -> Self
    where
        F: Fn() -> crate::text::AnnotatedString + 'static,
    {
        Self(Rc::new(resolver))
    }

    fn resolve(&self) -> crate::text::AnnotatedString {
        (self.0)()
    }
}

impl PartialEq for DynamicTextSource {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Clone, PartialEq)]
enum TextSource {
    Static(crate::text::AnnotatedString),
    Dynamic(DynamicTextSource),
}

impl TextSource {
    fn resolve(&self) -> crate::text::AnnotatedString {
        match self {
            TextSource::Static(text) => text.clone(),
            TextSource::Dynamic(dynamic) => dynamic.resolve(),
        }
    }
}

trait IntoTextSource {
    fn into_text_source(self) -> TextSource;
}

impl IntoTextSource for String {
    fn into_text_source(self) -> TextSource {
        TextSource::Static(crate::text::AnnotatedString::from(self))
    }
}

impl IntoTextSource for &str {
    fn into_text_source(self) -> TextSource {
        TextSource::Static(crate::text::AnnotatedString::from(self))
    }
}

impl IntoTextSource for crate::text::AnnotatedString {
    fn into_text_source(self) -> TextSource {
        TextSource::Static(self)
    }
}

impl<T> IntoTextSource for State<T>
where
    T: ToString + Clone + 'static,
{
    fn into_text_source(self) -> TextSource {
        let state = self;
        TextSource::Dynamic(DynamicTextSource::new(move || {
            crate::text::AnnotatedString::from(state.value().to_string())
        }))
    }
}

impl<T> IntoTextSource for MutableState<T>
where
    T: ToString + Clone + 'static,
{
    fn into_text_source(self) -> TextSource {
        let state = self;
        TextSource::Dynamic(DynamicTextSource::new(move || {
            crate::text::AnnotatedString::from(state.value().to_string())
        }))
    }
}

impl<F> IntoTextSource for F
where
    F: Fn() -> String + 'static,
{
    fn into_text_source(self) -> TextSource {
        TextSource::Dynamic(DynamicTextSource::new(move || {
            crate::text::AnnotatedString::from(self())
        }))
    }
}

impl IntoTextSource for DynamicTextSource {
    fn into_text_source(self) -> TextSource {
        TextSource::Dynamic(self)
    }
}

/// High-level element that displays text.
///
/// # When to use
/// Use this widget to display read-only text on the screen. For editable text,
/// use [`BasicTextField`](crate::widgets::BasicTextField).
///
/// # Arguments
///
/// * `value` - The string to display. Can be a `&str`, `String`, or `State<String>`.
/// * `modifier` - Modifiers to apply (e.g., padding, background, layout instructions).
/// * `style` - Text styling (color, font size).
///
/// # Example
///
/// ```rust,ignore
/// Text("Hello World", Modifier::padding(16.0), TextStyle::default());
/// ```
#[composable]
pub fn BasicText<S>(
    text: S,
    modifier: Modifier,
    style: TextStyle,
    overflow: TextOverflow,
    soft_wrap: bool,
    max_lines: usize,
    min_lines: usize,
) -> NodeId
where
    S: IntoTextSource + Clone + PartialEq + 'static,
{
    let current = text.into_text_source().resolve();

    let options = TextLayoutOptions {
        overflow,
        soft_wrap,
        max_lines,
        min_lines,
    }
    .normalized();

    // Create a text modifier element that will add TextModifierNode to the chain
    // TextModifierNode handles measurement, drawing, and semantics
    let text_element = modifier_element(TextModifierElement::new(current, style, options));
    let final_modifier = Modifier::from_parts(vec![text_element]);
    let combined_modifier = modifier.then(final_modifier);

    // text_modifier is inclusive of layout effects
    Layout(
        combined_modifier,
        EmptyMeasurePolicy,
        || {}, // No children
    )
}

#[composable]
pub fn Text<S>(value: S, modifier: Modifier, style: TextStyle) -> NodeId
where
    S: IntoTextSource + Clone + PartialEq + 'static,
{
    BasicText(
        value,
        modifier,
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
    use crate::run_test_composition;

    #[test]
    fn basic_text_creates_node() {
        let composition = run_test_composition(|| {
            BasicText(
                "Hello",
                Modifier::empty(),
                TextStyle::default(),
                TextOverflow::Clip,
                true,
                usize::MAX,
                1,
            );
        });

        assert!(composition.root().is_some());
    }
}
