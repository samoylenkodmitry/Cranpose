# Native windows as composition roots

Cranpose represents each native window as a root inside the app's composition.
A window modifier marks the layout node whose content belongs to a native
window. The app keeps one composition and moves content with its remembered
state when the tree changes.

## Window roots

`WindowModifierExt::window` attaches a `WindowConfig` to a modifier chain.
The config supplies window state and platform options such as title, size,
position, decorations, transparency, focus and size changes. The modifier node
registers a root in the app context's `WindowRootRegistry`. The layout node
identifies the window across recomposition. The platform reads the registry,
creates or updates native surfaces, and asks the scene builder to render each
root. See [`native_window.rs`](../crates/cranpose/src/native_window.rs),
[`window_root.rs`](../crates/cranpose-ui/src/modifier/window_root.rs) and
[`desktop.rs`](../crates/cranpose/src/desktop.rs).

`LocalWindowState::current()` gives composables access to the state for their
window. Window drag and resize areas, pointer position, and cross-window drag
and drop connect input to its native surface. See
[`window_local.rs`](../crates/cranpose/src/window_local.rs) and
[`modifier/drag_and_drop.rs`](../crates/cranpose-ui/src/modifier/drag_and_drop.rs).

## Movable content

`cranpose_core::movable(key, content)` preserves a keyed subtree when
composables place the subtree under a different parent. Remembered values,
effects and nodes travel with the subtree. `forget_movable(key)` releases
retained content when the app ends its use. Slot anchors and retained subtrees provide the
move lifecycle; see [`movable`](../crates/cranpose-core/src/lib.rs) and
[`slot/movable.rs`](../crates/cranpose-core/src/slot/movable.rs).

## Design rationale

The composition tree owns window content. A native window acts as a render
root for an existing subtree, so a window transition preserves the subtree's
composition identity. A modifier expresses the relationship at the content
node. `WindowRootRegistry` gives the platform a small surface
contract while the framework owns composition, layout and scene construction.

Desktop examples exercise tabs, grouped tool windows and transparent shader
windows in [`apps/desktop-demo/src/app`](../apps/desktop-demo/src/app).
