# Modifier System Migration Tracker

## Status: ✅ NodeCoordinator chain implemented for layout measurement

`measure_through_modifier_chain()` now builds a proper `NodeCoordinator` chain using
`LayoutModifierCoordinator` instances that wrap each reconciled `LayoutModifierNode`, with
`InnerCoordinator` wrapping the `MeasurePolicy`. Measurement invokes the actual reconciled nodes
(not temporary copies) with a shared `LayoutNodeContext` that properly accumulates invalidations.
Text remains a string-only stub with monospaced measurement, empty draw, and placeholder semantics.

## Completed Work

1. ✅ **Dispatch queues integrated.** `AppShell::run_dispatch_queues`
   (`crates/compose-app-shell/src/lib.rs#L237-L275`) now drains pointer/focus invalidations so the
   capability flags on `LayoutNode` match Jetpack Compose's lifecycle.
2. ✅ **Legacy widget nodes deleted.** `Button`, `Text`, and `Spacer` all emit `LayoutNode` +
   `MeasurePolicy`; bespoke `measure_*` helpers are gone.
3. ✅ **Centralized modifier reconciliation.** `ModifierNodeChain` + `ModifierChainHandle` reconcile
   node instances with capability tracking and modifier locals.
4. ✅ **Persistent `Modifier::then`.** `ModifierKind::Combined` mirrors Kotlin's `CombinedModifier`
   (`crates/compose-ui/src/modifier/mod.rs:235-382`).
5. ✅ **Layout modifier node implementations.** Padding/Size/Offset/Fill/Text nodes expose full
   measure + intrinsic logic (`crates/compose-ui/src/modifier_nodes.rs`).
6. ✅ **NodeCoordinator chain for layout measurement.** `measure_through_modifier_chain()`
   (`crates/compose-ui/src/layout/mod.rs:726-854`) builds a coordinator chain using
   `LayoutModifierCoordinator` wrapping each `LayoutModifierNode` and `InnerCoordinator` wrapping
   the `MeasurePolicy`. Coordinators invoke `measure()` on reconciled nodes (not temporary copies)
   with a shared `LayoutNodeContext` that accumulates invalidations and processes them after
   measurement (`crates/compose-ui/src/layout/coordinator.rs`).

## Architecture Overview

- **Widgets**: Pure composables emitting `LayoutNode`s with policies (Empty/Flex/etc.).
- **Modifier chain**: Builders chain via `self.then(...)`, flatten into `Vec<DynModifierElement>`
  for reconciliation, and also collapse into a `ResolvedModifiers` snapshot for layout/draw.
- **Measure pipeline**: `measure_through_modifier_chain()` builds a `NodeCoordinator` chain,
  wrapping each reconciled `LayoutModifierNode` in a `LayoutModifierCoordinator` and the
  `MeasurePolicy` in an `InnerCoordinator`. Coordinators invoke `measure()` on the actual
  reconciled nodes with a shared `LayoutNodeContext`, enabling proper invalidation tracking.
  Falls back to `ResolvedModifiers` only when no layout modifier nodes are present in the chain.
- **Text**: `Text()` adds `TextModifierElement` + `EmptyMeasurePolicy`; the element stores only a
  `String`, the node measures via the monospaced stub in `crates/compose-ui/src/text.rs`, `draw()` is
  empty, `update()` cannot invalidate on text changes, and semantics just set `content_description`.
- **Invalidation**: Capability flags exist, but layout/draw/semantics invalidations come from the
  resolved snapshot rather than node-driven updates.

## Known Shortcuts

### Modifier chain still flattened each recomposition
- `ModifierChainHandle::update()` allocates fresh element/inspector vectors and rebuilds
  `ResolvedModifiers` on every pass (`crates/compose-ui/src/modifier/chain.rs:72-231`), so the
  persistent tree never reaches the runtime. Kotlin walks the `CombinedModifier` tree directly.

