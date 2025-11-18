# Modifier System Migration Tracker

## Status: ⚠️ Coordinator-based layout measure exists, but it clones nodes each pass and ignores unknown layout modifiers; draw/pointer/semantics still run off flattened snapshots.

## Baseline (useful context)

- Modifier chain uses `ModifierKind::Combined`; reconciliation via `ModifierChainHandle` feeds a node
  chain with capability tracking.
- Widgets emit `LayoutNode` + `MeasurePolicy`; dispatch queues keep pointer/focus flags in sync.
- Built-in layout modifier nodes exist (padding/size/fill/offset/text).
- Layout measurement goes through a coordinator chain
  (`crates/compose-ui/src/layout/mod.rs:725`), but coordinators clone built-ins, ignore unknown
  layout modifiers, and have stub placement (`crates/compose-ui/src/layout/coordinator.rs:141`).

## Architecture Overview

- **Widgets**: Pure composables emitting `LayoutNode`s with policies.
- **Modifier chain**: Builders chain via `then`; reconciliation still flattens into element vectors
  and a `ResolvedModifiers` snapshot for layout/draw.
- **Measure pipeline**: Coordinator chain exists but rehydrates built-ins and skips unknown layout
  modifiers; falls back to `ResolvedModifiers` when no layout nodes.
- **Text**: `TextModifierElement` stores only a `String`; measure uses monospaced stub, draw empty,
  semantics use `content_description`, no invalidations on update.
- **Invalidation**: Capability flags exist but layout/draw/semantics invalidations mostly come from
  the flattened snapshot.

## Unacceptable Gaps

### Modifier chain still flattened
- `ModifierChainHandle::update()` clones element/inspector vectors and rebuilds
  `ResolvedModifiers` every recomposition (`crates/compose-ui/src/modifier/chain.rs:71`), so the
  persistent `ModifierKind` tree is discarded and stacked properties collapse (padding summed,
  backgrounds last-write wins).

### Coordinator chain gaps
- Only built-in padding/size/fill/offset/text measured; custom/stateful layout modifiers skipped and
  nodes rehydrated each pass (`crates/compose-ui/src/layout/coordinator.rs:141`).
- Placement/draw/pointer/semantics coordinators absent; runtime still depends on `ResolvedModifiers`
  snapshots.

### Text pipeline gap
- `TextModifierElement` is string-only (`crates/compose-ui/src/text_modifier_node.rs:166`), measure
  uses monospaced stub, draw empty, semantics via `content_description`, no invalidations on update;
  widget surface lacks style/overflow/min/max lines, etc.

## Remaining Work

### 1. Coordinator chain: use reconciled nodes and cover all phases
- Invoke reconciled layout modifier nodes (or fall back to snapshot) instead of cloning adapters; add
  placement/draw/pointer/semantics/lookahead support so ordering/state persist.

### 2. Reconcile without flattening
- Walk the `ModifierKind::Combined` tree directly for chain + inspector updates; keep O(1) updates
  and avoid `ResolvedModifiers` except as temporary draw/semantics data.

### 3. Finish Text
- Mirror `TextStringSimpleElement` surface, add paragraph cache + renderer integration, issue
  invalidations on updates, expose real semantics, expand widget API.

### 4. Testing & integration
- Add pointer/focus tests through `HitTestTarget`; add text layout/draw/semantics coverage. Run
  `cargo test > 1.tmp 2>&1` after major changes.

## References

- Kotlin modifier pipeline: `/media/huge/composerepo/compose/ui/ui/src/commonMain/kotlin/androidx/compose/ui/Modifier.kt`
- Node coordinator chain: `/media/huge/composerepo/compose/ui/ui/src/commonMain/kotlin/androidx/compose/ui/node/LayoutModifierNodeCoordinator.kt`
- Text reference: `/media/huge/composerepo/compose/foundation/foundation/src/commonMain/kotlin/androidx/compose/foundation/text/BasicText.kt`
  and `.../text/modifiers/TextStringSimpleNode.kt`
- Detailed parity checklist: [`modifier_match_with_jc.md`](modifier_match_with_jc.md)
