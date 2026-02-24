//! LinkedText widget — renders AnnotatedString with link annotations auto-handled.
//!
//! Mirrors the behaviour of Jetpack Compose `BasicText` / `Text` when the
//! annotated string contains `LinkAnnotation.Url` or `LinkAnnotation.Clickable`.

#![allow(non_snake_case)]

use crate::modifier::Modifier;
use crate::text::{AnnotatedString, LinkAnnotation, TextStyle};
use crate::widgets::ClickableText;
use cranpose_core::NodeId;
use std::rc::Rc;

/// Renders an [`AnnotatedString`] and automatically dispatches link clicks:
///
/// - [`LinkAnnotation::Url`] → calls `open_url(url)` (platform provides the URI handler).
/// - [`LinkAnnotation::Clickable`] → calls the handler stored in the annotation.
///
/// # Example — opening a URL
///
/// ```rust,ignore
/// let uri_handler = local_uri_handler().current();
/// let text = AnnotatedString::builder()
///     .append("Visit the ")
///     .with_link(
///         LinkAnnotation::Url("https://developer.android.com/".into()),
///         |b| b.append("Android Developers"),
///     )
///     .append(" site.")
///     .to_annotated_string();
///
/// LinkedText(
///     text,
///     Modifier::empty(),
///     TextStyle::default(),
///     move |url| { uri_handler.open_uri(url).ok(); },
/// );
/// ```
///
/// # Example — custom action (`LinkAnnotation::Clickable`)
///
/// ```rust,ignore
/// let text = AnnotatedString::builder()
///     .append("Click ")
///     .with_link(
///         LinkAnnotation::Clickable {
///             tag: "action".into(),
///             handler: Rc::new(move || println!("clicked!")),
///         },
///         |b| b.append("here"),
///     )
///     .to_annotated_string();
///
/// // open_url is never called for Clickable — pass a no-op.
/// LinkedText(text, Modifier::empty(), TextStyle::default(), |_| {});
/// ```
///
/// # JC parity
///
/// Equivalent to `Text(buildAnnotatedString { withLink(LinkAnnotation.Url(…)) { … } })`.
/// The `open_url` parameter corresponds to the platform-provided `LocalUriHandler`.
#[allow(clippy::needless_pass_by_value)]
pub fn LinkedText(
    text: AnnotatedString,
    modifier: Modifier,
    style: TextStyle,
    open_url: impl Fn(&str) + 'static,
) -> NodeId {
    let link_annotations: Rc<Vec<_>> = Rc::new(text.link_annotations.clone());
    let open_url: Rc<dyn Fn(&str)> = Rc::new(open_url);

    ClickableText(text, modifier, style, move |offset| {
        for ann in link_annotations
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
