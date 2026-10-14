use std::panic::Location;

use cranpose_core::{MutableState, OwnedMutableState, ownedMutableStateOfNeverEqual};
use cranpose_coroflow::{ViewModelStore, viewModel};
use cranpose_services::{SaveableRegistration, SaveableScope, Saver};

mod saved_stack;

/// One screen on the back stack — Android's `NavBackStackEntry`.
///
/// Two entries are equal when they are the same visit, so navigating to the
/// same route twice gives two entries with their own view models and saved
/// state.
#[derive(Clone)]
pub(crate) struct Entry<R> {
    id: u64,
    pub(crate) route: R,
    pub(crate) store: ViewModelStore,
    pub(crate) saveables: SaveableScope,
}

impl<R> Entry<R> {
    fn new(id: u64, route: R, stack: &SaveableScope) -> Self {
        Self {
            id,
            route,
            store: ViewModelStore::default(),
            saveables: stack.child(id),
        }
    }
}

impl<R> PartialEq for Entry<R> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

#[derive(Clone)]
pub(crate) struct BackStack<R> {
    entries: Vec<Entry<R>>,
    /// The segments that pops with [`NavOptions::save_state`] kept, each
    /// bottom first, for [`NavOptions::restore_state`] to bring back.
    saved: Vec<Vec<Entry<R>>>,
    next_id: u64,
    /// Where the entries' saved state lives, each entry in a child scope
    /// named by its id.
    saveables: SaveableScope,
}

impl<R: Clone + PartialEq> BackStack<R> {
    fn new(start: R, saveables: SaveableScope) -> Self {
        let mut stack = Self {
            entries: Vec::new(),
            saved: Vec::new(),
            next_id: 0,
            saveables,
        };
        stack.push(start);
        stack
    }

    fn push(&mut self, route: R) {
        self.entries
            .push(Entry::new(self.next_id, route, &self.saveables));
        self.next_id += 1;
    }

    /// Removes the entries above the topmost `route`, and that entry too when
    /// `inclusive`. The removed entries are returned so that their view models
    /// are dropped after the back stack is written, not while it is borrowed.
    /// [`forget`] forgets their saved state.
    fn split_above(&mut self, route: &R, inclusive: bool) -> Vec<Entry<R>> {
        let Some(index) = self.entries.iter().rposition(|entry| entry.route == *route) else {
            return Vec::new();
        };
        self.entries
            .split_off(if inclusive { index } else { index + 1 })
    }

    /// Keeps `popped` for a later [`NavOptions::restore_state`], in place of
    /// the segments kept before that hold one of its routes. Those are off
    /// screen, so their view models are dropped here; they are returned for
    /// [`forget`] to forget their saved state.
    fn keep(&mut self, popped: Vec<Entry<R>>) -> Vec<Entry<R>> {
        let (replaced, kept) = std::mem::take(&mut self.saved)
            .into_iter()
            .partition::<Vec<_>, _>(|segment| {
                segment
                    .iter()
                    .any(|entry| popped.iter().any(|new| new.route == entry.route))
            });
        self.saved = kept;
        self.saved.push(popped);
        let replaced: Vec<Entry<R>> = replaced.into_iter().flatten().collect();
        for entry in &replaced {
            entry.store.clear();
        }
        replaced
    }

    /// Takes the kept segment that holds `route`.
    fn take_saved(&mut self, route: &R) -> Option<Vec<Entry<R>>> {
        let index = self
            .saved
            .iter()
            .position(|segment| segment.iter().any(|entry| entry.route == *route))?;
        Some(self.saved.swap_remove(index))
    }

    fn top_route(&self) -> Option<&R> {
        self.entries.last().map(|entry| &entry.route)
    }

    pub(crate) fn top(&self) -> Option<Entry<R>> {
        self.entries.last().cloned()
    }

    /// The entry a pop would show.
    pub(crate) fn beneath_top(&self) -> Option<Entry<R>> {
        let index = self.entries.len().checked_sub(2)?;
        self.entries.get(index).cloned()
    }

    pub(crate) fn can_pop(&self) -> bool {
        self.entries.len() > 1
    }
}

/// Forgets the saved state of `popped` entries, and returns whether there
/// was one. Their view models drop once the host has faded them out.
fn forget<R>(popped: impl IntoIterator<Item = Entry<R>>) -> bool {
    let mut any = false;
    for entry in popped {
        entry.saveables.forget();
        any = true;
    }
    any
}

/// How [`NavController::navigate_with`] changes the back stack — Android's
/// `NavOptions`.
pub struct NavOptions<R> {
    pop_up_to: Option<(R, bool)>,
    launch_single_top: bool,
    save_state: bool,
    restore_state: bool,
}

