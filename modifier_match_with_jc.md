# Modifier ≈ Jetpack Compose Parity Plan

Goal: match Jetpack Compose's `Modifier` API surface and `Modifier.Node` runtime semantics so Kotlin samples and mental models apply 1:1 in Compose-RS.

---

## Current Gaps (Compose-RS)
- `Modifier` is an Rc-backed builder that caches layout/draw state; primitives read baked values instead of delegating to `Modifier.Node`s (`ModifierState`), so ordering, invalidation, and lifetimes differ from Jetpack Compose.
- Although `compose_foundation::ModifierNodeChain` exists, composables never reconcile one from their `Modifier`s, so nodes never attach to layout/draw/pointer pipelines.
- Traits like `ModifierElement`/`ModifierNode` still lack Kotlin parity features such as capability bitmasks, coroutine scopes, semantics/pointer slices, etc. (fold helpers + inspector metadata now exist, so the remaining work is lifecycle + capability plumbing).

## Jetpack Compose Reference Anchors
- `Modifier` is an immutable interface implemented by `EmptyModifier` and `CombinedModifier`, exposing `foldIn`, `foldOut`, `any`, `all`, and `then`.
- Every `Modifier.Element` that owns state has a matching `ModifierNodeElement` responsible for creating/updating a long-lived `Modifier.Node`.
- `NodeChain` diffing keeps node instances stable, wires parent/child links, tracks capability bitmasks, and services targeted invalidations for layout/draw/pointer/semantics.

## Migration Plan
1. **Mirror the data model**
   - Reintroduce Kotlin-style `Modifier` trait semantics (fold helpers, `any`, `all`, inspector stubs) while keeping existing APIs source-compatible.
   - Remove `ModifierState` responsibilities by moving cached state into real nodes.
2. **Adopt `ModifierNodeElement` / `Modifier.Node` parity**
   - Port Kotlin contracts (create/update/key/hash) into `compose_foundation`; extend `Modifier.Node` with lifecycle + capability tracking identical to Jetpack Compose.
   - Flesh out `ModifierNodeChain` with sentinel head/tail, diffing (linear + Myers fallback), and coordinator hooks.
3. **Wire runtime subsystems through chains**
   - Layout: each composable owns a `ModifierNodeChain`, updates it from the incoming `Modifier`, and runs measurement/intrinsics via layout nodes.
   - Draw & pointer input: renderer and dispatcher walk chain slices filtered by capability bitmasks; semantics tree mirrors Kotlin construction.
4. **Migrate modifier factories**
   - Re-implement padding/background/clickable/drawBehind/etc. as node-backed elements; delete value-based `ModifierState` usage once coverage is complete.
5. **Compatibility, validation, and tooling**
   - Provide temporary shims for legacy APIs during migration.
   - Add exhaustive tests (node reuse, invalidation targeting, pointer dispatch) plus desktop demos matching Kotlin expectations.

## Open Questions
- How much of Kotlin's `NodeCoordinator` / `LayoutNode` machinery should be transliterated versus adapted to the existing renderer?
- When to bring over inspector/debug metadata versus stubbing until desktop tooling lands?
- Should `ModifierState` remain behind a feature flag for compatibility, or be removed outright once node-backed equivalents exist?

## Near-Term Next Steps
1. **Make layout primitives consume `ModifierNodeChain` output**  
   - `LayoutNode` already keeps a `ModifierChainHandle`, but Column/Row/Box still read sizes, weights, and padding from `ModifierState`. Thread `ResolvedModifiers` (and direct chain traversal when needed) into the layout policies so measurement/intrinsics honor node ordering and capability slices.
2. **Migrate draw/backdrop modifiers to chain-driven resolved data**  
   - Teach `ModifierChainHandle::resolved_modifiers` to aggregate `BackgroundNode`/draw nodes (color, rounded corners, graphics layer) and update renderers to rely on the resolved data instead of `Modifier::background_color`/`corner_shape`.
3. **Capability-driven invalidation + diagnostics**  
   - Track node capability bitmasks, route `InvalidationKind` back into layout/draw/input dirty flags, and add a tracing hook (`COMPOSE_DEBUG_MODIFIERS`) that dumps node churn + invalidation reasons to speed up future migrations.

---

## Progress Snapshot (padding migration – 2024-XX-XX)
- `ModifierChainHandle` now lives inside `LayoutNode`/`SubcomposeLayoutNode`; every layout node reconciles its chain each frame and drains invalidations.
- New `ResolvedModifiers` struct captures runtime-only data (currently node-driven padding) so measurement/render stacks no longer read `ModifierState`.
- Padding modifiers exclusively emit `PaddingElement`s; layout/subcompose measurement subtract/adds padding from the resolved node data, matching Kotlin ordering.
- Renderers (`headless`, `pixels`, `wgpu`) pull visual padding via `ResolvedModifiers`, keeping hit regions/text bounds consistent after the migration.
- `Modifier` exposes Kotlin-style `foldIn`/`foldOut`/`any`/`all` helpers, `then` now mirrors Kotlin's short-circuit/ordering semantics, and the new `InspectorMetadata` helper records padding/background properties so tests can assert inspector output.

