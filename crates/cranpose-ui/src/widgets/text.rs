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
    density::Density,
    modifier::Modifier,
    text::{TextLayoutOptions, TextOptions, TextOverflow, TextStyle},
    text_modifier_node::{TextModifierElement, TextModifierNode, shared_text_style},
    widgets::{layout::NodeModifier, nodes::LayoutNode},
};

/// Text produced by a callback that runs each time the text composes, so state read in the callback
/// recomposes the text.
#[derive(Clone)]
pub struct DynamicTextSource(Rc<dyn Fn() -> crate::text::AnnotatedString>);

impl DynamicTextSource {
    /// Creates a source whose text `resolver` returns.
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

/// Where a text widget gets its text.
#[derive(Clone, PartialEq)]
pub enum TextSource {
    /// A fixed text.
    Static(crate::text::AnnotatedString),
    /// A text produced by a callback each time the widget composes.
    Dynamic(DynamicTextSource),
}

impl TextSource {
    fn resolve(&self) -> crate::text::AnnotatedString {
        match self {
            Self::Static(text) => text.clone(),
            Self::Dynamic(dynamic) => dynamic.resolve(),
        }
    }
}

/// A fixed text as a widget was given it, which the widget compares with the
/// text its node shows without making a copy.
#[doc(hidden)]
#[derive(Clone, Copy)]
pub enum StaticText<'a> {
    /// Characters with no styles, annotations or links.
    Plain(&'a str),
    /// An annotated string.
    Annotated(&'a crate::text::AnnotatedString),
}

impl StaticText<'_> {
    /// Whether `shown` is this text.
    pub(crate) fn is(self, shown: &crate::text::AnnotatedString) -> bool {
        match self {
            Self::Plain(text) => shown.is_plain(text),
            Self::Annotated(text) => shown == text,
        }
    }
}

#[doc(hidden)]
pub trait IntoTextSource {
    fn into_text_source(self) -> TextSource;

    /// The text, when it is fixed: `None` for a text read from a state or
    /// a callback each time it composes.
    fn static_text(&self) -> Option<StaticText<'_>> {
        None
    }
}

impl IntoTextSource for String {
    fn into_text_source(self) -> TextSource {
        TextSource::Static(crate::text::AnnotatedString::from(self))
    }

    fn static_text(&self) -> Option<StaticText<'_>> {
        Some(StaticText::Plain(self))
    }
}

impl IntoTextSource for &str {
    fn into_text_source(self) -> TextSource {
        TextSource::Static(crate::text::AnnotatedString::from(self))
    }

    fn static_text(&self) -> Option<StaticText<'_>> {
        Some(StaticText::Plain(self))
    }
}

impl IntoTextSource for crate::text::AnnotatedString {
    fn into_text_source(self) -> TextSource {
        TextSource::Static(self)
    }

    fn static_text(&self) -> Option<StaticText<'_>> {
        Some(StaticText::Annotated(self))
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

// The modifier of a text's node: the caller's `modifier`, then the text's
// `element`.
fn text_node_modifier(modifier: Modifier, element: TextModifierElement) -> Modifier {
    modifier.then(Modifier::from_dyn_element(modifier_element(element)))
}

// The style and options a text lays out with: the style in the locale in
// force, where the app localizes, and the options normalized.
fn text_layout_inputs(
    style: Arc<TextStyle>,
    options: TextLayoutOptions,
) -> (Arc<TextStyle>, TextLayoutOptions) {
    #[cfg(feature = "localization")]
    let style = crate::localization::apply_shared_text_locale(style);
    (style, options.normalized())
}

// Composes a text widget: a fixed text outside a selection container
// composes in its caller's group; any other in a group of its own.
#[track_caller]
fn compose_text<S>(
    text: S,
    modifier: Modifier,
    style: Arc<TextStyle>,
    options: TextLayoutOptions,
) -> NodeId
where
    S: IntoTextSource + Clone + PartialEq + 'static,
{
    if let Some(fixed) = text.static_text()
        && crate::widgets::local_selection_registrar()
            .current()
            .is_none()
    {
        return static_text::Text(fixed, modifier, style, options);
    }
    text_group::Text(text, modifier, style, options)
}

// The modifier of a fixed text's node as a text widget sets it: the
// caller's modifier, then the text's element, made only when the node does
// not show them already.
struct StaticTextModifier<'a> {
    modifier: Modifier,
    text: StaticText<'a>,
    style: Arc<TextStyle>,
    options: TextLayoutOptions,
    density: Density,
}

impl StaticTextModifier<'_> {
    // The element of the text's node.
    fn element(&self) -> TextModifierElement {
        let text = match self.text {
            StaticText::Plain(text) => {
                crate::thread_state::with(|state| state.retired_strings.borrow_mut().plain(text))
            }
            StaticText::Annotated(text) => text.clone(),
        };
        TextModifierElement::with_shared_style(
            text,
            Arc::clone(&self.style),
            self.options,
            self.density,
        )
    }

    // Whether `node` shows the text: its chain holds the caller's modifier,
    // then a text node of this text, style and options.
    fn shown_by(&self, node: &LayoutNode) -> bool {
        node.node_after(&self.modifier).is_some_and(|last| {
            last.borrow()
                .as_any()
                .downcast_ref::<TextModifierNode>()
                .is_some_and(|last| last.shows(self.text, &self.style, self.options, self.density))
        })
    }
}

