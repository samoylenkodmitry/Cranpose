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
use crate::text_modifier_node::TextModifierElement;
use crate::widgets::Layout;
use cranpose_core::{MutableState, NodeId, State};
use cranpose_foundation::modifier_element;
use std::rc::Rc;

#[derive(Clone)]
pub struct DynamicTextSource(Rc<dyn Fn() -> Rc<str>>);

impl DynamicTextSource {
    pub fn new<F>(resolver: F) -> Self
    where
        F: Fn() -> Rc<str> + 'static,
    {
        Self(Rc::new(resolver))
    }

    fn resolve(&self) -> Rc<str> {
        (self.0)()
    }
}

impl PartialEq for DynamicTextSource {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for DynamicTextSource {}

#[derive(Clone, PartialEq, Eq)]
enum TextSource {
    Static(Rc<str>),
    Dynamic(DynamicTextSource),
}

impl TextSource {
    fn resolve(&self) -> Rc<str> {
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
        TextSource::Static(Rc::from(self))
    }
}

impl IntoTextSource for &str {
    fn into_text_source(self) -> TextSource {
        TextSource::Static(Rc::from(self))
    }
}

impl<T> IntoTextSource for State<T>
where
    T: ToString + Clone + 'static,
{
    fn into_text_source(self) -> TextSource {
        let state = self;
        TextSource::Dynamic(DynamicTextSource::new(move || {
            Rc::from(state.value().to_string())
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
            Rc::from(state.value().to_string())
        }))
    }
}

impl<F> IntoTextSource for F
where
    F: Fn() -> String + 'static,
{
    fn into_text_source(self) -> TextSource {
        TextSource::Dynamic(DynamicTextSource::new(move || Rc::from(self())))
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
///   Note: Text styling (color, font size) is typically applied via the
///   `text_style` modifier (coming soon) or specific style modifiers.
///
/// # Example
///
/// ```rust,ignore
/// Text("Hello World", Modifier::padding(16.0));
/// ```
#[composable]
pub fn Text<S>(value: S, modifier: Modifier) -> NodeId
where
    S: IntoTextSource + Clone + PartialEq + 'static,
{
    let current = value.into_text_source().resolve();

    // Create a text modifier element that will add TextModifierNode to the chain
    // TextModifierNode handles measurement, drawing, and semantics
    let text_element = modifier_element(TextModifierElement::new(current));
    let final_modifier = Modifier::from_parts(vec![text_element]);
    let combined_modifier = modifier.then(final_modifier);

    // text_modifier is inclusive of layout effects
    Layout(
        combined_modifier,
        EmptyMeasurePolicy,
        || {}, // No children
    )
}
