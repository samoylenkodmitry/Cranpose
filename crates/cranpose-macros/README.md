# cranpose-macros

`cranpose-macros` provides `#[composable]` and `#[preview]`, the procedural
macros used by Cranpose components. Depend directly on the crate when a
framework crate defines composables or preview fixtures. App code can import
the macros through [`cranpose`](https://docs.rs/cranpose/latest/cranpose/).

## Composable functions

The `#[composable]` macro gives each function call a composition group. The
call-site identity keeps groups stable across recomposition. Value parameters
use `Clone` and `PartialEq`; callback parameters use stored callback slots, so a
new closure value reaches its composition position. `#[composable(no_skip)]`
re-runs the body on each call and stores no parameters. It opens no group: it
composes in its caller's group, where what it composes is matched by position,
as a loop's items are, so a state it or its content reads recomposes the
composable that called it, as with Compose's inline `Box` and `Row`.

This app example uses the facade and UI dependencies:

```text
use cranpose::*;

#[composable]
fn Greeting(name: String) {
    Text(name, Modifier::empty(), TextStyle::default());
}
```

`#[preview]` registers a parameterless component fixture for IDE previews.
Combine `#[preview]` with `#[composable]`. The `cranpose` `preview` feature
enables preview registration. Options include `name`, `group`, `width`,
`height`, and `dark`.

The `hot-reload` feature selects source-structure keys for development hot reload.

- [API reference on docs.rs](https://docs.rs/cranpose-macros/latest/cranpose_macros/)
- [Source on GitHub](https://github.com/samoylenkodmitry/cranpose/tree/main/crates/cranpose-macros)
