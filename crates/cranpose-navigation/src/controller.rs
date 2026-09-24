use std::panic::Location;

use cranpose_core::{MutableState, OwnedMutableState, ownedMutableStateOfNeverEqual};
use cranpose_coroflow::{ViewModelStore, viewModel};

/// One screen on the back stack — Android's `NavBackStackEntry`.
///
/// Two entries are equal when they are the same visit, so navigating to the
/// same route twice gives two entries with their own view models.
#[derive(Clone)]
pub(crate) struct Entry<R> {
    id: u64,
    pub(crate) route: R,
    pub(crate) store: ViewModelStore,
}

impl<R> PartialEq for Entry<R> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

#[derive(Clone)]
pub(crate) struct BackStack<R> {
    entries: Vec<Entry<R>>,
    next_id: u64,
}

impl<R: Clone + PartialEq> BackStack<R> {
    fn new(start: R) -> Self {
        let mut stack = Self {
            entries: Vec::new(),
            next_id: 0,
        };
        stack.push(start);
        stack
    }

    fn push(&mut self, route: R) {
        self.entries.push(Entry {
            id: self.next_id,
            route,
            store: ViewModelStore::default(),
        });
        self.next_id += 1;
    }

    /// Removes the entries above the topmost `route`, and that entry too when
    /// `inclusive`. The removed entries are returned so that their view models
    /// are dropped after the back stack is written, not while it is borrowed.
    fn split_above(&mut self, route: &R, inclusive: bool) -> Vec<Entry<R>> {
        let Some(index) = self.entries.iter().rposition(|entry| entry.route == *route) else {
            return Vec::new();
        };
        self.entries
            .split_off(if inclusive { index } else { index + 1 })
    }

    fn top_route(&self) -> Option<&R> {
        self.entries.last().map(|entry| &entry.route)
    }

    pub(crate) fn top(&self) -> Option<Entry<R>> {
        self.entries.last().cloned()
    }

    pub(crate) fn can_pop(&self) -> bool {
        self.entries.len() > 1
    }
}

/// How [`NavController::navigate_with`] changes the back stack — Android's
/// `NavOptions`.
pub struct NavOptions<R> {
    pop_up_to: Option<(R, bool)>,
    launch_single_top: bool,
}

impl<R> Default for NavOptions<R> {
    fn default() -> Self {
        Self {
            pop_up_to: None,
            launch_single_top: false,
        }
    }
}

impl<R> NavOptions<R> {
    /// Options that change nothing: the route is pushed on top.
    pub fn new() -> Self {
        Self::default()
    }

    /// Pops the entries above the topmost `route` before navigating, and that
    /// entry too when `inclusive` — Android's `popUpTo(route) { inclusive }`.
    /// Nothing is popped when `route` is not on the back stack.
    #[must_use]
    pub fn pop_up_to(mut self, route: R, inclusive: bool) -> Self {
        self.pop_up_to = Some((route, inclusive));
        self
    }

    /// Does not push the route when it is already on top — Android's
    /// `launchSingleTop = true`.
    #[must_use]
    pub fn launch_single_top(mut self) -> Self {
        self.launch_single_top = true;
        self
    }
}

/// The back stack of a [`NavHost`](crate::NavHost) — Android's
/// `NavController`.
///
/// It is `Copy`, so it moves into any number of `move` closures:
/// `move || nav.navigate(Screen::Detail { id })`. `R` is the route type,
/// usually an enum whose variants carry the screen's arguments.
pub struct NavController<R: Clone + 'static> {
    pub(crate) stack: MutableState<BackStack<R>>,
}

impl<R: Clone + 'static> Clone for NavController<R> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<R: Clone + 'static> Copy for NavController<R> {}

impl<R: Clone + 'static> PartialEq for NavController<R> {
    fn eq(&self, other: &Self) -> bool {
        self.stack == other.stack
    }
}

impl<R: Clone + PartialEq + 'static> NavController<R> {
    /// Pushes `route` on top of the back stack — Android's `navigate(route)`.
    pub fn navigate(&self, route: R) {
        self.navigate_with(route, NavOptions::new());
    }

    /// Navigates to `route` the way `options` say — Android's
    /// `navigate(route) { popUpTo(...); launchSingleTop = true }`.
    pub fn navigate_with(&self, route: R, options: NavOptions<R>) {
        let popped = self.stack.update(|stack| {
            let popped = options
                .pop_up_to
                .map(|(target, inclusive)| stack.split_above(&target, inclusive))
                .unwrap_or_default();
            if !(options.launch_single_top && stack.top_route() == Some(&route)) {
                stack.push(route);
            }
            popped
        });
        drop(popped);
    }

    /// Pops the top entry and returns whether there was one — Android's
    /// `popBackStack()`. Popping the last entry leaves the host empty.
    pub fn pop_back_stack(&self) -> bool {
        let popped = self.stack.update(|stack| stack.entries.pop());
        popped.is_some()
    }

    /// Pops the entries above the topmost `route`, and that entry too when
    /// `inclusive`, and returns whether anything was popped — Android's
    /// `popBackStack(route, inclusive)`.
    pub fn pop_back_stack_to(&self, route: &R, inclusive: bool) -> bool {
        let popped = self
            .stack
            .update(|stack| stack.split_above(route, inclusive));
        !popped.is_empty()
    }

    /// Pops the top entry when there is one beneath it, and returns whether it
    /// did — Android's `navigateUp()`.
    pub fn navigate_up(&self) -> bool {
        let popped = self
            .stack
            .update(|stack| stack.can_pop().then(|| stack.entries.pop()).flatten());
        popped.is_some()
    }

    /// The route on top of the back stack, `None` once everything is popped.
    /// Reading it in a composable recomposes that composable when it changes —
    /// Android's `currentBackStackEntryAsState()`.
    pub fn current_route(&self) -> Option<R> {
        self.stack.read(|stack| stack.top_route().cloned())
    }

    /// Every route on the back stack, bottom first. Reading it in a
    /// composable recomposes that composable when it changes — Android's
    /// `currentBackStack`.
    pub fn back_stack(&self) -> Vec<R> {
        self.stack.read(|stack| {
            stack
                .entries
                .iter()
                .map(|entry| entry.route.clone())
                .collect()
        })
    }
}

struct NavControllerState<R: Clone + 'static> {
    stack: OwnedMutableState<BackStack<R>>,
}

/// A [`NavController`] whose back stack starts at `start` — Android's
/// `rememberNavController()` plus `NavHost(startDestination)`.
///
/// The back stack lives in the nearest view model store, so a `NavHost`
/// nested in a screen keeps its back stack while another screen covers that
/// screen.
#[track_caller]
pub fn rememberNavController<R: Clone + PartialEq + 'static>(start: R) -> NavController<R> {
    let state = viewModel(Location::caller(), |_| NavControllerState {
        stack: ownedMutableStateOfNeverEqual(BackStack::new(start)),
    });
    NavController {
        stack: state.get().stack.handle(),
    }
}
