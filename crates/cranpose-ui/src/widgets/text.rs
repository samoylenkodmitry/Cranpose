//! Text widget implementation
//!
//! This implementation follows Jetpack Compose's BasicText architecture where text content
//! is implemented as a modifier node rather than as a measure policy. This properly separates
//! concerns: MeasurePolicy handles child layout, while TextModifierNode handles text content
//! measurement, drawing, and semantics.

use std::rc::Rc;

use cranpose_core::{MutableState, NodeId, State};
use cranpose_foundation::modifier_element;

use crate::{
    composable,
    layout::policies::EmptyMeasurePolicy,
    modifier::Modifier,
    text::{TextLayoutOptions, TextOptions, TextOverflow, TextStyle},
    text_modifier_node::TextModifierElement,
    widgets::layout::compose_layout,
};

#[derive(Clone)]
pub struct DynamicTextSource(Rc<dyn Fn() -> Rc<crate::text::AnnotatedString>>);

impl DynamicTextSource {
    pub fn new<F>(resolver: F) -> Self
    where
        F: Fn() -> Rc<crate::text::AnnotatedString> + 'static,
    {
        Self(Rc::new(resolver))
    }

    fn resolve(&self) -> Rc<crate::text::AnnotatedString> {
        (self.0)()
    }
}

impl PartialEq for DynamicTextSource {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Clone, PartialEq)]
pub enum TextSource {
    Static(Rc<crate::text::AnnotatedString>),
    Dynamic(DynamicTextSource),
}

impl TextSource {
    fn resolve(&self) -> Rc<crate::text::AnnotatedString> {
        match self {
            TextSource::Static(text) => text.clone(),
            TextSource::Dynamic(dynamic) => dynamic.resolve(),
        }
    }
}

#[doc(hidden)]
pub trait IntoTextSource {
    fn into_text_source(self) -> TextSource;
}

impl IntoTextSource for String {
    fn into_text_source(self) -> TextSource {
        TextSource::Static(Rc::new(crate::text::AnnotatedString::from(self)))
    }
}

impl IntoTextSource for &str {
    fn into_text_source(self) -> TextSource {
        TextSource::Static(Rc::new(crate::text::AnnotatedString::from(self)))
    }
}

impl IntoTextSource for crate::text::AnnotatedString {
    fn into_text_source(self) -> TextSource {
        TextSource::Static(Rc::new(self))
    }
}

impl IntoTextSource for Rc<crate::text::AnnotatedString> {
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
            Rc::new(crate::text::AnnotatedString::from(
                state.value().to_string(),
            ))
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
            Rc::new(crate::text::AnnotatedString::from(
                state.value().to_string(),
            ))
        }))
    }
}

impl<F> IntoTextSource for F
where
    F: Fn() -> String + 'static,
{
    fn into_text_source(self) -> TextSource {
        TextSource::Dynamic(DynamicTextSource::new(move || {
            Rc::new(crate::text::AnnotatedString::from(self()))
        }))
    }
}

impl IntoTextSource for DynamicTextSource {
    fn into_text_source(self) -> TextSource {
        TextSource::Dynamic(self)
    }
}

fn compose_basic_text_group(
    text: TextSource,
    modifier: Modifier,
    style: TextStyle,
    options: TextLayoutOptions,
) -> NodeId {
    let current = text.resolve();

    let options = options.normalized();

    let text_element = modifier_element(TextModifierElement::new(current, style, options));
    let final_modifier = Modifier::from_parts(vec![text_element]);
    let combined_modifier = modifier.then(final_modifier);

    compose_layout(combined_modifier, EmptyMeasurePolicy, || {})
}

#[composable]
pub fn BasicTextWithOptions<S>(
    text: S,
    modifier: Modifier,
    style: TextStyle,
    options: TextLayoutOptions,
) -> NodeId
where
    S: IntoTextSource + Clone + PartialEq + 'static,
{
    compose_basic_text_group(text.into_text_source(), modifier, style, options)
}

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
    compose_basic_text_group(
        text.into_text_source(),
        modifier,
        style,
        TextLayoutOptions {
            overflow,
            soft_wrap,
            max_lines,
            min_lines,
        },
    )
}

#[composable]
pub fn TextWithOptions<S>(
    value: S,
    modifier: Modifier,
    style: TextStyle,
    options: TextOptions,
) -> NodeId
where
    S: IntoTextSource + Clone + PartialEq + 'static,
{
    compose_basic_text_group(
        value.into_text_source(),
        modifier,
        style,
        TextLayoutOptions::from(options),
    )
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
pub fn Text<S>(value: S, modifier: Modifier, style: TextStyle) -> NodeId
where
    S: IntoTextSource + Clone + PartialEq + 'static,
{
    compose_basic_text_group(
        value.into_text_source(),
        modifier,
        style,
        TextLayoutOptions::from(TextOptions::default()),
    )
}

#[cfg(test)]
#[path = "tests/text_tests.rs"]
mod tests;
