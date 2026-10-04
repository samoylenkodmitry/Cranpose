# cranpose-liquid

`cranpose-liquid` provides Cranpose's glass surfaces, controls, semantic colors,
typography, and motion. Apps use the crate for buttons, cards, menus, sliders,
tabs, toggles, and themed surfaces across Cranpose targets.

Depend directly on `cranpose-liquid` when an app uses these components. The
feature set is empty. The app also needs `cranpose` with its host and
renderer features, plus the usual Cranpose UI setup.

## Theme and button

`LiquidTheme` supplies colors and typography to its child components. A button
uses a `GlassButtonSpec` and composes a `GlassButtonLabel`:

```rust
use cranpose_liquid::{
    GlassButton, GlassButtonLabel, GlassButtonSpec, LiquidTheme, LiquidThemeSpec,
};
use cranpose_ui::{Modifier, composable};

#[composable]
fn SaveAction(on_save: impl Fn() + 'static) {
    LiquidTheme(LiquidThemeSpec::default(), move || {
        GlassButton(Modifier::empty(), GlassButtonSpec::prominent(), on_save, || {
            GlassButtonLabel("Save", GlassButtonSpec::prominent());
        });
    });
}
```

The app host supplies composition, input, draw output, and platform services.
`SchemeMode::Auto` follows the system appearance; `Light` and `Dark` pin a
palette. `GlassButtonSpec` selects glass, prominent, plain, or destructive
styles.

- [API reference on docs.rs](https://docs.rs/cranpose-liquid/latest/cranpose_liquid/)
- [Source on GitHub](https://github.com/samoylenkodmitry/cranpose/tree/main/crates/cranpose-liquid)
