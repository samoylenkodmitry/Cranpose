---
name: cranpose
description: Cranpose UI framework conventions, APIs, and patterns. Use when writing Cranpose composables, state, layout, or modifiers, or when working in a project that depends on the cranpose crates.
user-invocable: true
---

# Cranpose

Cranpose is a declarative UI framework for Rust modelled on Jetpack Compose:
`#[composable]` functions, a slot table with fine-grained recomposition,
snapshot state, and modifier-chain layout. One codebase targets desktop,
Android (including Wear OS), iOS, and the web through WebAssembly, rendering
through wgpu everywhere.

General rules:
- No unnecessary comments. Good names beat narration.
- No em dashes.
- Keep it simple and concrete.

## The two things that surprise people first

**Composables are CamelCase functions.** That is deliberate -- it matches
Jetpack Compose, and it is why every Cranpose file starts with:

```rust
#![allow(non_snake_case)] // #[composable] functions are CamelCase

use cranpose::prelude::*;
```

**Argument order is `(modifier, spec, content)`,** with the modifier first and
the content closure last:

```rust
Column(
    Modifier::empty().fill_max_size().padding(24.0),
    ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(12.0)),
    move || {
        Text("Hello", Modifier::empty(), TextStyle::default());
    },
);
```

`Text` is the exception: its value comes first, then the modifier, then the
style -- `Text(value, modifier, style)`.

## State

`rememberMutableStateOf` creates state that survives recomposition. Read it
with `.value()` or `.get()`, write with `.set()` or `.update()`:

```rust
#[composable]
fn Counter() {
    let count = rememberMutableStateOf(|| 0i32);

    Button(
        Modifier::empty().padding(10.0),
        ButtonSpec::default(),
        move || count.set(count.get() + 1),
        move || {
            Text(format!("Count: {}", count.get()), Modifier::empty(), TextStyle::default());
        },
    );
}
```

State handles are `Copy`. Move them into closures directly; do not `.clone()`
them.

| Call | Use it for |
| --- | --- |
| `remember(\|\| ...)` | A value computed once, not observed for changes. |
| `rememberMutableStateOf(\|\| ...)` | Observable state. Reading it subscribes the composable. |
| `rememberUpdatedState(value)` | A value a long-lived effect should see fresh without restarting. |
| `rememberCoroutineScope()` | A scope for work started from an event handler. |
| `mutableStateOf(...)` | State owned outside the composition. |
| `SnapshotStateList` / `SnapshotStateMap` | Collections where element changes should invalidate readers. |

## Effects

`LaunchedEffect` takes a key expression first, like `remember`. The effect
restarts when the key changes:

```rust
LaunchedEffect(request_id.get(), move |scope| {
    state.set(Loading);
    scope.launch_background(/* ... */);
});
```

Use `DisposableEffect` when something must be undone on teardown, and
`SideEffect` to publish composition results to non-Cranpose code.

`key(keys, || { ... })` scopes composition identity to `keys` instead of just
call-site position: changing `keys` discards the block's remembered state and
starts over, and it separates two blocks at the same call site (a loop body,
a branch) that would otherwise share one.

## Lists

A `for` loop composes every item. For anything long, use `LazyColumn` (or
`LazyRow`), which composes only what is on screen. It takes a state handle as
its second argument, and its content closure receives a scope:

```rust
let list_state = rememberLazyListState();

LazyColumn(
    Modifier::empty().fill_max_size(),
    list_state,
    LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(4.0)),
    move |scope| {
        scope.items(
            LazyItems::new(count).content_type(|index: usize| (index % 5) as u64),
            move |index| {
                Text(format!("Row {index}"), Modifier::empty(), TextStyle::default());
            },
        );
    },
);
```

`content_type` lets the runtime reuse subtrees between items of the same shape.
Give it a real grouping when items differ structurally.

## Modifiers

A `Modifier` is an ordered chain, and order matters -- `.padding(8.0).background(c)`
paints the background inside the padding, `.background(c).padding(8.0)` outside
it. Build with `Modifier::empty()` and chain:

```rust
Modifier::empty()
    .fill_max_width()
    .height(140.0)
    .padding(8.0)
    .background(Color(0.3, 0.3, 0.4, 0.4))
    .rounded_corners(8.0)
    .clickable(move |_| { /* ... */ })
```

Common ones: `fill_max_size`, `fill_max_width`, `width`, `height`, `size`,
`weight`, `padding`, `offset`, `background`, `rounded_corners`, `alpha`,
`scale`, `shadow`, `clickable`, and the `on_*` pointer, focus and scroll
handlers.

## Layout

| Composable | Shape |
| --- | --- |
| `Column` / `Row` | Linear stacks. Spacing via `LinearArrangement`. |
| `Box` | Overlay children in the same space. |
| `Scaffold` / `ScreenScaffold` | Screen skeletons. |
| `BoxWithConstraints` | Children that depend on the space available. |
| `SubcomposeLayout` | Composition driven by measurement. |
| `Spacer` | Fixed or weighted gap. |

`cranpose-liquid` adds the first-party glass component library: `LiquidCard`,
`GlassButton`, `LiquidNavBar`, `LiquidTabBar`, `LiquidSlider`,
`LiquidSegmentedControl`, `LiquidToggle`, and friends. Reach for those before
hand-building a glass surface.

## Launching

```rust
fn main() {
    AppLauncher::new()
        .with_title("Todo")
        .with_size(420, 560)
        .try_run(TodoApp)
        .expect("launch the app");
}
```

## Getting started in a new project

Copy `apps/isolated-demo` from the repository. It is a complete starter that
depends only on published crates and targets desktop, Android and web. Starting
from scratch means reinventing the platform entry points it already has.

```toml
[dependencies]
cranpose = { version = "0.1", features = ["desktop", "renderer-wgpu"] }
```

## Pitfalls

- **Reading state outside a composable does not subscribe anything.** If a
  change does not repaint, check that the read happens inside the composable
  that should react to it.
- **A `for` loop over a long collection composes all of it.** Use `LazyColumn`.
- **Do not `.clone()` state handles.** They are `Copy`.
- **`LaunchedEffect` needs a key.** Passing a key that changes every
  composition restarts the effect every frame.
- **Cranpose is pre-alpha.** Versions are not compatible with each other and
  APIs change without deprecation cycles. Pin an exact version.
