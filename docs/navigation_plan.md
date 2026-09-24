# Navigation and scoped view models plan

What Cranpose needs so that an app built the way Jetpack Compose apps are, with
`NavHost`, per-screen view models and self-contained UI parts that resolve
their own view model, ports across line by line. Items are ticked as they land.

## API rules

The [coroflow rules](coroflow_plan.md#api-rules) apply: Kotlin names, no
boilerplate at the call site, Kotlin behavior unless listed under "Deliberate
differences". In addition:

- **Routes are plain Rust values.** A route is any `Clone + PartialEq` type,
  usually an enum whose variants carry the arguments:
  `Screen::Detail { id: 7 }`. The `NavHost` content is a `match`, so the
  compiler checks that every destination is handled, which is what Kotlin's
  type-safe `composable<Detail> { }` routes approximate.
- **Every handle is `Copy`.** `NavController` and view model handles move into
  any number of `move` closures without a `clone()`.
- **A view model store is a scope.** Every view model lives in the nearest
  store: a back stack entry, a `ViewModelStoreOwner` block, or, outside both,
  the position that asked for it.

## A. View model stores (`cranpose-coroflow`)

- [x] `ViewModelStore`: view models keyed by type and key; dropping the store
      drops them, which cancels their `viewModelScope`
- [x] `viewModel(key, factory)`: the nearest store's view model for `key`,
      created once. It survives recomposition and its caller leaving the
      composition, such as a list item scrolled away, for as long as the store
      lives — Android's `viewModel(key) { }`
- [x] `ViewModelStoreOwner { }`: a nested store whose view models live as long
      as the block stays in the composition, for dialogs, sheets and other
      self-contained parts — the arch starter's nested `ScreenScope`
- [x] `ProvideViewModelStore(store) { }` for hosts, previews and tests that
      seed a store with view models built from fake dependencies
- [x] `rememberViewModel` is replaced by `viewModel`

## B. Lifecycle effects (`cranpose-coroflow`)

- [x] `LifecycleStartEffect(keys) { ...; on_stop_or_dispose { } }` and
      `LifecycleResumeEffect(keys) { ...; on_pause_or_dispose { } }`, which run
      each time the host starts or resumes — the arch starter's `initOnce`
      and `onStop` hooks

## C. Navigation (`cranpose-navigation`)

- [x] `rememberNavController(start)`: a `Copy` `NavController<R>` whose back
      stack lives in the nearest view model store, so a nested `NavHost` keeps
      its back stack while its screen is covered
- [x] `navigate`, `navigate_with(route, NavOptions)` with `pop_up_to`,
      `inclusive` and `launch_single_top`, `pop_back_stack`,
      `pop_back_stack_to` and `navigate_up`
- [x] `current_route()` and `back_stack()` as observable state
- [x] `NavHost(controller, content)`: composes the top entry, crossfades
      between entries with Compose's default transition, and pops on the
      platform back gesture while there is somewhere to go back to
- [x] Each back stack entry owns a `ViewModelStore`. Covering an entry keeps
      its view models; popping it drops them once its exit transition ends
- [x] `BackHandler` moves to `cranpose-services` so that navigation does not
      depend on the platform crate; `cranpose::BackHandler` stays

## D. Templates and dogfood

- [x] The coroflow demo navigates with `NavHost`, and its note rows resolve
      their own view models
- [ ] `coroflow`, `cranpose-coroflow` and `cranpose-navigation` are published
      with the next release
- [ ] `apps/isolated-demo`, the project template, uses `NavHost` and view
      models
- [ ] `cranpose-showcase` is the template for self-contained UI parts: a card
      that resolves its own view model, shares a screen-scoped bus with its
      screen, and is tested through a store seeded with fakes
- [ ] `cranscan` navigates with `NavHost`
- [ ] `cranamp` runs its background work on coroflow

## Fixed on the way

- `Crossfade`, `AnimatedVisibility` and the Liquid menus no longer restart a
  finished animation on their first composition when given their own spec
  instead of the default one.
- `ComposeTestRule::pump_until_idle` returns while an animation waits for its
  next frame instead of recomposing until it hits the replay limit.

## Deliberate differences

- The back stack and view models do not survive the process being killed.
  Keep what must survive in a `SavedStateHandle` or `rememberSaveable`.
- A screen's `remember` state is not kept while another screen covers it;
  keep it in the screen's view model, which is.
- `launch_single_top` compares the whole route, so navigating to
  `Detail { id: 2 }` from `Detail { id: 1 }` pushes a new entry.
- A view model resolved outside any store lives as long as its position in the
  composition. Android has no such case, because an activity always provides a
  store.
