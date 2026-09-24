use std::{cell::RefCell, rc::Rc};

use coroflow::{Flow, FlowExt, MainScope, StateFlow};
use cranpose_core::{
    MutableState, OwnedMutableState, State, ownedMutableStateOf, ownedMutableStateOfNeverEqual,
    remember,
};
use cranpose_services::{LifecycleState, rememberLifecycleState};

use crate::{dispatcher::require_main_dispatcher, lifecycle::is_active_for};

/// A `Copy` handle to a value remembered for one position in the
/// composition, such as a view model.
///
/// Because it is `Copy`, it moves into any number of `move` closures without a
/// `clone()`: `move || view_model.get().on_add()`. Two handles are equal when
/// they refer to the same remembered value, so a composable taking one skips
/// while the value stays the same. The value lives as long as the position
/// that remembered it.
pub struct Handle<T: 'static> {
    state: MutableState<Rc<T>>,
}

impl<T: 'static> Clone for Handle<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: 'static> Copy for Handle<T> {}

impl<T: 'static> PartialEq for Handle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.state == other.state
    }
}

impl<T: 'static> Handle<T> {
    /// The remembered value. Reading it never subscribes the caller to
    /// recomposition.
    pub fn get(&self) -> Rc<T> {
        self.state.get_non_reactive()
    }
}

/// Remembers the value `init` builds for this position in the composition and
/// returns a [`Handle`] to it.
#[track_caller]
pub fn rememberHandle<T: 'static>(init: impl FnOnce() -> T) -> Handle<T> {
    remember(|| ownedMutableStateOfNeverEqual(Rc::new(init()))).with(|owned| Handle {
        state: owned.handle(),
    })
}

/// Remembers a view model for this position in the composition — Android's
/// `viewModel { }`.
///
/// `factory` receives the view model's [`MainScope`] (its `viewModelScope`).
/// When this position leaves the composition the view model is dropped, which
/// cancels everything it launched.
#[track_caller]
pub fn rememberViewModel<VM: 'static>(factory: impl FnOnce(MainScope) -> VM) -> Handle<VM> {
    rememberHandle(|| factory(MainScope::new(require_main_dispatcher("rememberViewModel"))))
}

/// Remembers a [`MainScope`] for this position in the composition —
/// Compose's `rememberCoroutineScope()`.
///
/// Coroutines launched through it, typically from event handlers, are
/// cancelled when this position leaves the composition.
#[track_caller]
pub fn rememberCoroutineScope() -> Handle<MainScope> {
    rememberHandle(|| MainScope::new(require_main_dispatcher("rememberCoroutineScope")))
}

struct StateCollection<F: Flow<Item: Clone + 'static>> {
    flow: F,
    state: OwnedMutableState<F::Item>,
    scope: Option<MainScope>,
}

impl<F> StateCollection<F>
where
    F: Flow + Clone + 'static,
    F::Item: Clone + PartialEq + 'static,
{
    fn new(flow: &F, initial: F::Item) -> Self {
        Self {
            flow: flow.clone(),
            state: ownedMutableStateOf(initial),
            scope: None,
        }
    }

    fn set_active(&mut self, active: bool) {
        if !active {
            self.scope = None;
            return;
        }
        if self.scope.is_some() {
            return;
        }
        let scope = MainScope::new(require_main_dispatcher("collectAsState"));
        let target = self.state.handle();
        let flow = self.flow.clone();
        scope.launch(async move {
            flow.collect(move |value| target.set(value)).await;
        });
        self.scope = Some(scope);
    }
}

#[track_caller]
fn collect_state<F>(
    flow: &F,
    initial: impl FnOnce() -> F::Item,
    active: bool,
    same: impl Fn(&F, &F) -> bool,
) -> State<F::Item>
where
    F: Flow + Clone + 'static,
    F::Item: Clone + PartialEq + 'static,
{
    let holder = remember(|| RefCell::new(None::<StateCollection<F>>));
    holder.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot
            .as_ref()
            .is_some_and(|collection| !same(&collection.flow, flow))
        {
            *slot = None;
        }
        let collection = slot.get_or_insert_with(|| StateCollection::new(flow, initial()));
        collection.set_active(active);
        collection.state.as_state()
    })
}

