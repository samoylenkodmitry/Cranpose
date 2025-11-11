# Next Task: Pointer/Focus Manager Wiring & Capability Contract Polish

## Context
Capability-driven traversal is in place (`ModifierNodeChain::for_each_node_with_capability` mirrors Kotlin’s `NodeChain.forEachKind`), and `LayoutNode` now raises pointer/focus invalidations via `needs_pointer_pass` and `needs_focus_sync`. However, those queues are never drained outside of tests—the app shell simply marks the scene dirty, so pointer/focus updates still rely on layout/draw side effects. At the same time, the public guidelines still tell authors to override `as_draw_node`/`as_pointer_input_node`, even though Kotlin’s `Modifier.Node` contract relies solely on capability masks. We need to finish the Kotlin parity story by wiring the new flags into the runtime (pointer dispatcher + focus manager) and by polishing the API/docs so third-party nodes can depend on masks instead of bespoke `as_*` hooks.

## Current State
- `LayoutNode::dispatch_modifier_invalidations` sets `needs_pointer_pass`/`needs_focus_sync` and flips the new `request_pointer_invalidation`/`request_focus_invalidation` atomics, but `compose-app-shell` ignores those flags beyond re-rendering. Pointer input still only reprocesses events during layout/draw, and `FocusManager` never sees the new invalidations.
- `ModifierNode` still exposes the `as_*` helpers, every built-in node overrides them manually, and our docs/tests keep referencing that pattern. Kotlin’s `ModifierNodeElement.kt` only requires the node to set the capability mask + implement the specialized trait.
- We added a regression test for “mask-only” nodes, yet the runtime still depends on `as_*` to obtain `DrawModifierNode`/`PointerInputNode` trait objects. Without a helper or derive macro, downstream authors still have to write the overrides by hand.

## Goals
1. **Service pointer invalidations without layout/draw** — Drain `needs_pointer_pass` in the same places Kotlin’s `Owner.onInvalidatePointerInput` fires so pointer repasses happen immediately and only when necessary.
2. **Surface focus invalidations to `FocusManager`** — Bubble `needs_focus_sync` through a dedicated queue so focus targets/requesters update state without piggybacking on layout.
3. **Polish the capability contract** — Provide helpers/docs/tests so setting capability bits + implementing the specialized trait is enough; the runtime should stop telling authors to override `as_*`.

## Jetpack Compose Reference
- `androidx/compose/ui/node/NodeChain.kt` (`invalidateKind(NodeKind.Pointer/Focus)`) and `Owner.onInvalidatePointerInput/onInvalidateFocus` for how pointer/focus queues get serviced.
- `androidx/compose/ui/input/pointer/PointerInputDelegatingNode.kt` for pointer repass scheduling.
- `androidx/compose/ui/focus/FocusOwner.kt` + `FocusTargetNode.kt` for focus invalidation flows.
- `androidx/compose/ui/Modifier.kt` / `ModifierNodeElement.kt` for the capability-driven contract that avoids `as_*` helpers.

## Implementation Plan

### Phase 1 — Pointer invalidation servicing
1. Teach `LayoutEngine`/`AppShell` to poll `LayoutNode::needs_pointer_pass()` after composition/layout and enqueue a “pointer repass” task instead of only toggling `scene_dirty`.
2. Extend the pointer input stack (`crates/compose-ui/src/modifier/pointer_input.rs` + renderers) with a `request_repass()` API that mirrors Kotlin’s `PointerInputDelegatingNode.requestPointerInput`.
3. When a repass is scheduled, drain the pointer chain using the existing capability visitors and clear `needs_pointer_pass`/`request_pointer_invalidation()`. Add regression tests that mutate pointer modifiers without touching layout and assert that handlers rerun.

### Phase 2 — Focus invalidation servicing
1. Add a focus invalidation queue (similar to Kotlin’s `FocusInvalidationManager`) that tracks `LayoutNode`s with `needs_focus_sync`.
2. Integrate the queue with `FocusManager` so `FocusTargetNode`/`FocusRequesterNode` refresh their state immediately, clearing `needs_focus_sync` without forcing layout.
3. Cover scenarios like `FocusRequester.requestFocus()` + modifier updates with tests to ensure focus invalidations no longer depend on layout dirtiness.

### Phase 3 — Capability contract polish
1. Introduce a helper (derive macro or blanket impl) that automatically wires `as_draw_node`/`as_pointer_input_node`/etc. when a node implements the corresponding specialized trait, eliminating boilerplate for third parties.
2. Update docs (`modifier_match_with_jc.md`, inline Rustdoc) and samples to emphasize “set `NodeCapabilities`, implement the trait” instead of overriding `as_*`.
3. Audit every `ModifierNodeElement::capabilities()` for accuracy and add regression tests proving a node that only sets the capability bit (no manual override) still participates in draw/pointer/focus traversals.

## Acceptance Criteria
- Pointer/focus invalidations are fully drained via runtime queues; they no longer rely on layout/draw flags, and end-to-end tests confirm repasses/focus updates trigger without a measure/layout pass.
- The pointer dispatcher and `FocusManager` expose new APIs/hooks that mirror Jetpack Compose’s targeted routing.
- Library docs/samples/tests no longer instruct users to override `as_*`; capability bits + specialized traits suffice, with regression tests covering mask-only nodes.
- `cargo fmt`, `cargo clippy --all-targets --all-features`, and `cargo test` continue to pass.
