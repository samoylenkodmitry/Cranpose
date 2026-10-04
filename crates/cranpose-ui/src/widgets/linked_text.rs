//! LinkedText widget — renders AnnotatedString with link annotations auto-handled.
//!
//! Mirrors the behaviour of Jetpack Compose `BasicText` / `Text` when the
//! annotated string contains `LinkAnnotation.Url` or `LinkAnnotation.Clickable`.

#![expect(non_snake_case)]

use std::rc::Rc;

use cranpose_core::NodeId;

use crate::{
    modifier::Modifier,
    text::{AnnotatedString, LinkAnnotation, TextStyle},
    widgets::ClickableText,
};

/// Displays annotated text and dispatches each link action. A URL annotation
/// calls `open_url`; a clickable annotation calls its own handler. A shared
/// [`Rc<AnnotatedString>`] retains the prepared text across uses.
///
/// # Example
///
/// ```rust
/// use cranpose_ui::{text::AnnotatedString, *};
///
/// #[composable]
/// fn WebsiteLink() {
///     let text = AnnotatedString::builder()
///         .append("Visit ")
///         .with_link(
///             LinkAnnotation::Url("https://example.com".into()),
///             |builder| builder.append("the website"),
///         )
///         .to_annotated_string();
///     LinkedText(text, Modifier::empty(), TextStyle::default(), |url| {
///         println!("Open URL: {url}");
///     });
/// }
/// ```
///
/// An app can pass a callback from the platform URI service to open the URL.
pub fn LinkedText(
    text: impl Into<Rc<AnnotatedString>>,
    modifier: Modifier,
    style: TextStyle,
    open_url: impl Fn(&str) + 'static,
) -> NodeId {
    let text = text.into();
    let text_for_links = text.clone();
    let open_url: Rc<dyn Fn(&str)> = Rc::new(open_url);
    let modifier = modifier.stable_semantics(link_actions(Rc::clone(&text), Rc::clone(&open_url)));

    ClickableText(text, modifier, style, move |offset| {
        for ann in text_for_links
            .link_annotations
            .iter()
            .filter(|a| a.range.start <= offset && offset < a.range.end)
        {
            match &ann.item {
                LinkAnnotation::Url(url) => open_url(url),
                LinkAnnotation::Clickable { handler, .. } => handler(),
            }
        }
    })
}

/// One action per link for a screen reader's actions menu, so a person who
/// cannot aim a tap at one word still opens every link the text holds.
fn link_actions(
    text: Rc<AnnotatedString>,
    open_url: Rc<dyn Fn(&str)>,
) -> impl Fn(&mut cranpose_foundation::SemanticsConfiguration) {
    move |config| {
        for link in &text.link_annotations {
            let shown = text.text.get(link.range.clone());
            let (label, action) = link_action(shown, &link.item, &open_url);
            config
                .custom_actions
                .push(cranpose_foundation::SemanticsCustomAction::new(
                    label,
                    move || action(),
                ));
        }
    }
}

fn link_action(
    shown: Option<&str>,
    link: &LinkAnnotation,
    open_url: &Rc<dyn Fn(&str)>,
) -> (String, Rc<dyn Fn()>) {
    let shown = shown.map(str::trim).filter(|shown| !shown.is_empty());
    match link {
        LinkAnnotation::Url(url) => {
            let label = format!("Open {}", shown.unwrap_or(url));
            let open_url = Rc::clone(open_url);
            let url = url.clone();
            (label, Rc::new(move || open_url(&url)))
        }
        LinkAnnotation::Clickable { tag, handler } => {
            let label = format!("Open {}", shown.unwrap_or(tag));
            (label, Rc::clone(handler))
        }
    }
}

#[cfg(test)]
#[path = "tests/linked_text_tests.rs"]
mod tests;
