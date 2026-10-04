# Cranpose Foundation

`cranpose-foundation` defines shared modifier-node, input, focus, text and
semantics contracts for Cranpose UI. Framework extension authors use the crate
to add modifier nodes and input handlers. App developers usually start with
the widgets and `Modifier` type from [`cranpose`](https://docs.rs/cranpose/latest/cranpose/).

## Handle a pointer event

`PointerEvent` carries event kind and logical pointer positions. The platform
adapter converts device coordinates before the UI layer receives an event.

```rust
use cranpose_foundation::{PointerEvent, PointerEventKind};
use cranpose_ui_graphics::Point;

let position = Point::new(24.0, 32.0);
let event = PointerEvent::new(PointerEventKind::Down, position, position);
```

Custom modifier nodes use `ModifierNode`, `ModifierNodeElement` and the
`impl_modifier_node!` helper. `cranpose-ui` supplies `Modifier::from_element`
as the app-facing entry point. See the
[`ModifierNode` API](https://docs.rs/cranpose-foundation/latest/cranpose_foundation/modifier/trait.ModifierNode.html),
the [modifier guide](https://github.com/samoylenkodmitry/Cranpose/blob/main/docs/MODIFIERS.md)
and the [crate source](https://github.com/samoylenkodmitry/Cranpose/tree/main/crates/cranpose-foundation).

The feature set is empty. The dependency set stays platform-neutral.
