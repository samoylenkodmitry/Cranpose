# Cranpose UI Graphics

`cranpose-ui-graphics` provides geometry, units, colors, brushes, paths and
render-effect data. Widget authors use these values to describe visual
content. Renderer authors consume the same values to produce pixels.
Application code can import common types through
[`cranpose::prelude`](https://docs.rs/cranpose/latest/cranpose/prelude/).

## Define geometry and color

```rust
use cranpose_ui_graphics::{Color, Dp, Point, Rect, Size};

let padding = Dp(16.0);
let bounds = Rect::from_origin_size(Point::new(8.0, 12.0), Size::new(120.0, 48.0));
let accent = Color::from_rgb_u8(40, 110, 220);
```

The feature set is empty. `Dp`, `Sp` and `Px` remain distinct units;
`Density` converts logical values to the device grid. See the
[`Density` API](https://docs.rs/cranpose-ui-graphics/latest/cranpose_ui_graphics/unit/struct.Density.html),
the [graphics API](https://docs.rs/cranpose-ui-graphics/latest/cranpose_ui_graphics/)
and the [crate source](https://github.com/samoylenkodmitry/Cranpose/tree/main/crates/cranpose-ui-graphics).
