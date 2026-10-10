//! Text widget implementation
//!
//! This implementation follows Jetpack Compose's BasicText architecture where text content
//! is implemented as a modifier node rather than as a measure policy. This properly separates
//! concerns: MeasurePolicy handles child layout, while TextModifierNode handles text content
//! measurement, drawing, and semantics.

use std::{rc::Rc, sync::Arc};

use cranpose_core::{MutableState, NodeId, State};
use cranpose_foundation::modifier_element;

use crate::{
    modifier::Modifier,
    text::{TextLayoutOptions, TextOptions, TextOverflow, TextStyle},
    text_modifier_node::{TextModifierElement, shared_text_style},
    widgets::layout::compose_empty_layout,
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
    style: Arc<TextStyle>,
    options: TextLayoutOptions,
) -> NodeId {
    #[cfg(feature = "localization")]
    let style = crate::localization::apply_shared_text_locale(style);
    let current = text.resolve();

    let options = options.normalized();

    let modifier = match crate::widgets::local_selection_registrar().current() {
        Some(registrar) => modifier.then(crate::widgets::selection_container::selectable_text(
            registrar,
            Rc::clone(&current),
            TextStyle::clone(&style),
            options,
        )),
        None => modifier,
    };
    let density = crate::density::density();
    let text_element = modifier_element(TextModifierElement::with_shared_style(
        current, style, options, density,
    ));
    let final_modifier = Modifier::from_parts(&[text_element]);
    let combined_modifier = modifier.then(final_modifier);

    compose_empty_layout(combined_modifier, density)
}

/// The composable every text widget calls, named `Text` for the source
/// traces inspection shows.
mod shared_style {
    use std::sync::Arc;

    use cranpose_core::NodeId;

    use super::{IntoTextSource, compose_basic_text_group};
    use crate::{
        composable,
        modifier::Modifier,
        text::{TextLayoutOptions, TextStyle},
    };

    /// Its stored parameters hold the style
    /// [`crate::text_modifier_node::shared_text_style`] shares with equal
    /// ones, eight bytes where a style takes 320, and compare it by pointer.
    #[composable]
    pub(super) fn Text<S>(
        text: S,
        modifier: Modifier,
        style: Arc<TextStyle>,
        options: TextLayoutOptions,
    ) -> NodeId
    where
        S: IntoTextSource + Clone + PartialEq + 'static,
    {
        compose_basic_text_group(text.into_text_source(), modifier, style, options)
    }
}

#[track_caller]
#[expect(non_snake_case)]
pub fn BasicTextWithOptions<S>(
    text: S,
    modifier: Modifier,
    style: TextStyle,
    options: TextLayoutOptions,
) -> NodeId
where
    S: IntoTextSource + Clone + PartialEq + 'static,
{
    shared_style::Text(text, modifier, shared_text_style(style), options)
}

#[track_caller]
#[expect(non_snake_case)]
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
    shared_style::Text(
        text,
        modifier,
        shared_text_style(style),
        TextLayoutOptions {
            overflow,
            soft_wrap,
            max_lines,
            min_lines,
        },
    )
}

#[track_caller]
#[expect(non_snake_case)]
pub fn TextWithOptions<S>(
    value: S,
    modifier: Modifier,
    style: TextStyle,
    options: TextOptions,
) -> NodeId
where
    S: IntoTextSource + Clone + PartialEq + 'static,
{
    shared_style::Text(
        value,
        modifier,
        shared_text_style(style),
        TextLayoutOptions::from(options),
    )
}

/// Displays plain or annotated text with the supplied style. The modifier
/// controls layout, appearance and input around the text.
///
/// # Example
///
/// ```rust
/// use cranpose_ui::*;
///
/// #[composable]
/// fn Greeting() {
///     Text(
///         "Hello",
///         Modifier::empty().padding(16.0),
///         TextStyle::default(),
///     );
/// }
/// ```
#[track_caller]
#[expect(non_snake_case)]
pub fn Text<S>(value: S, modifier: Modifier, style: TextStyle) -> NodeId
where
    S: IntoTextSource + Clone + PartialEq + 'static,
{
    shared_style::Text(
        value,
        modifier,
        shared_text_style(style),
        TextLayoutOptions::from(TextOptions::default()),
    )
}

#[cfg(test)]
#[path = "tests/text_tests.rs"]
mod tests;
