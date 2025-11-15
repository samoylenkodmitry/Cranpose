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
use compose_core::{MutableState, NodeId, State};
use compose_foundation::modifier_element;
use std::rc::Rc;

#[derive(Clone)]
pub struct DynamicTextSource(Rc<dyn Fn() -> String>);

impl DynamicTextSource {
    pub fn new<F>(resolver: F) -> Self
    where
        F: Fn() -> String + 'static,
    {
        Self(Rc::new(resolver))
    }

    fn resolve(&self) -> String {
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
    Static(String),
    Dynamic(DynamicTextSource),
}

impl TextSource {
    fn resolve(&self) -> String {
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
        TextSource::Static(self)
    }
}

impl<'a> IntoTextSource for &'a str {
    fn into_text_source(self) -> TextSource {
        TextSource::Static(self.to_string())
    }
}

impl<T> IntoTextSource for State<T>
where
    T: ToString + Clone + 'static,
{
    fn into_text_source(self) -> TextSource {
        let state = self.clone();
        TextSource::Dynamic(DynamicTextSource::new(move || state.value().to_string()))
    }
}

impl<T> IntoTextSource for MutableState<T>
where
    T: ToString + Clone + 'static,
{
    fn into_text_source(self) -> TextSource {
        let state = self.clone();
        TextSource::Dynamic(DynamicTextSource::new(move || state.value().to_string()))
    }
}

impl<F> IntoTextSource for F
where
    F: Fn() -> String + 'static,
{
    fn into_text_source(self) -> TextSource {
        TextSource::Dynamic(DynamicTextSource::new(self))
    }
}

impl IntoTextSource for DynamicTextSource {
    fn into_text_source(self) -> TextSource {
        TextSource::Dynamic(self)
    }
}

/// Creates a text widget displaying the specified content.
///
/// # Architecture
///
/// Following Jetpack Compose's BasicText pattern, this implementation uses:
/// - **TextModifierElement**: Adds text content as a modifier node
/// - **EmptyMeasurePolicy**: Delegates all measurement to modifier nodes
///
/// This matches Kotlin's pattern:
/// ```kotlin
/// Layout(modifier.then(TextStringSimpleElement(...)), EmptyMeasurePolicy)
/// ```
///
/// Text content lives in the modifier node (TextModifierNode), not in the measure policy,
/// which properly separates layout policy (child arrangement) from content rendering (text).
#[composable]
pub fn Text<S>(value: S, modifier: Modifier) -> NodeId
where
    S: IntoTextSource + Clone + PartialEq + 'static,
{
    let current = value.into_text_source().resolve();

    // Create a text modifier element that will add TextModifierNode to the chain
    // TextModifierNode handles measurement, drawing, and semantics
    let text_element = modifier_element(TextModifierElement::new(current.clone()));
    let final_modifier = Modifier::from_parts(vec![text_element]);
    let combined_modifier = modifier.then(final_modifier);

    // Use EmptyMeasurePolicy - TextModifierNode handles all measurement via LayoutModifierNode::measure()
    // This matches Jetpack Compose's BasicText architecture where TextStringSimpleNode provides measurement
    Layout(
        combined_modifier,
        EmptyMeasurePolicy,
        || {}, // No children
    )
}