### Draw/pointer/semantics not yet using coordinator chain
- Layout measurement now uses the `NodeCoordinator` chain, but draw, pointer input, and semantics
  still consume `ResolvedModifiers` snapshots. The coordinator chain should be extended to support
  these phases, with `DrawModifierNodeCoordinator`, `PointerInputModifierNodeCoordinator`, etc.
- `ModifierChainHandle::compute_resolved()` still sums padding and overwrites later properties into a
  `ResolvedModifiers` snapshot (`crates/compose-ui/src/modifier/chain.rs:173-219`); stacked
  modifiers lose ordering and "last background wins" for properties not driven by modifier nodes yet.
- No lookahead/approach measurement hooks exist yet.

### Text modifier pipeline gap
- `TextModifierElement` captures only a `String`
  (`crates/compose-ui/src/text_modifier_node.rs:167-205`) and cannot invalidate on updates; style/
  font resolver/overflow/softWrap/minLines/maxLines/color/auto-size/placeholders/selection cannot
  reach the node.
- `TextModifierNode` uses monospaced measurement, has an empty `draw()`, and only sets
  `content_description` semantics; Kotlin’s `TextStringSimpleNode` manages paragraph caches,
  baselines, text substitution, and `SemanticsPropertyReceiver.text` (`.../text/modifiers/TextStringSimpleNode.kt`).
- `TextStringSimpleNode` manually invalidates layout/draw/semantics around a `ParagraphLayoutCache`
  while `shouldAutoInvalidate` is false; our `update()` cannot issue invalidations because it never
  sees a live `ModifierNodeContext`.
- The widget API (`crates/compose-ui/src/widgets/text.rs:125-143`) exposes neither style nor
  callbacks like `onTextLayout`, so parity with `BasicText` isn’t possible.

## Remaining Work

### 1. Extend coordinator chain to draw/pointer/semantics
- ✅ Layout measurement now uses `NodeCoordinator` chain with `LayoutModifierCoordinator`.
- Surface draw/pointer/semantics nodes from the same chain using similar coordinator wrappers,
  preserving modifier ordering for layers/clipping.
- Add lookahead/approach measurement hooks to the coordinator interface.

### 2. Preserve the persistent modifier tree during reconciliation
- Stop cloning intermediate element/inspector vectors; walk the `ModifierKind::Combined` tree
  directly when updating the chain/inspector snapshot so modifier updates stay O(1).

### 3. Finish the Text modifier pipeline
- Mirror Kotlin’s `TextStringSimpleElement` surface (style, `FontFamily.Resolver`, overflow,
  softWrap, minLines/maxLines, `ColorProducer`, auto-size, placeholders/selection hooks).
- Store a paragraph cache/renderer handle inside `TextModifierNode`, call into the existing external
  text renderer/paragraph library for measurement + draw, issue
  `invalidateMeasurement`/`invalidateDraw`/`invalidateSemantics` on updates (no auto invalidate), and
  expose real semantics (`text`, `getTextLayoutResult`, substitution toggles). Remove the runtime
  metadata fallback once semantics provide `SemanticsRole::Text`.
- Expand `Text`/`BasicText` to pass the full parameter set through the modifier element.

### 4. Testing & integration cleanup
- Add pointer/focus integration tests that dispatch through `HitTestTarget` instead of counting
  nodes; add text layout/draw/semantics assertions once the pipeline is wired.
- After each major change, run `cargo test > 1.tmp 2>&1` and inspect the log before iterating.

## References

- Kotlin modifier pipeline: `/media/huge/composerepo/compose/ui/ui/src/commonMain/kotlin/androidx/compose/ui/Modifier.kt`
- Node coordinator chain: `/media/huge/composerepo/compose/ui/ui/src/commonMain/kotlin/androidx/compose/ui/node/LayoutModifierNodeCoordinator.kt`
- Text reference: `/media/huge/composerepo/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/text/BasicText.kt`
  and `.../text/modifiers/TextStringSimpleNode.kt`
- Detailed parity checklist: [`modifier_match_with_jc.md`](modifier_match_with_jc.md)
