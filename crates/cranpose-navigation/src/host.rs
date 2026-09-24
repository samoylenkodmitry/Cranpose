use cranpose_animation::{AnimationType, Easing, tween};
use cranpose_coroflow::ProvideViewModelStore;
use cranpose_services::BackHandler;
use cranpose_ui::{Crossfade, composable};

use crate::controller::NavController;

/// How long Compose's `NavHost` fades between destinations.
const DEFAULT_TRANSITION_MILLIS: u64 = 700;

/// Shows the screen on top of `controller`'s back stack — Android's
/// `NavHost`.
///
/// `content` receives the route and composes its screen, usually with a
/// `match`, so the compiler checks that every route has one:
///
/// ```
/// use cranpose_navigation::{NavHost, rememberNavController};
///
/// #[derive(Clone, PartialEq)]
/// enum Screen {
///     Home,
///     Detail { id: u32 },
/// }
///
/// # #[allow(non_snake_case)]
/// fn App() {
///     let nav = rememberNavController(Screen::Home);
///     NavHost(nav, move |screen| match screen {
///         Screen::Home => HomeScreen(move |id| nav.navigate(Screen::Detail { id })),
///         Screen::Detail { id } => DetailScreen(id),
///     });
/// }
/// # #[allow(non_snake_case)]
/// # fn HomeScreen(_open: impl Fn(u32) + 'static) {}
/// # #[allow(non_snake_case)]
/// # fn DetailScreen(_id: u32) {}
/// ```
///
/// Screens crossfade the way Compose's do. The platform back gesture pops the
/// back stack while there is a screen beneath the top one. Every entry has
/// its own view model store: `viewModel` in a screen finds the screen's view
/// models, which are kept while other screens cover it and dropped once it is
/// popped and has faded out.
#[composable(no_skip)]
pub fn NavHost<R, F>(controller: NavController<R>, content: F)
where
    R: Clone + PartialEq + 'static,
    F: FnMut(R) + 'static,
{
    NavHostWith(
        controller,
        tween(DEFAULT_TRANSITION_MILLIS, Easing::FastOutSlowInEasing),
        content,
    );
}

/// [`NavHost`] with its own `transition` between screens — Compose's
/// `NavHost(enterTransition, exitTransition)` for a fade.
#[composable(no_skip)]
pub fn NavHostWith<R, F>(controller: NavController<R>, transition: AnimationType, mut content: F)
where
    R: Clone + PartialEq + 'static,
    F: FnMut(R) + 'static,
{
    let (top, can_pop) = controller
        .stack
        .read(|stack| (stack.top(), stack.can_pop()));
    BackHandler(can_pop, move || {
        controller.navigate_up();
    });
    Crossfade(top, transition, move |entry| {
        if let Some(entry) = entry {
            let route = entry.route;
            ProvideViewModelStore(entry.store, || content(route));
        }
    });
}