impl NodeModifier for StaticTextModifier<'_> {
    fn provided_locals(&self) -> Vec<cranpose_core::ProvidedValue> {
        self.modifier.provided_composition_locals()
    }

    fn to_modifier(&self) -> Modifier {
        text_node_modifier(self.modifier.clone(), self.element())
    }

    fn set_on(self, node: &mut LayoutNode) {
        // A node that shows another text takes this one in its text node,
        // with no modifier made for it, as a ticker's does each frame.
        if !self.shown_by(node) && !node.update_node_after(&self.modifier, self.element()) {
            node.set_modifier(self.to_modifier());
        }
    }
}

// The text widget of a fixed text, named `Text` for the source traces
// inspection shows.
mod static_text {
    use std::sync::Arc;

    use cranpose_core::NodeId;

    use super::{StaticText, StaticTextModifier, text_layout_inputs};
    use crate::{
        composable,
        modifier::Modifier,
        text::{TextLayoutOptions, TextStyle},
        widgets::layout::{LayoutEnvironment, compose_empty_layout},
    };

    // Composes in its caller's group, as an inline composable: a fixed text
    // reads no state, and its node holds what a later composition compares,
    // so it keeps no group, scope or parameters of its own.
    #[composable(no_skip)]
    pub(super) fn Text(
        text: StaticText<'_>,
        modifier: Modifier,
        style: Arc<TextStyle>,
        options: TextLayoutOptions,
    ) -> NodeId {
        let (style, options) = text_layout_inputs(style, options);
        let environment = LayoutEnvironment::current();
        compose_empty_layout(
            StaticTextModifier {
                modifier,
                text,
                style,
                options,
                density: environment.density,
            },
            environment,
        )
    }
}

// The text widget in a group of its own, named `Text` for the source traces
// inspection shows: a text read from a state or a callback, whose scope that
// read recomposes alone, or one in a selection container, which remembers
// its place in the container's selection.
mod text_group {
    use std::sync::Arc;

    use cranpose_core::NodeId;

    use super::{IntoTextSource, TextModifierElement, text_layout_inputs, text_node_modifier};
    use crate::{
        composable,
        modifier::Modifier,
        text::{TextLayoutOptions, TextStyle},
        widgets::layout::{LayoutEnvironment, compose_empty_layout},
    };

    // Its stored parameters hold the style `shared_text_style` shares with
    // equal ones, eight bytes where a style takes 320, and compare it by
    // pointer.
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
        let (style, options) = text_layout_inputs(style, options);
        let environment = LayoutEnvironment::current();
        let text = text.into_text_source().resolve();
        let modifier = match crate::widgets::local_selection_registrar().current() {
            Some(registrar) => modifier.then(crate::widgets::selection_container::selectable_text(
                registrar,
                text.clone(),
                Arc::clone(&style),
                options,
            )),
            None => modifier,
        };
        compose_empty_layout(
            text_node_modifier(
                modifier,
                TextModifierElement::with_shared_style(text, style, options, environment.density),
            ),
            environment,
        )
    }
}

/// Displays `text` in `style` with the given layout `options`. Compose's `BasicText`.
#[track_caller]
#[expect(non_snake_case, reason = "names follow Compose's casing")]
pub fn BasicTextWithOptions<S>(
    text: S,
    modifier: Modifier,
    style: TextStyle,
    options: TextLayoutOptions,
) -> NodeId
where
    S: IntoTextSource + Clone + PartialEq + 'static,
{
    compose_text(text, modifier, shared_text_style(style), options)
}

/// Displays `text` in `style` with the given overflow, wrapping and line limits. Compose's
/// `BasicText`.
#[track_caller]
#[expect(non_snake_case, reason = "names follow Compose's casing")]
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
    compose_text(
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

/// Displays `value` in `style` with the given text `options`. Compose's `Text`.
#[track_caller]
#[expect(non_snake_case, reason = "names follow Compose's casing")]
pub fn TextWithOptions<S>(
    value: S,
    modifier: Modifier,
    style: TextStyle,
    options: TextOptions,
) -> NodeId
where
    S: IntoTextSource + Clone + PartialEq + 'static,
{
    compose_text(
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
#[expect(non_snake_case, reason = "names follow Compose's casing")]
pub fn Text<S>(value: S, modifier: Modifier, style: TextStyle) -> NodeId
where
    S: IntoTextSource + Clone + PartialEq + 'static,
{
    compose_text(
        value,
        modifier,
        shared_text_style(style),
        TextLayoutOptions::from(TextOptions::default()),
    )
}

#[cfg(test)]
#[path = "tests/text_tests.rs"]
mod tests;
