//! Links the application is opened with: a URL in one of its schemes that
//! another application or the system opened, and the deep link of a
//! notification the person tapped.
//!
//! A link that opened the application waits until a screen collects the
//! stream, so the launch link reaches the first collector. A navigation host
//! turns links into routes with `cranpose_navigation::BindDeepLinks`.

use cranpose_core::{EventStream, rememberEventStream};

use crate::inbox::{Inbox, InboxObserver};

static LINKS: Inbox<String> = Inbox::new("opened link");

/// Registers `observer` for opened links, replaying the ones that arrived
/// before there was anywhere to put them.
///
/// Applications collect the stream from [`rememberOpenedLinks`] instead of
/// calling this.
pub fn observe_opened_links(observer: impl Fn(String) + Send + Sync + 'static) -> InboxObserver {
    LINKS.observe(observer)
}

/// Publishes a link the application was opened with. Platform hosts call it
/// from any thread; the framework moves each link onto the UI thread before a
/// composition sees it.
pub fn publish_opened_link(link: impl Into<String>) {
    LINKS.publish(link.into());
}

/// Discards the links waiting for a collector. Used by tests and host
/// teardown.
pub fn clear_opened_links() {
    LINKS.clear();
}

/// Collects the links the application is opened with for as long as this
/// call stays in the composition — what Android's `onNewIntent` and the
/// launch intent's data give an activity.
///
/// ```rust,no_run
/// use cranpose_macros::composable;
/// use cranpose_services::rememberOpenedLinks;
///
/// #[composable]
/// fn Links() {
///     let links = rememberOpenedLinks();
///     cranpose_core::CollectEvents(links, (), |link| {
///         log::info!("opened with {link}");
///     });
/// }
/// ```
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn rememberOpenedLinks() -> EventStream<String> {
    rememberEventStream((), |sender| {
        observe_opened_links(move |link| sender.send(link))
    })
}
