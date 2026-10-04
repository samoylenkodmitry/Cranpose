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

`ViewModelStoreOwner` scopes stores to a composition subtree. `viewModel(key,
factory)` resolves an instance from the nearest store, or from the composition
position outside a provided store. `ProvideViewModelStore` lets a host, preview
or test supply a store. Lifecycle start and resume effects follow host state.

## Deliberate differences

- Back-stack and view-model state live in memory. Apps save data across process
  death through `SavedStateHandle` or `rememberSaveable`.
- A covered screen keeps its view models; screen-local `remember` state follows
  composition lifetime.
- `launch_single_top` compares complete route values. Distinct arguments create
  distinct entries.
- A view model resolved outside a store follows its composition position.
