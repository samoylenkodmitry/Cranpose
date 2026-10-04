# Cranpose UI Layout

`cranpose-ui-layout` defines measurement, placement, alignment and arrangement
contracts for Cranpose widgets. Custom layout authors use `Constraints`,
`Measurable`, `MeasureScope` and `Placeable`. App developers usually use
`Column`, `Row`, `Box` and `Layout` from
[`cranpose`](https://docs.rs/cranpose/latest/cranpose/).

## Adjust child constraints

Constraints use logical pixels. A custom measure policy can deflate parent
bounds before a child measure call:

```rust
use cranpose_ui_layout::Constraints;

let parent = Constraints::loose(320.0, 480.0);
let child = parent.deflate(16.0, 12.0);
let (width, height) = child.constrain(400.0, 40.0);
assert_eq!((width, height), (304.0, 40.0));
```

The feature set is empty. See the
[`Constraints` API](https://docs.rs/cranpose-ui-layout/latest/cranpose_ui_layout/struct.Constraints.html),
the [layout guide](https://github.com/samoylenkodmitry/Cranpose/blob/main/docs/guide.md#layout-and-lists)
and the [crate source](https://github.com/samoylenkodmitry/Cranpose/tree/main/crates/cranpose-ui-layout).
