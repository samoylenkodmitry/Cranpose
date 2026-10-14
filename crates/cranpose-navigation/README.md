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

## Saved state

Each back-stack entry has its own `SaveableScope`, so `rememberSaveable` in a
screen keeps the state of that visit. A covered screen gets its state back, and
a popped screen forgets it. Two visits of one route keep apart.

`rememberNavController` keeps the back stack and the screens' saved state in
memory. `rememberSaveableNavController` saves both at each save boundary and
restores them when the app starts again. Its `Saver` gives each route a saved
form and may read the forms that older builds wrote:

```rust
use cranpose_navigation::{NavHost, rememberSaveableNavController};
use cranpose_services::Saver;
use cranpose_ui::composable;

#[derive(Clone, PartialEq)]
enum Screen {
    Home,
    Detail { id: u32 },
}

fn routes() -> Saver<Screen> {
    Saver::new(
        |screen: &Screen| match screen {
            Screen::Home => "home".to_owned(),
            Screen::Detail { id } => format!("detail {id}"),
        },
        |saved| match saved {
            "home" => Some(Screen::Home),
            _ => saved.strip_prefix("detail ")?.parse().ok().map(|id| Screen::Detail { id }),
        },
    )
}

#[composable]
fn App() {
    let nav = rememberSaveableNavController("nav", Screen::Home, routes());
    NavHost(nav, move |screen| match screen {
        Screen::Home => {}
        Screen::Detail { id: _ } => {}
    });
}
```

Restoring keeps the entries beneath the first route the saver cannot read. When
it cannot read the bottom entry, or nothing was saved, the back stack starts at
the start route.

## Results

A screen returns a value to the screen beneath it through that screen's
back-stack entry, as Android's `savedStateHandle` does.
`previous_back_stack_entry` gives the entry beneath the top, and `set_result`
leaves the value there. In the screen beneath, `composing_entry` gives the
screen's own entry, and `result` reads the value and recomposes the reader when
it changes. `take_result` removes the value once the screen has acted on it:

```rust
use cranpose_navigation::{NavHost, rememberNavController};
use cranpose_ui::composable;

#[derive(Clone, PartialEq)]
enum Screen {
    Home,
    PickColor,
}

#[derive(Clone)]
struct Picked(u32);

#[composable]
fn App() {
    let nav = rememberNavController(Screen::Home);
    NavHost(nav, move |screen| match screen {
        Screen::Home => {
            let picked = nav.composing_entry().and_then(|entry| entry.result::<Picked>());
            HomeScreen(picked.map(|Picked(color)| color), move || {
                nav.navigate(Screen::PickColor)
            });
        }
        Screen::PickColor => PickColorScreen(move |color| {
            if let Some(home) = nav.previous_back_stack_entry() {
                home.set_result(Picked(color));
            }
            nav.pop_back_stack();
        }),
    });
}

#[composable]
fn HomeScreen(_picked: Option<u32>, _pick: impl Fn() + 'static) {}

#[composable]
fn PickColorScreen(_done: impl Fn(u32) + 'static) {}
```

The type of a result is its key. Results live in memory and go with a popped
entry; a screen that shows a result for longer takes it into its own saved
state.

## Browser history

On the web, `BindBrowserHistory(controller, saver)` keeps the browser's history
in step with the back stack, as Compose Multiplatform's `bindToNavigation`
does. Each navigation adds an entry whose URL fragment is `#/` and the route's
saved form. The browser's Back pops and its Forward navigates again, and a
pop in the app goes back in the history. A page opened or reloaded at a
route's fragment shows that route over the start. Elsewhere the call does
nothing, so an app can make it unconditionally.

## Android API map

| Android API | Cranpose API |
|---|---|
| `rememberNavController()` and `startDestination` | `rememberNavController`, `NavController` |
| `NavHost` and typed destinations | `NavHost` with a route `match` |
| Enter and exit transitions | `NavHostWith` |
| `navigate`, `popBackStack`, `navigateUp` | `navigate`, `pop_back_stack`, `navigate_up` |
| `popUpTo`, inclusive pop, single top | `navigate_with`, `NavOptions` |
| Current back-stack entry | `current_route`, `back_stack` |
| `NavBackStackEntry`, `previousBackStackEntry`, `getBackStackEntry` | `NavBackStackEntry`, `composing_entry`, `previous_back_stack_entry`, `back_stack_entry` |
| Results through `savedStateHandle` | `NavBackStackEntry::set_result`, `result`, `take_result` |
| View model scoped to a back-stack entry | `cranpose_coroflow::viewModel` inside a screen |
| `rememberSaveable` in a destination | `rememberSaveable` inside a screen |
| Back stack restored after process death | `rememberSaveableNavController` with a route `Saver` |
| Compose Multiplatform `window.bindToNavigation` | `BindBrowserHistory` |
| Predictive back in `NavHost` | `NavHost` follows the gesture; `cranpose_services::PredictiveBackHandler` for custom screens |

- [API reference on docs.rs](https://docs.rs/cranpose-navigation/latest/cranpose_navigation/)
- [Source on GitHub](https://github.com/samoylenkodmitry/cranpose/tree/main/crates/cranpose-navigation)
