use std::{cell::RefCell, rc::Rc};

use cranpose_animation::{AnimationType, Easing, tween};
use cranpose_coroflow::ProvideViewModelStore;
use cranpose_services::{BackGesture, PredictiveBackHandler, ProvideSaveableScope};
use cranpose_ui::{
    ContentSeek, SeekableCrossfade, composable,
    widgets::{Dialog, DialogSpec},
};

use crate::controller::{Entry, HostView, NavController};

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
/// back stack while there is a screen beneath the top one. While an
/// interactive back gesture moves (Android's predictive back, the iOS edge
/// swipe), the screen beneath shows through as far as the gesture went; when
/// the user lets go, the fade finishes or goes back from there. Every entry has
/// its own view model store: `viewModel` in a screen finds the screen's view
/// models, which are kept while other screens cover it and dropped once it is
/// popped and has faded out. Every entry also has its own
/// [`SaveableScope`](cranpose_services::SaveableScope), so `rememberSaveable`
/// in a screen keeps that visit's state while it is covered and forgets it
/// once it is popped.
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
pub fn NavHostWith<R, F>(controller: NavController<R>, transition: AnimationType, content: F)
where
    R: Clone + PartialEq + 'static,
    F: FnMut(R) + 'static,
{
    NavHostWithDialogs(controller, transition, |_| None, content);
}

/// [`NavHostWith`] where `dialog` gives the routes that show as dialogs
/// their [`DialogSpec`] — Compose's `dialog(route)` destinations.
///
/// A dialog entry shows in a [`Dialog`] over the screen beneath it, which
/// stays composed; dialogs above one another stack. Dismissing a dialog, by
/// the back gesture or a tap outside when its spec allows, pops it and the
/// entries above it. A dialog entry has its own view model store and saved
/// state, as a screen has.
#[composable(no_skip)]
pub fn NavHostWithDialogs<R, D, F>(
    controller: NavController<R>,
    transition: AnimationType,
    dialog: D,
    content: F,
) where
    R: Clone + PartialEq + 'static,
    D: Fn(&R) -> Option<DialogSpec> + 'static,
    F: FnMut(R) + 'static,
{
    let HostView {
        screen,
        toward,
        dialogs,
    } = controller
        .stack
        .read(|stack| stack.screens_and_dialogs(&dialog));
    let gesture = cranpose_core::rememberMutableStateOf(|| None::<f32>);
    // A dialog on top takes the back gesture itself.
    PredictiveBackHandler(
        dialogs.is_empty() && toward.is_some(),
        move |step| {
            gesture.set(match step {
                BackGesture::Started(event) | BackGesture::Progressed(event) => {
                    Some(event.progress)
                }
                BackGesture::Cancelled => None,
            });
        },
        move || {
            gesture.set(None);
            controller.navigate_up();
        },
    );
    let seek = gesture
        .get()
        .zip(toward)
        .map(|(fraction, toward)| ContentSeek {
            toward: Some(toward),
            fraction,
        });
    let content = Rc::new(RefCell::new(content));
    let shown = Rc::clone(&content);
    SeekableCrossfade(screen, seek, transition, move |entry| {
        if let Some(entry) = entry {
            compose_entry(entry, &shown);
        }
    });
    for (depth, entry, spec) in dialogs {
        let content = Rc::clone(&content);
        cranpose_core::key(entry.id(), move || {
            Dialog(
                spec,
                move |_| controller.pop_to_depth(depth),
                move || compose_entry(entry.clone(), &content),
            );
        });
    }
}

/// Composes `entry`'s route with `content`, in the entry's saved state and
/// view model store.
fn compose_entry<R, F: FnMut(R)>(entry: Entry<R>, content: &RefCell<F>) {
    let Entry {
        route,
        store,
        saveables,
        ..
    } = entry;
    ProvideSaveableScope(saveables, || {
        ProvideViewModelStore(store, || (content.borrow_mut())(route));
    });
}
