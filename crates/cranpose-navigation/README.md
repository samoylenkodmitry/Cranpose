# cranpose-navigation

`cranpose-navigation` provides a typed back stack and a composable `NavHost`.
Routes can carry screen arguments, and each stack entry owns a view-model
store.

Depend directly on `cranpose-navigation` when a Cranpose app needs screen
navigation. The feature set is empty. The app also needs its usual
`cranpose` host and renderer features; the navigation crate brings UI,
animation, and `cranpose-coroflow` APIs as dependencies.

## Define routes and screens

`rememberNavController` stores the stack in the current composition. `NavHost`
maps each route to a screen, while a screen callback can push a typed route:

```rust
use cranpose_navigation::{NavHost, rememberNavController};
use cranpose_ui::composable;

#[derive(Clone, PartialEq)]
enum Screen {
    Home,
    Detail { id: u32 },
}

#[composable]
fn App() {
    let nav = rememberNavController(Screen::Home);
    NavHost(nav, move |screen| match screen {
        Screen::Home => HomeScreen(move || nav.navigate(Screen::Detail { id: 7 })),
        Screen::Detail { id } => DetailScreen(id),
    });
}

#[composable]
fn HomeScreen(_open_detail: impl Fn() + 'static) {}

#[composable]
fn DetailScreen(_id: u32) {}
```

`NavHost` crossfades between screens and connects platform back requests to
stack pop. Use `NavHostWith` for a custom transition and `NavController::navigate_with`
for pop-up and single-top options.

## Android API map

| Android API | Cranpose API |
|---|---|
| `rememberNavController()` and `startDestination` | `rememberNavController`, `NavController` |
| `NavHost` and typed destinations | `NavHost` with a route `match` |
| Enter and exit transitions | `NavHostWith` |
| `navigate`, `popBackStack`, `navigateUp` | `navigate`, `pop_back_stack`, `navigate_up` |
| `popUpTo`, inclusive pop, single top | `navigate_with`, `NavOptions` |
| Current back-stack entry | `current_route`, `back_stack` |
| View model scoped to a back-stack entry | `cranpose_coroflow::viewModel` inside a screen |

- [API reference on docs.rs](https://docs.rs/cranpose-navigation/latest/cranpose_navigation/)
- [Source on GitHub](https://github.com/samoylenkodmitry/cranpose/tree/main/crates/cranpose-navigation)
