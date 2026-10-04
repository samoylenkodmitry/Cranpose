# Cranpose foundation

Core modifier and input contracts used by Cranpose UI. This crate defines
`ModifierNode`, `DelegatableNode` and `ModifierNodeElement`, plus shared pointer,
focus, gesture, text-input and semantics infrastructure. The `Modifier` value
and user-facing layout widgets live in `cranpose-ui`.

Use these contracts to create a custom modifier node. Create its modifier with
`Modifier::from_element` from `cranpose-ui`, then define the element's `create`
and `update` methods.