impl<R> Default for NavOptions<R> {
    fn default() -> Self {
        Self {
            pop_up_to: None,
            launch_single_top: false,
            save_state: false,
            restore_state: false,
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

    /// Keeps the entries that [`Self::pop_up_to`] pops, with their view
    /// models and saved state, for a later navigation with
    /// [`Self::restore_state`] to bring back — Android's
    /// `popUpTo(route) { saveState = true }`. A tab bar uses it to give each
    /// tab its own back stack.
    #[must_use]
    pub fn save_state(mut self) -> Self {
        self.save_state = true;
        self
    }

    /// Brings back the entries a [`Self::save_state`] pop kept for the route,
    /// bottom first, in place of a new entry for it — Android's
    /// `restoreState = true`. Without such entries the route is pushed as
    /// usual.
    #[must_use]
    pub fn restore_state(mut self) -> Self {
        self.restore_state = true;
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
        let forgotten = self.stack.update(|stack| {
            let popped = options
                .pop_up_to
                .map(|(target, inclusive)| stack.split_above(&target, inclusive))
                .unwrap_or_default();
            let forgotten = if options.save_state && !popped.is_empty() {
                stack.keep(popped)
            } else {
                popped
            };
            let restored = options
                .restore_state
                .then(|| stack.take_saved(&route))
                .flatten();
            if let Some(restored) = restored {
                stack.entries.extend(restored);
            } else if !(options.launch_single_top && stack.top_route() == Some(&route)) {
                stack.push(route);
            }
            forgotten
        });
        forget(forgotten);
    }

    /// Pops the top entry and returns whether there was one — Android's
    /// `popBackStack()`. Popping the last entry leaves the host empty.
    pub fn pop_back_stack(&self) -> bool {
        forget(self.stack.update(|stack| stack.entries.pop()))
    }

    /// Pops the entries above the topmost `route`, and that entry too when
    /// `inclusive`, and returns whether anything was popped — Android's
    /// `popBackStack(route, inclusive)`.
    pub fn pop_back_stack_to(&self, route: &R, inclusive: bool) -> bool {
        forget(
            self.stack
                .update(|stack| stack.split_above(route, inclusive)),
        )
    }

    /// Pops the entries above the first `depth`.
    pub(crate) fn pop_to_depth(&self, depth: usize) {
        forget(self.stack.update(|stack| {
            let keep = depth.min(stack.entries.len());
            stack.entries.split_off(keep)
        }));
    }

    /// Pops the top entry when there is one beneath it, and returns whether it
    /// did — Android's `navigateUp()`.
    pub fn navigate_up(&self) -> bool {
        forget(
            self.stack
                .update(|stack| stack.can_pop().then(|| stack.entries.pop()).flatten()),
        )
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
    /// Declared first so that it drops first and reads the back stack one
    /// last time.
    _saved: Option<SaveableRegistration>,
    stack: OwnedMutableState<BackStack<R>>,
}

impl<R: Clone + 'static> NavControllerState<R> {
    fn controller(&self) -> NavController<R> {
        NavController {
            stack: self.stack.handle(),
        }
    }
}

/// A [`NavController`] whose back stack starts at `start` — Android's
/// `rememberNavController()` plus `NavHost(startDestination)`.
///
/// The back stack lives in the nearest view model store, so a `NavHost`
/// nested in a screen keeps its back stack while another screen covers that
/// screen. It lives only in memory, and so does the `rememberSaveable` state
/// of its screens: a covered screen gets its state back, and a popped one
/// forgets it. [`rememberSaveableNavController`] also restores both after the
/// process restarts.
#[track_caller]
pub fn rememberNavController<R: Clone + PartialEq + 'static>(start: R) -> NavController<R> {
    let location = Location::caller();
    let state = viewModel(location, |_| {
        let saveables = SaveableScope::current().child(location).in_memory();
        saveables.forget();
        NavControllerState {
            _saved: None,
            stack: ownedMutableStateOfNeverEqual(BackStack::new(start, saveables)),
        }
    });
    state.get().controller()
}

/// A [`NavController`] whose back stack is saved under `key` at each save
/// boundary, as `rememberSaveable` state is, and restored when it is next
/// created, after a process restart too. `saver` gives each route its saved
/// form; it may read the forms older builds wrote.
///
/// The `rememberSaveable` state of each screen is saved with it, apart from
/// other visits of the same route. A popped screen forgets its state.
/// Restoring keeps the entries beneath the first route `saver` cannot read;
/// when it cannot read the bottom one, or nothing was saved, the back stack
/// starts at `start`.
#[track_caller]
pub fn rememberSaveableNavController<R: Clone + PartialEq + 'static>(
    key: &'static str,
    start: R,
    saver: Saver<R>,
) -> NavController<R> {
    let state = viewModel(Location::caller(), |_| {
        let scope = SaveableScope::current();
        let saveables = scope.child(key);
        let restored = scope
            .restore(key)
            .and_then(|saved| saved_stack::decode(&saved, &saver, &saveables));
        let stack = match restored {
            Some(stack) => {
                saveables.forget_children_except(
                    stack
                        .entries
                        .iter()
                        .chain(stack.saved.iter().flatten())
                        .map(|entry| entry.id),
                );
                stack
            }
            None => {
                saveables.forget();
                BackStack::new(start, saveables)
            }
        };
        let stack = ownedMutableStateOfNeverEqual(stack);
        let handle = stack.handle();
        let saved = scope.register(key, move || {
            handle.try_with(|stack| saved_stack::encode(stack, &saver))
        });
        NavControllerState {
            _saved: Some(saved),
            stack,
        }
    });
    state.get().controller()
}
