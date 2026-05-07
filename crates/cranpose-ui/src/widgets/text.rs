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
use crate::text::{TextLayoutOptions, TextOptions, TextOverflow, TextStyle};
use crate::text_modifier_node::TextModifierElement;
use crate::widgets::Layout;
use cranpose_core::{MutableState, NodeId, State};
use cranpose_foundation::modifier_element;
use std::rc::Rc;

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
fn compose_basic_text_group(
    text: TextSource,
    modifier: Modifier,
    style: TextStyle,
    options: TextLayoutOptions,
) -> NodeId {
    let current = text.resolve();

    let options = options.normalized();

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
    BasicTextWithOptions(
        text,
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
    BasicTextWithOptions(value, modifier, style, TextLayoutOptions::from(options))
}

#[composable]
pub fn Text<S>(value: S, modifier: Modifier, style: TextStyle) -> NodeId
where
    S: IntoTextSource + Clone + PartialEq + 'static,
{
    TextWithOptions(value, modifier, style, TextOptions::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run_test_composition;
    use cranpose_core::{location_key, Composition, MemoryApplier};
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn basic_text_creates_node() {
        let composition = run_test_composition(|| {
            BasicTextWithOptions(
                "Hello",
                Modifier::empty(),
                TextStyle::default(),
                TextLayoutOptions::default(),
            );
        });

        assert!(composition.root().is_some());
    }

    #[test]
    fn text_with_options_creates_node() {
        let composition = run_test_composition(|| {
            TextWithOptions(
                "Hello",
                Modifier::empty(),
                TextStyle::default(),
                TextOptions {
                    overflow: TextOverflow::Ellipsis,
                    soft_wrap: false,
                    max_lines: Some(1),
                    ..TextOptions::default()
                },
            );
        });

        assert!(composition.root().is_some());
    }

    #[test]
    fn basic_text_recomposes_when_dynamic_source_changes() {
        let mut composition = Composition::new(MemoryApplier::new());
        let runtime = composition.runtime_handle();
        let state = MutableState::with_runtime("Hello".to_string(), runtime);
        let resolutions = Rc::new(Cell::new(0));

        composition
            .render(location_key(file!(), line!(), column!()), {
                let text_state = state;
                let resolutions = Rc::clone(&resolutions);
                move || {
                    let text_state = text_state;
                    let resolutions = Rc::clone(&resolutions);
                    BasicText(
                        DynamicTextSource::new(move || {
                            resolutions.set(resolutions.get() + 1);
                            Rc::new(crate::text::AnnotatedString::from(text_state.value()))
                        }),
                        Modifier::empty(),
                        TextStyle::default(),
                        TextOverflow::Clip,
                        true,
                        usize::MAX,
                        1,
                    );
                }
            })
            .expect("initial text render");

        assert_eq!(resolutions.get(), 1);

        state.set_value("World".to_string());
        composition
            .process_invalid_scopes()
            .expect("dynamic text recomposition");

        assert_eq!(resolutions.get(), 2);
    }
}
