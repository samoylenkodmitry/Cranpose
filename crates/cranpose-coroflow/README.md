# cranpose-coroflow

`cranpose-coroflow` connects [`coroflow`](https://docs.rs/coroflow/latest/coroflow/)
to Cranpose compositions. The crate provides main-thread dispatch, view-model
stores, lifecycle-aware collection, saved state, and flow observation.

Depend directly on `cranpose-coroflow` when a Cranpose app needs a view model,
a screen-scoped coroutine, or a flow collected as UI state. Plain async work
and streams use `coroflow` directly. The feature set is empty.

## View-model store

`ViewModelStore` keeps each model under a key and type. `NavHost` gives each
back-stack entry its own store; `ViewModelStoreOwner` gives another UI subtree
the same lifetime behavior.

```rust
use cranpose_coroflow::ViewModelStore;

struct SearchModel {
    query: String,
}

let store = ViewModelStore::default();
store.put("search", SearchModel {
    query: String::from("cranpose"),
});

let query = store
    .get::<_, SearchModel>(&"search")
    .map(|model| model.query == "cranpose");
assert_eq!(query, Some(true));
```

`viewModel(key, factory)` reads the nearest store from a composition and gives
the model a `coroflow::MainScope`. `SavedStateHandle` stores displayable values
through the Cranpose preferences service. `StateFlowCollect` and `FlowCollect`
expose flow values as Cranpose state.

Saved-state handles using the same namespace and preferences backend share one
value per key. `get`, `set`, and `get_mutable_state_flow` return a `Result`;
requesting a live key with a different Rust type returns `SavedStateError`.
Changing a returned flow or calling `set` updates every observer of that key.
Persistence happens at the next lifecycle or explicit `run_durable_saves` boundary,
without depending on recomposition. Keep a handle alive until that save completes;
keeping only a flow does not retain its persistence registration.

## Android API map

| Android API | Cranpose API |
|---|---|
| `Dispatchers.Main`, `viewModelScope` | `main_dispatcher`, `coroflow::Dispatchers::main` |
| `viewModel(key) { }` | `viewModel`, `Handle` |
| `ViewModelStore`, store owner | `ViewModelStore`, `ProvideViewModelStore`, `ViewModelStoreOwner` |
| `LifecycleStartEffect`, `LifecycleResumeEffect` | `LifecycleStartEffect`, `LifecycleResumeEffect` |
| `SavedStateHandle` | `SavedStateHandle` |
| `StateFlow.collectAsState()` | `StateFlowCollect::collectAsState` |
| `Flow.collectAsState(initial)` | `FlowCollect::collectAsState` |
| `repeatOnLifecycle`, `flowWithLifecycle` | `repeat_on_lifecycle`, `LifecycleFlowExt::flow_with_lifecycle` |

- [API reference on docs.rs](https://docs.rs/cranpose-coroflow/latest/cranpose_coroflow/)
- [Source on GitHub](https://github.com/samoylenkodmitry/cranpose/tree/main/crates/cranpose-coroflow)
