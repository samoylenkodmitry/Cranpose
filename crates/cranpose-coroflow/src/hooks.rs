use std::{cell::RefCell, rc::Rc};

use coroflow::{Flow, FlowExt, MainScope, StateFlow};
use cranpose_core::{OwnedMutableState, State, ownedMutableStateOf, remember};

use crate::dispatcher::require_main_dispatcher;

/// Remembers a view model for this position in the composition — Android's
/// `viewModel { }`.
///
/// `factory` receives the view model's [`MainScope`] (its `viewModelScope`).
/// When this position leaves the composition the view model is dropped, which
/// cancels everything it launched.
#[track_caller]
pub fn rememberViewModel<VM: 'static>(factory: impl FnOnce(MainScope) -> VM) -> Rc<VM> {
    remember(|| {
        let scope = MainScope::new(require_main_dispatcher("rememberViewModel"));
        Rc::new(factory(scope))
    })
    .with(Rc::clone)
}

struct StateCollection<T: Clone + 'static> {
    flow: StateFlow<T>,
    state: OwnedMutableState<T>,
    _scope: MainScope,
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
}

impl<T: Clone + PartialEq + 'static> StateFlowCollect<T> for StateFlow<T> {
    #[track_caller]
    fn collectAsState(&self) -> State<T> {
        let holder = remember(|| RefCell::new(None::<StateCollection<T>>));
        holder.with(|cell| {
            let mut slot = cell.borrow_mut();
            if let Some(active) = slot.as_ref().filter(|active| active.flow.same_as(self)) {
                return active.state.as_state();
            }
            *slot = None;
            let state = ownedMutableStateOf(self.value());
            let target = state.handle();
            let scope = MainScope::new(require_main_dispatcher("collectAsState"));
            let flow = self.clone();
            scope.launch(async move {
                flow.collect(move |value| target.set(value)).await;
            });
            let result = state.as_state();
            *slot = Some(StateCollection {
                flow: self.clone(),
                state,
                _scope: scope,
            });
            result
        })
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
