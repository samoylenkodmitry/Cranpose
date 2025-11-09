# Next Task: Chain-driven background rendering

## Context
`ModifierChainHandle::resolved_modifiers()` currently only captures padding, so renderers still peek into `ModifierState` via `Modifier::background_color()`/`corner_shape()` when issuing draw commands. That bypasses the node chain, ignores ordering, and prevents Jetpack-style invalidations from working for draw modifiers. Now that padding runs through nodes, the next step is to migrate background rendering to the same resolved pipeline.

## Goals
1. Extend `ResolvedModifiers` to expose an optional background descriptor built from draw-capable nodes (`BackgroundNode`, future draw nodes).
2. Update renderers (`crates/compose-ui/src/renderer.rs`, `compose-render/*/pipeline/style.rs`) to consume the resolved background instead of reading `ModifierState`.
3. Verify via tests that background data flows through the chain, respects modifier ordering, and no longer depends on the legacy state shortcuts.

## Suggested Steps
1. **Extend resolved data**
   - Introduce a `ResolvedBackground` struct (color + optional `RoundedCornerShape`) and add a `background: Option<ResolvedBackground>` field + accessors on `ResolvedModifiers`.
   - Update `ModifierChainHandle::resolved_modifiers()` to iterate `chain.draw_nodes()`, downcast to `BackgroundNode`, and record the latest node's color/shape (mirror Kotlin’s last-wins behavior). Expose helpers such as `resolved.background()` for consumers.
2. **Teach nodes to surface draw data**
   - Ensure `BackgroundNode` (and its element) stores the color (and shape/brush if available) and provides getters so the chain handle can read it. Keep `Modifier::background` returning `Modifier` unchanged, but stop relying on `ModifierState` for draw data once the chain is authoritative.
3. **Update renderers and style builders**
   - Plumb `LayoutNodeData::resolved_modifiers()` into `evaluate_modifier` inside `crates/compose-ui/src/renderer.rs` and replace the existing `modifier.background_color()` logic with the resolved background descriptor.
   - Mirror the change in `compose-render/pixels/src/style.rs` and `compose-render/wgpu/src/pipeline/style.rs` so platform backends collect color/shape from the resolved modifiers.
4. **Tidy tests + compatibility**
   - Extend `crates/compose-ui/src/modifier/tests/modifier_tests.rs` or `modifier_nodes_tests.rs` with assertions that `ModifierChainHandle::resolved_modifiers().background()` reflects the latest background node and that removing the modifier clears it.
   - Update `crates/compose-ui/src/tests/renderer_tests.rs` (or add a new test) to ensure rendering still emits the correct background primitives when only node-backed data is present.
5. **Housekeeping**
   - Once renderers trust the resolved data, remove or deprecate any now-unused `ModifierState` background helpers (or keep them only for transitional logging). Document the new flow in `modifier_match_with_jc.md`.
6. Run `cargo fmt` and `cargo test -p compose-ui modifier_tests modifier_nodes_tests renderer_tests` (or `cargo test -p compose-ui` if faster) to verify the migration.

## Reference Files
- `crates/compose-ui/src/modifier/mod.rs`
- `crates/compose-ui/src/modifier/chain.rs`
- `crates/compose-ui/src/modifier_nodes.rs`
- `crates/compose-ui/src/renderer.rs`
- `crates/compose-render/pixels/src/style.rs`
- `crates/compose-render/wgpu/src/pipeline/style.rs`
- `crates/compose-ui/src/tests/modifier_nodes_tests.rs`
- `crates/compose-ui/src/tests/renderer_tests.rs`

## Definition of Done
- `ResolvedModifiers` provides a background descriptor populated from draw nodes.
- Renderer/headless + external pipelines use the resolved background data instead of `ModifierState`.
- Tests cover the new resolved background behavior, and `cargo test -p compose-ui …` completes successfully.
