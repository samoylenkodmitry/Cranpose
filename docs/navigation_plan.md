# Navigation and scoped view models

The framework supplies a typed back stack, `NavHost`, per-entry view model
stores, and lifecycle-aware store owners. The coroflow demo exercises the
framework APIs. Current navigation definitions live in
[`cranpose-navigation`](../crates/cranpose-navigation/src/lib.rs); view model
and lifecycle APIs live in
[`cranpose-coroflow`](../crates/cranpose-coroflow/src/lib.rs).

## Current model

Routes are Rust values with `Clone + PartialEq`. `NavHost` matches
each route in application code, so the compiler checks route selection.
`rememberNavController` stores its back stack in the nearest view model store.
Each back-stack entry owns a store for screen view models; the store drops when
its entry leaves the stack and its exit transition finishes.

Each entry also owns a `SaveableScope` named by its id, inside the controller's
scope. `rememberSaveable` in a screen saves there, so two visits of one route
keep apart and a covered screen restores its state. Popping an entry forgets its
scope: its saves stop, and the stored values go at the next save boundary.
`rememberSaveableNavController` saves the back stack as `1 <next id>` followed
by ` <id> <length> <route>` per entry, with each route in the app's `Saver`
form. On restore it keeps the entries beneath the first unreadable route and
forgets the saved state of entries that are not on the restored stack.

While an interactive back gesture moves, `NavHost` shows the entry beneath
through a `SeekableCrossfade` at the gesture's progress. A cancelled gesture
fades back from there; a completed one pops, and the crossfade finishes from
where the gesture left it, keeping the composition of the entry beneath.

`BindBrowserHistory` keeps the browser's session history in step with a back
stack: each entry it adds keeps the back stack's depth in its state and shows
the top route's saved form in the URL fragment. A push adds an entry, pops go
back with `history.go`, and any other change replaces the current entry.
`popstate` brings the user's moves back: a smaller depth pops to it, and a
larger one, or an entry with no depth such as a typed fragment, navigates to
the fragment's route. The web host implements `cranpose_services::BrowserHistory`
over `window.history`.

`ViewModelStoreOwner` scopes stores to a composition subtree. `viewModel(key,
factory)` resolves an instance from the nearest store, or from the composition
position outside a provided store. `ProvideViewModelStore` lets a host, preview
or test supply a store. Lifecycle start and resume effects follow host state.

## Deliberate differences

- `rememberNavController` keeps the back stack and its screens' saveable state
  in memory. `rememberSaveableNavController` restores both after process death.
  View models live in memory; they save data through `SavedStateHandle`, whose
  namespaces the app names.
- A covered screen keeps its view models; screen-local `remember` state follows
  composition lifetime.
- `launch_single_top` compares complete route values. Distinct arguments create
  distinct entries.
- A view model resolved outside a store follows its composition position.
