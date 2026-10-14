use cranpose_core::CollectEvents;
use cranpose_services::rememberOpenedLinks;

use crate::controller::NavController;

/// Shows the route that `parse` reads from each link the app is opened with
/// — Android's deep links, `navDeepLink` with `handleDeepLink`.
///
/// The links are the ones `cranpose_services::rememberOpenedLinks` collects:
/// a URL in one of the schemes that the app's `cranpose_capabilities`
/// declaration names with `linking`, and the deep link of a tapped
/// notification. The back stack becomes its start entry with the link's
/// route above it, as Android builds the back stack of a deep link, so back
/// returns to the start; a link to the start route leaves the start alone. A
/// link that `parse` gives `None` for changes nothing. The link that launched
/// the app waits until this is composed.
#[track_caller]
pub fn BindDeepLinks<R, P>(controller: NavController<R>, parse: P)
where
    R: Clone + PartialEq + 'static,
    P: Fn(&str) -> Option<R> + 'static,
{
    let links = rememberOpenedLinks();
    CollectEvents(links, controller, move |link: String| {
        if let Some(route) = parse(&link) {
            controller.open_deep_link(route);
        }
    });
}