## Progress Snapshot (inspector metadata rollout – 2024-XX-XX)
- `InspectorInfo` now exposes reusable helpers (`add_dimension`, `add_alignment`, `add_offset_components`, `describe`, `debug_properties`) so modifier factories can record structured metadata and tests/logs can dump it without poking internal structs.
- Common factories (`size`, `fillMax*`, `offset`, `align*`, `clip_to_bounds`, `clickable`, `graphics_layer`, padding/background) call `with_inspector_metadata`, ensuring metadata ordering matches modifier insertion order and surfaces all key parameters.
- `modifier_tests` assert metadata emission for layout + interaction modifiers, verify ordering, and cover the new debugging helper, giving us Kotlin-style inspector coverage for future migrations.

### Follow-up tasks for the next agent
1. **Integrate with real layout primitives**  
   - Column/Row/Box still read `LayoutProperties` (weight/align/padding) directly from `ModifierState`. Start pulling padding/offset/weight from `ResolvedModifiers` + chain traversal so layout policies use node-backed data and ordering matches Kotlin.
2. **Migrate draw modifiers**  
   - Repeat the padding flow for `Modifier::background` (and friends). Add a `BackgroundNode`-driven resolved property (color + shape) and update renderers/tests so draw no longer consult `ModifierState`.
3. **Capability-driven invalidations + diagnostics**  
   - `ModifierChainHandle::sync` still discards draw/pointer/semantics invalidations. Plumb them through `LayoutNode`, log via `COMPOSE_DEBUG_MODIFIERS`, and add node-chain dumps in `debug_tests` to catch regressions early.
4. **Documentation/tests**  
   - Extend `modifier_nodes_tests` with node-backed background assertions and add an integration test that recomputes layout after chain-driven padding/background changes without touching `ModifierState`.

Keep referencing the Kotlin sources under `/media/huge/composerepo/compose/ui/ui/src/commonMain/kotlin/androidx/compose/ui` for behavioral parity, especially `Modifier.kt`, `ModifierNodeElement.kt`, and `NodeChain.kt`.

---

## Detailed Execution Notes (current sprint)

### 1. Chain wiring inside `LayoutNode`
- Add a `ModifierChainHandle` field to `LayoutNode`/`SubcomposeLayoutNode` (`crates/compose-ui/src/widgets/nodes/layout_node.rs`) so every layout primitive owns the reconciled chain instead of calling `Modifier::resolve_*` helpers.
- During `LayoutNode::update`, call `handle.update(modifier)` and stash the returned `ResolvedModifiers`; this becomes the single source of truth for padding, background, and future layout modifier outputs.
- When measuring Column/Row (`crates/compose-ui/src/layout/mod.rs`), pull layout modifier nodes via `handle.chain().layout_nodes()` and run them before the intrinsic/measure passes. Start with layout-affecting nodes (padding, offset) and gate each subsystem behind a capability bitmask to mirror Kotlin's `DelegatableNode`.
- Propagate the handle to renderers by exposing `LayoutNode::resolved_modifiers()` so the draw tree can ask for background/graphics-layer state without re-reading `ModifierState`.

### 2. Background/draw migration
- Implement `BackgroundElement` + `BackgroundNode` in `crates/compose-ui/src/modifier/background.rs`/`crate::modifier_nodes`, mirroring `PaddingElement` but targeting draw capability bits and storing brush/shape/resolved alpha.
- Update `ModifierChainHandle::resolved_modifiers()` to detect `BackgroundNode` instances and populate a new background slot in `ResolvedModifiers`.
- Thread the resolved background through `compose-render/*/pipeline.rs` so both the pixels and wgpu backends issue draw commands directly from node data. Remove the legacy `ModifierState`-driven background reads after parity tests pass.
- Add targeted clippy-safe tests in `crates/compose-ui/src/modifier/tests/modifier_tests.rs` that verify background nodes invalidate draw only, reuse instances when the brush is unchanged, and survive `Modifier` reordering.

### 3. Capability-driven invalidations
- Extend `ModifierNodeChain` diffing so each node tracks a bitset of the capabilities it implements (layout/draw/pointer/semantics). Kotlin's `NodeKind` enum is the reference for the exact bits to copy.
- Update `ModifierChainHandle::take_invalidations()` to translate each `InvalidationKind` into `LayoutNode::invalidate_layout`, `invalidate_draw`, or `invalidate_pointer_input`. Start by toggling per-node dirty flags in `LayoutNode` and logging via `trace!` until the renderer consumes them.
- Surface the pending invalidation flags through `crates/compose-ui/src/renderer.rs` so renderers short-circuit when no draw nodes are dirty, matching Jetpack Compose's conditional re-draw behavior.