/// Whether a lifecycle-aware collection runs in `state`: while the host is at
/// least started. Hosts that report no lifecycle stay in
/// [`LifecycleState::Created`] and keep collecting.
pub fn collects_in(state: LifecycleState) -> bool {
    is_active_for(state, LifecycleState::Started)
}

/// Reads a [`StateFlow`] as Cranpose state.
pub trait StateFlowCollect<T: Clone + 'static> {
    /// Collects the flow while this position stays in the composition and
    /// returns its latest value as [`State`] — Android's `collectAsState()`.
    ///
    /// The first composition reads the flow's current value, so there is no
    /// placeholder frame. The collection counts as a subscriber, which is what
    /// keeps a `WhileSubscribed` upstream running.
    fn collectAsState(&self) -> State<T>;

    /// Like [`collectAsState`](StateFlowCollect::collectAsState), but pauses
    /// collecting while the app is stopped — Android's
    /// `collectAsStateWithLifecycle()`. Android and iOS stop an app in the
    /// background, desktop hosts while its window is minimized or hidden, and
    /// browsers while its tab is in the background.
    ///
    /// A paused collection is not a subscriber, so `WhileSubscribed` upstreams
    /// stop after their timeout while the app sits in the background, and the
    /// state keeps the last value until the app comes back.
    fn collectAsStateWithLifecycle(&self) -> State<T>;
}

impl<T: Clone + PartialEq + 'static> StateFlowCollect<T> for StateFlow<T> {
    #[track_caller]
    fn collectAsState(&self) -> State<T> {
        collect_state(self, || self.value(), true, StateFlow::same_as)
    }

    #[track_caller]
    fn collectAsStateWithLifecycle(&self) -> State<T> {
        let lifecycle = rememberLifecycleState().get();
        collect_state(
            self,
            || self.value(),
            collects_in(lifecycle),
            StateFlow::same_as,
        )
    }
}

/// Reads any [`Flow`] as Cranpose state, starting from a given value.
///
/// Unlike a [`StateFlow`], an arbitrary flow has no current value and no
/// identity, so the state starts at `initial`, and the flow this position
/// first collected keeps running until the position leaves the composition;
/// wrap the call in a `key` to switch flows.
pub trait FlowCollect: Flow<Item: Clone + PartialEq + 'static> {
    /// Collects the flow while this position stays in the composition and
    /// returns its latest value, `initial` until the first one arrives —
    /// Compose's `Flow.collectAsState(initial)`.
    fn collectAsState(&self, initial: Self::Item) -> State<Self::Item>;

    /// Like [`collectAsState`](FlowCollect::collectAsState), but collects only
    /// while the app is not stopped, starting the flow again when it comes
    /// back — Android's `collectAsStateWithLifecycle(initialValue)`.
    fn collectAsStateWithLifecycle(&self, initial: Self::Item) -> State<Self::Item>;
}

impl<F> FlowCollect for F
where
    F: Flow + Clone + 'static,
    F::Item: Clone + PartialEq + 'static,
{
    #[track_caller]
    fn collectAsState(&self, initial: F::Item) -> State<F::Item> {
        collect_state(self, || initial, true, |_, _| true)
    }

    #[track_caller]
    fn collectAsStateWithLifecycle(&self, initial: F::Item) -> State<F::Item> {
        let lifecycle = rememberLifecycleState().get();
        collect_state(self, || initial, collects_in(lifecycle), |_, _| true)
    }
}

/// Collects `flow` on the main thread while this position stays in the
/// composition, calling `on_item` for every value — Android's
/// `LaunchedEffect(key) { flow.collect(onItem) }`.
///
/// The collection restarts when `key` changes; `flow` and `on_item` from the
/// composition that started it are the ones used.
#[track_caller]
pub fn CollectFlow<K, F, H>(key: K, flow: F, on_item: H)
where
    K: PartialEq + 'static,
    F: Flow + 'static,
    F::Run: 'static,
    H: FnMut(F::Item) + 'static,
{
    let holder = remember(|| RefCell::new(None::<(K, MainScope)>));
    holder.with(|cell| {
        let mut slot = cell.borrow_mut();
        if matches!(&*slot, Some((current, _)) if *current == key) {
            return;
        }
        *slot = None;
        let scope = MainScope::new(require_main_dispatcher("CollectFlow"));
        scope.launch(async move {
            flow.collect(on_item).await;
        });
        *slot = Some((key, scope));
    });
}
