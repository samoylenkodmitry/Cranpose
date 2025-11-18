# Modifier Migration Reality Check

Concise status of how the modifier system differs from Jetpack Compose and what to fix next.

## Current Snapshot (only what matters for remaining work)

- ✅ Modifier node chain + capability masks (`crates/compose-foundation/src/modifier.rs`); builders take `self` and use `then`; `ModifierKind::Combined` mirrors Kotlin (`crates/compose-ui/src/modifier/mod.rs:235`).
- ⚠️ Reconciliation still flattens modifiers into element/inspector vectors and a `ResolvedModifiers` snapshot every update (`crates/compose-ui/src/modifier/chain.rs:71,188`), losing structural sharing and modifier ordering.
- ⚠️ Layout measurement uses a coordinator chain (`crates/compose-ui/src/layout/mod.rs:725`), but `LayoutModifierCoordinator` clones padding/size/fill/offset/text nodes each measure, ignores unknown layout modifiers, and has stub placement (`crates/compose-ui/src/layout/coordinator.rs:141`).
- ⚠️ Draw/pointer/semantics still read `ResolvedModifiers`; padding/offset/background/layers collapse to last-write wins.
- ⚠️ Text is string-only: monospaced measure (`crates/compose-ui/src/text.rs:1`), empty draw, semantics via `content_description` (`crates/compose-ui/src/text_modifier_node.rs:159`), no invalidations on update; widget surface lacks style/overflow/min/max lines.
- ⚠️ Pointer integration tests still count nodes instead of dispatching through `HitTestTarget` (`crates/compose-ui/src/tests/pointer_input_integration_test.rs`).

## Gaps vs Jetpack Compose

### Flattened reconciliation
- Problem: Combined tree discarded; `ResolvedModifiers` snapshot collapses ordering and per-node state.
- Fix: Walk the Combined tree directly for chain + inspector updates; pipe phase work through reconciled nodes, not snapshots.

### Coordinator chain stops at measurement
- Problem: Built-in layout nodes are cloned and unknown ones are skipped; placement/draw/pointer/semantics/lookahead not handled.
- Fix: Invoke reconciled layout modifier nodes (or explicitly fall back), implement placement/draw/hit-test/semantics/lookahead coordinators, and drop padding/size/offset munging once node-driven layout is authoritative.

### Text pipeline mismatch
- Problem: Missing style/resolver/overflow/softWrap/min/max lines; no paragraph cache/renderer; no invalidations; semantics rely on `content_description`.
- Fix: Mirror `TextStringSimpleElement`, add paragraph cache + renderer integration, call invalidateMeasurement/Draw/Semantics on updates, expose real semantics, expand `Text`/`BasicText` API, remove metadata fallback.

### Tests
- Problem: No pointer/focus/text integration coverage through runtime paths.
- Fix: Add pointer/focus tests via `HitTestTarget`; add text layout/draw/semantics assertions once pipelines are node-driven.

## References

- Kotlin modifier pipeline: `/media/huge/composerepo/compose/ui/ui/src/commonMain/kotlin/androidx/compose/ui/Modifier.kt`
- Coordinator chain: `/media/huge/composerepo/compose/ui/ui/src/commonMain/kotlin/androidx/compose/ui/node/LayoutModifierNodeCoordinator.kt`
- Text: `/media/huge/composerepo/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/text/BasicText.kt` and `.../text/modifiers/TextStringSimpleNode.kt`