### 4. Testing and instrumentation
- Strengthen unit coverage in `crates/compose-ui/src/modifier/tests/` and `modifier_nodes_tests` by asserting: (a) nodes are reused when `equals` is stable, (b) invalidations fire when mutating node state, and (c) removing a modifier clears stale resolved data (padding/background zero out).
- Add a regression test under `crates/compose-ui/src/tests/debug_tests.rs` that composes a Column with dynamic padding/background changes and verifies layout is recomputed by reading `LayoutNode::last_measure_version`.
- Capture profiling samples with `cargo test -p compose-ui modifier_nodes_tests -- --nocapture` and note any hotspots in `ModifierNodeChain::update_from_slice`; if needed, port Kotlin's gap buffer diff to avoid churn when large modifier stacks change a single element.

### 5. Modifier builder cleanup
- Added an `InspectorMetadata` helper carried alongside each `Modifier` so factories can describe themselves for tooling/tests.
- Updated `Modifier::then` to short-circuit empty chains, preserve element ordering, and reuse the backing storage instead of cloning vectors unnecessarily.
- Extended `modifier_tests` with coverage for `then` semantics and inspector metadata serialization (padding + background) to prevent regressions.

---

## Backlog: Kotlin Parity Focus Areas

### Modifier API cleanup (builder-level parity)
- ✅ Ported Kotlin's `foldIn`, `foldOut`, `any`, `all`, and stub inspector metadata into `crates/compose-ui/src/modifier/mod.rs`, giving `Modifier` the same trait surface Jetpack exposes. Future code can now fold/predicate without depending on `ModifierState`.
- ✅ Refactored `Modifier::then` so it short-circuits empty modifiers, keeps element order stable, and records inspector metadata from participating factories (padding/background so far).
- Leverage the new inspector metadata + debugging surface inside devtools/logging paths so future modifiers stay instrumented as we migrate them to nodes.
- Introduce `Modifier.Element` markers by splitting `DynModifierElement` into `ModifierNodeElement` (node-backed) and legacy value elements, keeping both until migration is complete; the Kotlin sources show how factories expose ergonomics while still returning `Modifier`.
- Thread equality/hash contract hooks (Kotlin's `InspectorInfo`) into `ModifierElement` implementations. Even if we ignore tooling for now, honoring `equals/hashCode` is required so `ModifierNodeChain` can reuse entries without relying solely on `TypeId`.

### ModifierNodeChain + lifecycle parity
- Extend `ModifierNodeChain` (`crates/compose-foundation/src/modifier.rs:385`) with sentinel head/tail nodes, parent/child links, and capability bitmasks stored on the node structs themselves. Kotlin's `NodeCoordinator` assumes this structure to walk slices without allocation.
- Replace the current `Vec::remove` diff with the linear scan + Myers fallback used upstream so large modifier stacks (e.g., pointer filters) don't reallocate when a middle element toggles. Benchmark before/after to ensure update time matches Compose on similar traces.
- Propagate lifecycle hooks all the way through the chain: invoke `on_detach` when nodes leave, call `on_reset` when reusing with a different parent, and bubble `request_update` results out of `BasicModifierNodeContext` so layout nodes can schedule async updates.
- Expose capability-filtered iterators (`layout_nodes_mut`, `draw_slice`, etc.) that mirror Kotlin's `DelegatableNode` walking APIs; this lets layout/draw/pointer code avoid downcasting at call sites.

### Pointer input, semantics, and focus
- Flesh out the pointer pipeline by making `ModifierChainHandle` export a dedicated iterator over `PointerInputNode`s and teaching `crates/compose-ui/src/renderer.rs` + `pointer_input.rs` to dispatch events through that chain instead of consulting `ModifierState`.
- Port Jetpack Compose's `PointerInputModifierNode` gesture lifecycle (awaitPointerEventScope, cancellation) to `crate::modifier_nodes` so more complex modifiers like `clickable` can graduate from the ModOp system.
- Add semantics/focus node scaffolding parallel to `PointerInputNode`, wiring invalidations through `InvalidationKind::Semantics` so accessibility tree updates can be targeted once the surfaces exist.

### Tooling + diagnostics
- Attach lightweight tracing to `ModifierChainHandle::update` and `take_invalidations` so we can log node churn, invalidation types, and capability coverage per layout node (guard behind `COMPOSE_DEBUG_MODIFIERS` env var).
- Mirror Kotlin's `ModifierLocal` debugging by dumping the reconciled chain for a selected layout node in `crates/compose-ui/src/tests/debug_tests.rs`, ensuring future regressions are observable via snapshot tests.
