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
            GlassButtonLabel("Save".into(), GlassButtonSpec::prominent());
        });
    });
}
```

The app host supplies composition, input, draw output, and platform services.
`LiquidThemeSpec::palette` selects `LiquidPalette::System { scheme, accent }`
or `LiquidPalette::Custom(colors)`. The system palette defaults to
`SchemeMode::Auto`; `Light` and `Dark` pin its appearance. Custom palettes carry
their own `is_dark` value. `LiquidTheme` applies accessibility settings to both
sources and configures text-selection appearance from the resulting palette.

```rust
use cranpose_liquid::prelude::*;
use cranpose_ui_graphics::Color;

let mut colors = LiquidColors::dark(Color::from_rgb_u8(176, 230, 210));
colors.background = Color::from_rgb_u8(10, 40, 30);
let spec = LiquidThemeSpec {
    palette: LiquidPalette::Custom(colors),
    ..Default::default()
};
```

Custom palette inversion complements RGB channels and preserves alpha; increased
contrast follows inversion. Supply the original palette so `LiquidTheme` applies
these settings once. `GlassButtonSpec` selects glass, prominent, plain, or
destructive styles.

- [API reference on docs.rs](https://docs.rs/cranpose-liquid/latest/cranpose_liquid/)
- [Source on GitHub](https://github.com/samoylenkodmitry/cranpose/tree/main/crates/cranpose-liquid)
